use anyhow::{bail, Context, Result};
use chrono::Utc;
use netvan_core::paths::data_dir;
use netvan_core::types::SpeedtestResult;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::Value;
use std::env;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::win_cmd;

static SPEEDTEST_CHILD: Lazy<Mutex<Option<Child>>> = Lazy::new(|| Mutex::new(None));
static SPEEDTEST_CANCELLED: AtomicBool = AtomicBool::new(false);

/// Kill the in-flight speedtest CLI process, if any.
pub fn cancel_speedtest() -> Result<()> {
    SPEEDTEST_CANCELLED.store(true, Ordering::SeqCst);
    let mut guard = SPEEDTEST_CHILD.lock();
    if let Some(child) = guard.as_mut() {
        let _ = child.kill();
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct SpeedtestProgress {
    pub phase: String,
    pub download_mbps: Option<f64>,
    pub upload_mbps: Option<f64>,
    pub ping_ms: Option<f64>,
    pub server_name: Option<String>,
}

pub fn resolve_cli_path(configured: Option<&str>) -> PathBuf {
    if let Some(p) = configured {
        let path = PathBuf::from(p);
        if path.exists() {
            return path;
        }
    }

    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(cwd) = env::current_dir() {
        candidates.push(cwd.join("speedtest").join("speedtest.exe"));
    }
    if let Ok(exe) = env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            candidates.push(exe_dir.join("speedtest").join("speedtest.exe"));
            if let Some(parent) = exe_dir.parent() {
                candidates.push(parent.join("speedtest").join("speedtest.exe"));
            }
        }
    }
    candidates.extend([
        data_dir().join("speedtest.exe"),
        data_dir().join("ookla").join("speedtest.exe"),
        PathBuf::from("speedtest.exe"),
        PathBuf::from(r"C:\Program Files\Ookla\Speedtest CLI\speedtest.exe"),
    ]);

    for c in candidates {
        if c.exists() {
            return c;
        }
    }
    PathBuf::from("speedtest")
}

pub async fn run_speedtest(
    nic_id: Option<String>,
    server_id: Option<String>,
    cli_path: Option<String>,
    accept_eula: bool,
) -> Result<SpeedtestResult> {
    run_speedtest_with_progress(nic_id, server_id, cli_path, accept_eula, |_| {}).await
}

pub async fn run_speedtest_with_progress<F>(
    nic_id: Option<String>,
    server_id: Option<String>,
    cli_path: Option<String>,
    accept_eula: bool,
    on_progress: F,
) -> Result<SpeedtestResult>
where
    F: FnMut(SpeedtestProgress) + Send + 'static,
{
    let path = resolve_cli_path(cli_path.as_deref());
    tokio::task::spawn_blocking(move || {
        run_speedtest_streaming(&path, nic_id, server_id, accept_eula, on_progress)
    })
    .await?
}

