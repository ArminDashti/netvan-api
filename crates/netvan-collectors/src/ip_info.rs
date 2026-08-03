//! IP geo / ASN lookup via asip-api (server-side; avoids browser Origin → 403).

use anyhow::{bail, Context, Result};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};

const API_BASE: &str = "https://asip-api.xaigrok.ir/api/v1/ip/info";
const CACHE_TTL: Duration = Duration::from_secs(3600);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpInfo {
    pub ip: String,
    pub asn: Option<i64>,
    #[serde(rename = "as")]
    pub as_name: Option<String>,
    pub country: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiResponse {
    ip: Option<String>,
    asn: Option<i64>,
    #[serde(rename = "as")]
    as_name: Option<String>,
    country: Option<String>,
}

struct CacheEntry {
    info: Option<IpInfo>,
    at: Instant,
}

static CACHE: Lazy<Mutex<HashMap<String, CacheEntry>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

fn is_public_ipv4(ip: &str) -> bool {
    let parts: Vec<u8> = match ip
        .trim()
        .split('.')
        .map(|p| p.parse::<u8>())
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(p) if p.len() == 4 => p,
        _ => return false,
    };
    let (a, b) = (parts[0], parts[1]);
    if a == 10 || a == 127 || a == 0 {
        return false;
    }
    if a == 169 && b == 254 {
        return false;
    }
    if a == 172 && (16..=31).contains(&b) {
        return false;
    }
    if a == 192 && b == 168 {
        return false;
    }
    if a == 100 && (64..=127).contains(&b) {
        return false;
    }
    if a >= 224 {
        return false;
    }
    true
}

pub async fn lookup_ip_info(ip: &str) -> Result<Option<IpInfo>> {
    let key = ip.trim().to_string();
    if key.is_empty() || !is_public_ipv4(&key) {
        return Ok(None);
    }

    {
        let cache = CACHE.lock();
        if let Some(entry) = cache.get(&key) {
            if entry.at.elapsed() < CACHE_TTL {
                return Ok(entry.info.clone());
            }
        }
    }

    let url = format!("{API_BASE}/{}", urlencoding_encode(&key));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .user_agent("Netvan/0.1")
        .build()
        .context("build http client")?;

    let res = client.get(&url).send().await.context("asip-api request")?;
    if !res.status().is_success() {
        bail!("asip-api HTTP {}", res.status());
    }
    let raw: ApiResponse = res.json().await.context("asip-api json")?;
    let info = IpInfo {
        ip: raw.ip.unwrap_or(key.clone()),
        asn: raw.asn,
        as_name: raw.as_name,
        country: raw.country,
    };

    CACHE.lock().insert(
        key,
        CacheEntry {
            info: Some(info.clone()),
            at: Instant::now(),
        },
    );
    Ok(Some(info))
}

fn urlencoding_encode(s: &str) -> String {
    // IPv4 needs no encoding; keep safe for odd inputs.
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
