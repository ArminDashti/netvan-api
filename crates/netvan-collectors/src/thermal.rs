//! Live temperature sensors via the LibreHardwareMonitor helper (`netvan-hwmon`).

use netvan_core::types::ThermalSnapshot;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::time::{Duration, Instant};
use tracing::warn;

const CACHE_TTL: Duration = Duration::from_millis(1500);

struct Helper {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
}

struct State {
    helper: Option<Helper>,
    snapshot: ThermalSnapshot,
    at: Option<Instant>,
    spawn_failed: bool,
}

static STATE: Lazy<Mutex<State>> = Lazy::new(|| {
    Mutex::new(State {
        helper: None,
        snapshot: ThermalSnapshot {
            sensors: Vec::new(),
        },
        at: None,
        spawn_failed: false,
    })
});

pub fn snapshot() -> ThermalSnapshot {
    #[cfg(not(windows))]
    {
        ThermalSnapshot {
            sensors: Vec::new(),
        }
    }
    #[cfg(windows)]
    {
        snapshot_windows()
    }
}

#[cfg(windows)]
fn snapshot_windows() -> ThermalSnapshot {
    let mut st = STATE.lock();
    if let Some(at) = st.at {
        if at.elapsed() < CACHE_TTL {
            return st.snapshot.clone();
        }
    }

    match poll_helper(&mut st) {
        Ok(snap) => {
            st.snapshot = snap.clone();
            st.at = Some(Instant::now());
            snap
        }
        Err(e) => {
            warn!("thermal helper: {e:#}");
            drop_helper(&mut st);
            st.snapshot = ThermalSnapshot {
                sensors: Vec::new(),
            };
            st.at = Some(Instant::now());
            st.snapshot.clone()
        }
    }
}

#[cfg(windows)]
fn poll_helper(st: &mut State) -> anyhow::Result<ThermalSnapshot> {
    if st.helper.is_none() {
        if st.spawn_failed {
            return Ok(ThermalSnapshot {
                sensors: Vec::new(),
            });
        }
        match spawn_helper() {
            Ok(h) => st.helper = Some(h),
            Err(e) => {
                st.spawn_failed = true;
                warn!("netvan-hwmon spawn failed: {e:#}");
                return Ok(ThermalSnapshot {
                    sensors: Vec::new(),
                });
            }
        }
    }

    let helper = st.helper.as_mut().expect("helper set");
    helper.stdin.write_all(b"snapshot\n")?;
    helper.stdin.flush()?;

    let mut line = String::new();
    helper.stdout.read_line(&mut line)?;
    if line.is_empty() {
        anyhow::bail!("helper closed stdout");
    }

    let snap: ThermalSnapshot = serde_json::from_str(line.trim())?;
    Ok(snap)
}

#[cfg(windows)]
fn drop_helper(st: &mut State) {
    if let Some(mut h) = st.helper.take() {
        let _ = h.stdin.write_all(b"quit\n");
        let _ = h.child.kill();
        let _ = h.child.wait();
    }
    st.spawn_failed = false;
}

#[cfg(windows)]
fn spawn_helper() -> anyhow::Result<Helper> {
    let path = helper_path().ok_or_else(|| anyhow::anyhow!("netvan-hwmon.exe not found next to netvan-api"))?;
    let mut cmd = Command::new(&path);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    crate::win_cmd::hide_console(&mut cmd);
    let mut child = cmd.spawn()?;
    let stdin = child.stdin.take().ok_or_else(|| anyhow::anyhow!("no stdin"))?;
    let stdout = child.stdout.take().ok_or_else(|| anyhow::anyhow!("no stdout"))?;
    Ok(Helper {
        child,
        stdin,
        stdout: BufReader::new(stdout),
    })
}

#[cfg(windows)]
fn helper_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("NETVAN_HWMON_PATH") {
        let pb = PathBuf::from(p);
        if pb.is_file() {
            return Some(pb);
        }
    }
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    [
        dir.join("netvan-hwmon.exe"),
        dir.join("netvan-hwmon").join("netvan-hwmon.exe"),
    ]
    .into_iter()
    .find(|p| p.is_file())
}
