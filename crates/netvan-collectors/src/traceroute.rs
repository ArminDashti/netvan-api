use anyhow::{Context, Result};
use chrono::Utc;
use netvan_core::types::{HttpLatencySample, TracerouteHop, TracerouteResult};
use serde::Serialize;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

use crate::win_cmd;

#[derive(Debug, Clone, Serialize)]
pub struct TracerouteHopEvent {
    pub phase: String,
    pub hop: Option<u8>,
    pub address: Option<String>,
    pub rtt_ms: Option<f64>,
}

pub async fn traceroute(
    target: &str,
    nic_id: Option<String>,
    max_hops: u8,
) -> Result<TracerouteResult> {
    traceroute_with_progress(target, nic_id, max_hops, |_| {}).await
}

pub async fn traceroute_with_progress<F>(
    target: &str,
    nic_id: Option<String>,
    max_hops: u8,
    on_hop: F,
) -> Result<TracerouteResult>
where
    F: FnMut(TracerouteHopEvent) + Send + 'static,
{
    let target = target.to_string();
    let nic_id_c = nic_id.clone();
    tokio::task::spawn_blocking(move || {
        traceroute_streaming(&target, nic_id_c, max_hops, on_hop)
    })
    .await?
}

fn traceroute_streaming<F>(
    target: &str,
    nic_id: Option<String>,
    max_hops: u8,
    mut on_hop: F,
) -> Result<TracerouteResult>
where
    F: FnMut(TracerouteHopEvent),
{
    let ts = Utc::now().timestamp();
    let mut cmd = Command::new("tracert");
    win_cmd::hide_console(&mut cmd);
    let mut child = cmd
        .args(["-d", "-h", &max_hops.to_string(), "-w", "2000", target])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("spawn tracert")?;

    let stdout = child.stdout.take().context("tracert stdout missing")?;
    let reader = BufReader::new(stdout);
    let mut hops = Vec::new();

    for line in reader.lines() {
        let line = line.context("read tracert stdout")?;
        if let Some(hop) = parse_hop_line(&line) {
            on_hop(TracerouteHopEvent {
                phase: "hop".into(),
                hop: Some(hop.hop),
                address: hop.address.clone(),
                rtt_ms: hop.rtt_ms,
            });
            hops.push(hop);
        }
    }

    let _ = child.wait();
    on_hop(TracerouteHopEvent {
        phase: "done".into(),
        hop: None,
        address: None,
        rtt_ms: None,
    });

    Ok(TracerouteResult {
        id: 0,
        nic_id,
        target: target.to_string(),
        ts,
        hops,
    })
}

fn parse_hop_line(line: &str) -> Option<TracerouteHop> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    let parts: Vec<&str> = trimmed.split_whitespace().collect();
    if parts.is_empty() {
        return None;
    }
    let hop = parts[0].parse::<u8>().ok()?;
    let mut rtt = None;
    let mut address = None;
    for p in &parts[1..] {
        if *p == "*" || *p == "<1" || p.ends_with("ms") {
            if p.ends_with("ms") {
                let num = p.trim_end_matches("ms").replace('<', "");
                if let Ok(v) = num.parse::<f64>() {
                    rtt = Some(v);
                }
            }
        } else if p.contains('.') || p.contains(':') {
            address = Some((*p).to_string());
        }
    }
    Some(TracerouteHop {
        hop,
        address,
        hostname: None,
        rtt_ms: rtt,
    })
}

/// Re-export alias used by tools page.
pub type OneShotHttp = HttpLatencySample;
