use anyhow::Result;
use netvan_core::types::NslookupResult;
use std::net::ToSocketAddrs;
use std::process::Command;

use crate::win_cmd;

pub async fn nslookup(query: &str) -> Result<NslookupResult> {
    let query = query.to_string();
    tokio::task::spawn_blocking(move || nslookup_blocking(&query)).await?
}

fn nslookup_blocking(query: &str) -> Result<NslookupResult> {
    // Prefer system nslookup for familiar output; also collect A/AAAA via resolver.
    let mut records = Vec::new();
    if let Ok(addrs) = (query, 0u16).to_socket_addrs() {
        for a in addrs {
            records.push(a.ip().to_string());
        }
    }

    let mut cmd = Command::new("nslookup");
    win_cmd::hide_console(&mut cmd);
    let output = cmd.arg(query).output();
    let raw = match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            format!("{stdout}{stderr}")
        }
        Err(e) => format!("nslookup spawn failed: {e}\nResolved: {records:?}"),
    };

    if records.is_empty() {
        for line in raw.lines() {
            let t = line.trim();
            if t.starts_with("Address:") {
                let addr = t.trim_start_matches("Address:").trim();
                if !addr.is_empty() && !records.contains(&addr.to_string()) {
                    records.push(addr.to_string());
                }
            }
        }
    }

    Ok(NslookupResult {
        query: query.to_string(),
        records,
        raw,
    })
}