fn run_speedtest_streaming<F>(
    cli: &Path,
    nic_id: Option<String>,
    server_id: Option<String>,
    accept_eula: bool,
    mut on_progress: F,
) -> Result<SpeedtestResult>
where
    F: FnMut(SpeedtestProgress),
{
    if which_exists(cli).is_none() && !cli.exists() {
        bail!(
            "Ookla Speedtest CLI not found. Install from https://www.speedtest.net/apps/cli \
             and place speedtest.exe in PATH, repo speedtest/, or Netvan data dir ({}), or set path in Settings.",
            data_dir().display()
        );
    }

    let mut args = vec![
        "--format=jsonl".to_string(),
        "--progress=yes".to_string(),
        "--progress-update-interval=200".to_string(),
    ];
    if accept_eula {
        args.push("--accept-license".into());
        args.push("--accept-gdpr".into());
    }
    if let Some(id) = server_id.clone() {
        args.push("--server-id".into());
        args.push(id);
    }

    SPEEDTEST_CANCELLED.store(false, Ordering::SeqCst);

    let mut cmd = Command::new(cli);
    win_cmd::hide_console(&mut cmd);
    let mut child = cmd
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("spawn {}", cli.display()))?;

    let stderr_pipe = child.stderr.take();
    let stdout = child
        .stdout
        .take()
        .context("speedtest stdout missing")?;
    let reader = BufReader::new(stdout);

    *SPEEDTEST_CHILD.lock() = Some(child);
    // Ensures the global handle is cleared even if the read loop returns early.
    struct ClearChildOnDrop;
    impl Drop for ClearChildOnDrop {
        fn drop(&mut self) {
            if let Some(mut child) = SPEEDTEST_CHILD.lock().take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
    let _clear_guard = ClearChildOnDrop;

    let mut download_mbps = 0.0;
    let mut upload_mbps = 0.0;
    let mut ping_ms = 0.0;
    let mut jitter_ms: Option<f64> = None;
    let mut packet_loss: Option<f64> = None;
    let mut server_id_out: Option<String> = server_id;
    let mut server_name: Option<String> = None;
    let mut result_json: Option<String> = None;
    let mut last_line = String::new();

    for line in reader.lines() {
        if SPEEDTEST_CANCELLED.load(Ordering::SeqCst) {
            bail!("cancelled");
        }
        let line = line.context("read speedtest stdout")?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        last_line = trimmed.to_string();
        let Ok(json) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };
        let typ = json.get("type").and_then(|v| v.as_str()).unwrap_or("");

        match typ {
            "testStart" => {
                server_name = json
                    .pointer("/server/name")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                if server_id_out.is_none() {
                    server_id_out = json
                        .pointer("/server/id")
                        .map(|v| v.to_string().trim_matches('"').to_string());
                }
                on_progress(SpeedtestProgress {
                    phase: "start".into(),
                    download_mbps: None,
                    upload_mbps: None,
                    ping_ms: None,
                    server_name: server_name.clone(),
                });
            }
            "ping" => {
                if let Some(lat) = json.pointer("/ping/latency").and_then(|v| v.as_f64()) {
                    ping_ms = lat;
                }
                if let Some(j) = json.pointer("/ping/jitter").and_then(|v| v.as_f64()) {
                    jitter_ms = Some(j);
                }
                on_progress(SpeedtestProgress {
                    phase: "ping".into(),
                    download_mbps: None,
                    upload_mbps: None,
                    ping_ms: Some(ping_ms),
                    server_name: server_name.clone(),
                });
            }
            "download" => {
                if let Some(bps) = json
                    .pointer("/download/bandwidth")
                    .and_then(|v| v.as_f64())
                {
                    download_mbps = bps * 8.0 / 1_000_000.0;
                }
                on_progress(SpeedtestProgress {
                    phase: "download".into(),
                    download_mbps: Some(download_mbps),
                    upload_mbps: None,
                    ping_ms: Some(ping_ms),
                    server_name: server_name.clone(),
                });
            }
            "upload" => {
                if let Some(bps) = json.pointer("/upload/bandwidth").and_then(|v| v.as_f64()) {
                    upload_mbps = bps * 8.0 / 1_000_000.0;
                }
                on_progress(SpeedtestProgress {
                    phase: "upload".into(),
                    download_mbps: Some(download_mbps),
                    upload_mbps: Some(upload_mbps),
                    ping_ms: Some(ping_ms),
                    server_name: server_name.clone(),
                });
            }
            "result" => {
                result_json = Some(trimmed.to_string());
                if let Some(bps) = json
                    .pointer("/download/bandwidth")
                    .and_then(|v| v.as_f64())
                {
                    download_mbps = bps * 8.0 / 1_000_000.0;
                }
                if let Some(bps) = json.pointer("/upload/bandwidth").and_then(|v| v.as_f64()) {
                    upload_mbps = bps * 8.0 / 1_000_000.0;
                }
                if let Some(lat) = json.pointer("/ping/latency").and_then(|v| v.as_f64()) {
                    ping_ms = lat;
                }
                jitter_ms = json.pointer("/ping/jitter").and_then(|v| v.as_f64());
                packet_loss = json.pointer("/packetLoss").and_then(|v| v.as_f64());
                server_name = json
                    .pointer("/server/name")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .or(server_name);
                server_id_out = json
                    .pointer("/server/id")
                    .map(|v| v.to_string().trim_matches('"').to_string())
                    .or(server_id_out);
                on_progress(SpeedtestProgress {
                    phase: "result".into(),
                    download_mbps: Some(download_mbps),
                    upload_mbps: Some(upload_mbps),
                    ping_ms: Some(ping_ms),
                    server_name: server_name.clone(),
                });
            }
            _ => {}
        }
    }

    let status = {
        let mut guard = SPEEDTEST_CHILD.lock();
        match guard.take() {
            Some(mut child) => child.wait().context("wait speedtest")?,
            None => {
                if SPEEDTEST_CANCELLED.load(Ordering::SeqCst) {
                    bail!("cancelled");
                }
                bail!("speedtest child missing");
            }
        }
    };
    let stderr = {
        let mut buf = String::new();
        if let Some(mut err) = stderr_pipe {
            use std::io::Read;
            let _ = err.read_to_string(&mut buf);
        }
        buf
    };

    if SPEEDTEST_CANCELLED.load(Ordering::SeqCst) {
        bail!("cancelled");
    }

    if !status.success() && result_json.is_none() && download_mbps <= 0.0 && upload_mbps <= 0.0 {
        bail!(
            "speedtest failed: {}",
            if stderr.trim().is_empty() {
                last_line
            } else {
                stderr.trim().to_string()
            }
        );
    }

    Ok(SpeedtestResult {
        id: 0,
        nic_id,
        ts: Utc::now().timestamp(),
        server_id: server_id_out,
        server_name,
        download_mbps,
        upload_mbps,
        ping_ms,
        jitter_ms,
        packet_loss,
        raw_json: result_json.or(Some(last_line)),
    })
}

fn which_exists(cli: &Path) -> Option<PathBuf> {
    if cli.exists() {
        return Some(cli.to_path_buf());
    }
    let name = cli.file_name()?.to_string_lossy().to_string();
    let mut cmd = Command::new("where");
    win_cmd::hide_console(&mut cmd);
    let out = cmd.arg(&name).output().ok()?;
    if out.status.success() {
        let line = String::from_utf8_lossy(&out.stdout)
            .lines()
            .next()?
            .trim()
            .to_string();
        if !line.is_empty() {
            return Some(PathBuf::from(line));
        }
    }
    None
}
