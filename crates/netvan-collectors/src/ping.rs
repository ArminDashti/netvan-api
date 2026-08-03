use anyhow::{Context, Result};
use chrono::Utc;
use netvan_core::types::PingSample;
use serde::Serialize;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::Instant;

use crate::win_cmd;

#[derive(Debug, Clone, Serialize)]
pub struct PingLineEvent {
    pub phase: String,
    pub line: Option<String>,
}

/// ICMP ping via Windows `ping.exe`, optionally binding source address for a NIC.
pub async fn ping_once(
    target: &str,
    nic_id: Option<String>,
    source_ip: Option<String>,
) -> Result<PingSample> {
    ping(target, nic_id, source_ip, 1, None).await
}

pub async fn ping(
    target: &str,
    nic_id: Option<String>,
    source_ip: Option<String>,
    count: u32,
    packet_size: Option<u32>,
) -> Result<PingSample> {
    ping_with_progress(target, nic_id, source_ip, count, packet_size, |_| {}).await
}

pub async fn ping_with_progress<F>(
    target: &str,
    nic_id: Option<String>,
    source_ip: Option<String>,
    count: u32,
    packet_size: Option<u32>,
    on_line: F,
) -> Result<PingSample>
where
    F: FnMut(PingLineEvent) + Send + 'static,
{
    let target = target.to_string();
    let nic_id_c = nic_id.clone();
    let source = source_ip.clone();
    tokio::task::spawn_blocking(move || {
        ping_streaming(
            &target,
            nic_id_c,
            source,
            count.max(1),
            packet_size,
            on_line,
        )
    })
    .await?
}

fn ping_streaming<F>(
    target: &str,
    nic_id: Option<String>,
    source_ip: Option<String>,
    count: u32,
    packet_size: Option<u32>,
    mut on_line: F,
) -> Result<PingSample>
where
    F: FnMut(PingLineEvent),
{
    let ts = Utc::now().timestamp();
    let mut cmd = Command::new("ping");
    win_cmd::hide_console(&mut cmd);
    cmd.args(["-n", &count.to_string(), "-w", "3000"]);
    if let Some(size) = packet_size {
        cmd.args(["-l", &size.to_string()]);
    }
    if let Some(src) = source_ip {
        cmd.args(["-S", &src]);
    }
    cmd.arg(target);

    let start = Instant::now();
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("spawn ping")?;

    let stderr_pipe = child.stderr.take();
    let stdout = child.stdout.take().context("ping stdout missing")?;
    let reader = BufReader::new(stdout);
    let mut lines_acc = Vec::new();

    for line in reader.lines() {
        let line = line.context("read ping stdout")?;
        on_line(PingLineEvent {
            phase: "line".into(),
            line: Some(line.clone()),
        });
        lines_acc.push(line);
    }

    let status = child.wait().context("wait ping")?;
    let stderr = {
        let mut buf = String::new();
        if let Some(mut err) = stderr_pipe {
            use std::io::Read;
            let _ = err.read_to_string(&mut buf);
        }
        buf
    };

    let stdout_text = lines_acc.join("\n");
    let stdout_empty = stdout_text.trim().is_empty();
    let text = if stdout_empty {
        stderr.clone()
    } else {
        stdout_text
    };

    if !stderr.trim().is_empty() && stdout_empty {
        for line in stderr.lines() {
            on_line(PingLineEvent {
                phase: "line".into(),
                line: Some(line.to_string()),
            });
        }
    }

    on_line(PingLineEvent {
        phase: "done".into(),
        line: None,
    });

    let rtt = parse_rtt_ms(&text).or_else(|| {
        if status.success() {
            Some(start.elapsed().as_secs_f64() * 1000.0)
        } else {
            None
        }
    });
    let success = rtt.is_some() && status.success();
    Ok(PingSample {
        nic_id,
        target: target.to_string(),
        ts,
        rtt_ms: rtt,
        success,
        error: if success {
            None
        } else if !stderr.trim().is_empty() {
            Some(stderr.trim().to_string())
        } else {
            Some(text.trim().to_string())
        },
        raw_output: Some(text),
    })
}

fn parse_rtt_ms(text: &str) -> Option<f64> {
    // Prefer Average = 12ms for multi-packet; else first time=12ms
    for line in text.lines() {
        let lower = line.to_lowercase();
        if let Some(idx) = lower.find("average") {
            let rest = &lower[idx..];
            if let Some(eq) = rest.find('=') {
                let num: String = rest[eq + 1..]
                    .chars()
                    .skip_while(|c| !c.is_ascii_digit() && *c != '.')
                    .take_while(|c| c.is_ascii_digit() || *c == '.')
                    .collect();
                if let Ok(v) = num.parse::<f64>() {
                    return Some(v);
                }
            }
        }
    }
    for line in text.lines() {
        let lower = line.to_lowercase();
        if let Some(idx) = lower.find("time=") {
            let rest = &lower[idx + 5..];
            let num: String = rest
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            if let Ok(v) = num.parse::<f64>() {
                return Some(v);
            }
            if rest.starts_with('<') {
                return Some(1.0);
            }
        }
        if lower.contains("time<") {
            return Some(1.0);
        }
    }
    None
}
