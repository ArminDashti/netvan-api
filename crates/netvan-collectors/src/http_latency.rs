use anyhow::Result;
use chrono::Utc;
use netvan_core::types::HttpLatencySample;
use std::net::ToSocketAddrs;
use std::time::Instant;
use tokio::net::TcpStream;

/// Browser-like HTTP(S) timing: DNS, TCP connect, TLS (approx), TTFB, total.
pub async fn measure_http(
    url: &str,
    nic_id: Option<String>,
    _bind_ip: Option<String>,
) -> Result<HttpLatencySample> {
    let ts = Utc::now().timestamp();
    let parsed = match url::Url::parse(url) {
        Ok(u) => u,
        Err(e) => {
            return Ok(HttpLatencySample {
                nic_id,
                url: url.to_string(),
                ts,
                dns_ms: None,
                connect_ms: None,
                tls_ms: None,
                ttfb_ms: None,
                total_ms: None,
                status_code: None,
                success: false,
                error: Some(e.to_string()),
            });
        }
    };

    let host = parsed.host_str().unwrap_or("").to_string();
    let port = parsed.port_or_known_default().unwrap_or(443);
    let is_https = parsed.scheme() == "https";
    let total_start = Instant::now();

    let dns_start = Instant::now();
    let addrs = match tokio::task::spawn_blocking({
        let host = host.clone();
        move || {
            (host.as_str(), port)
                .to_socket_addrs()
                .map(|a| a.collect::<Vec<_>>())
        }
    })
    .await
    {
        Ok(Ok(a)) if !a.is_empty() => a,
        Ok(Ok(_)) => return Ok(fail(nic_id, url, ts, "DNS returned no addresses")),
        Ok(Err(e)) => return Ok(fail(nic_id, url, ts, &e.to_string())),
        Err(e) => return Ok(fail(nic_id, url, ts, &e.to_string())),
    };
    let dns_ms = dns_start.elapsed().as_secs_f64() * 1000.0;

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
    {
        Ok(c) => c,
        Err(e) => return Ok(fail(nic_id, url, ts, &e.to_string())),
    };

    let connect_start = Instant::now();
    let connect_ms = match TcpStream::connect(addrs[0]).await {
        Ok(_s) => Some(connect_start.elapsed().as_secs_f64() * 1000.0),
        Err(_) => None,
    };

    let req_start = Instant::now();
    match client.get(url).send().await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let ttfb_ms = req_start.elapsed().as_secs_f64() * 1000.0;
            let _ = resp.bytes().await;
            let total_ms = total_start.elapsed().as_secs_f64() * 1000.0;
            let tls_ms = if is_https {
                connect_ms.map(|c| (ttfb_ms - c).max(0.0) * 0.3)
            } else {
                None
            };
            Ok(HttpLatencySample {
                nic_id,
                url: url.to_string(),
                ts,
                dns_ms: Some(dns_ms),
                connect_ms,
                tls_ms,
                ttfb_ms: Some(ttfb_ms),
                total_ms: Some(total_ms),
                status_code: Some(status),
                success: status < 500,
                error: None,
            })
        }
        Err(e) => Ok(HttpLatencySample {
            nic_id,
            url: url.to_string(),
            ts,
            dns_ms: Some(dns_ms),
            connect_ms,
            tls_ms: None,
            ttfb_ms: None,
            total_ms: Some(total_start.elapsed().as_secs_f64() * 1000.0),
            status_code: None,
            success: false,
            error: Some(e.to_string()),
        }),
    }
}

fn fail(nic_id: Option<String>, url: &str, ts: i64, err: &str) -> HttpLatencySample {
    HttpLatencySample {
        nic_id,
        url: url.to_string(),
        ts,
        dns_ms: None,
        connect_ms: None,
        tls_ms: None,
        ttfb_ms: None,
        total_ms: None,
        status_code: None,
        success: false,
        error: Some(err.to_string()),
    }
}
