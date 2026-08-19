use anyhow::Result;
use chrono::Utc;
use netvan_core::db::Database;
use netvan_core::history::{HistoryRange, TimeRange};
use netvan_core::ipc::{RpcRequest, RpcResponse};
use netvan_core::settings::AppSettings;
use netvan_core::types::*;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{error, info};

use crate::http_latency;
use crate::nic::NicCollector;
use crate::nslookup;
use crate::ping;
use crate::speedtest;
use crate::system_metrics::SystemMetricsCollector;
use crate::traceroute;
use crate::traffic_process::ProcessTrafficCollector;

pub struct CollectorEngine {
    db: Database,
    nic: NicCollector,
    system: SystemMetricsCollector,
    process_traffic: Mutex<ProcessTrafficCollector>,
    last_oper: RwLock<HashMap<String, String>>,
    settings: RwLock<AppSettings>,
    running: RwLock<bool>,
}

impl CollectorEngine {
    pub fn new(db: Database) -> Result<Arc<Self>> {
        let mut settings = db.get_settings()?;
        settings.clamp_strip_prefs();
        Ok(Arc::new(Self {
            db,
            nic: NicCollector::new(),
            system: SystemMetricsCollector::new(),
            process_traffic: Mutex::new(ProcessTrafficCollector::new()),
            last_oper: RwLock::new(HashMap::new()),
            settings: RwLock::new(settings),
            running: RwLock::new(true),
        }))
    }

    pub fn db(&self) -> &Database {
        &self.db
    }

    pub fn settings_snapshot(&self) -> AppSettings {
        self.settings.read().clone()
    }

    pub async fn start_background(self: Arc<Self>) {
        let eng = self.clone();
        tokio::spawn(async move {
            loop {
                if !*eng.running.read() {
                    break;
                }
                if let Err(e) = eng.tick_bandwidth().await {
                    error!("bandwidth tick: {e:#}");
                }
                let ms = eng.settings.read().bandwidth_interval_ms;
                tokio::time::sleep(Duration::from_millis(ms.max(500))).await;
            }
        });

        let eng = self.clone();
        tokio::spawn(async move {
            loop {
                if !*eng.running.read() {
                    break;
                }
                if let Err(e) = eng.tick_ping().await {
                    error!("ping tick: {e:#}");
                }
                let secs = eng.settings.read().ping_interval_secs;
                tokio::time::sleep(Duration::from_secs(secs.max(2))).await;
            }
        });

        let eng = self.clone();
        tokio::spawn(async move {
            loop {
                if !*eng.running.read() {
                    break;
                }
                if let Err(e) = eng.tick_http().await {
                    error!("http tick: {e:#}");
                }
                let secs = eng.settings.read().http_interval_secs;
                tokio::time::sleep(Duration::from_secs(secs.max(5))).await;
            }
        });

        let eng = self.clone();
        tokio::spawn(async move {
            loop {
                if !*eng.running.read() {
                    break;
                }
                if let Err(e) = eng.tick_traffic().await {
                    error!("traffic tick: {e:#}");
                }
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        });

        let eng = self.clone();
        tokio::spawn(async move {
            // Warm-up second CPU refresh so utilization is meaningful.
            tokio::time::sleep(Duration::from_millis(500)).await;
            eng.system.refresh_live();
            loop {
                if !*eng.running.read() {
                    break;
                }
                eng.system.refresh_live();
                let ms = eng.settings.read().system_interval_ms.max(500);
                tokio::time::sleep(Duration::from_millis(ms)).await;
            }
        });

        let eng = self.clone();
        tokio::spawn(async move {
            loop {
                if !*eng.running.read() {
                    break;
                }
                if let Err(e) = eng.tick_system_persist() {
                    error!("system persist: {e:#}");
                }
                let ms = eng.settings.read().system_persist_interval_ms.max(1000);
                tokio::time::sleep(Duration::from_millis(ms)).await;
            }
        });

        let eng = self.clone();
        tokio::spawn(async move {
            loop {
                if !*eng.running.read() {
                    break;
                }
                let days = eng.settings.read().retention_raw_days;
                if let Err(e) = eng.db.prune_raw(days) {
                    error!("prune: {e:#}");
                }
                let s = eng.settings.read().clone();
                if let Err(e) = eng.db.prune_system_metrics(
                    s.system_raw_retention_days,
                    s.system_hourly_retention_days,
                ) {
                    error!("prune system: {e:#}");
                }
                tokio::time::sleep(Duration::from_secs(3600)).await;
            }
        });

        info!("collector background tasks started");
    }

    pub fn stop(&self) {
        *self.running.write() = false;
    }

    async fn tick_bandwidth(&self) -> Result<()> {
        let nics = self.nic.list()?;
        let mut last = self.last_oper.write();
        for n in &nics {
            self.db.upsert_nic(n)?;
            if let Some(prev) = last.get(&n.id) {
                if prev != &n.oper_status {
                    let event = if n.oper_status == "Up" {
                        "connected"
                    } else {
                        "disconnected"
                    };
                    self.db.insert_link_event(&LinkEvent {
                        nic_id: n.id.clone(),
                        ts: Utc::now().timestamp(),
                        event: event.into(),
                        detail: Some(format!("{prev} -> {}", n.oper_status)),
                    })?;
                }
            }
            last.insert(n.id.clone(), n.oper_status.clone());
        }
        drop(last);
        for s in NicCollector::sample_bandwidth(&nics) {
            self.db.insert_bandwidth(&s)?;
        }
        Ok(())
    }

    async fn tick_ping(&self) -> Result<()> {
        let settings = self.settings.read().clone();
        let nics = self.nic.list().unwrap_or_default();
        let up_nics: Vec<_> = nics
            .into_iter()
            .filter(|n| n.oper_status == "Up" && n.media_type != "Loopback")
            .collect();
        for target in &settings.ping_targets {
            if up_nics.is_empty() {
                let sample = ping::ping_once(target, None, None).await?;
                self.db.insert_ping(&sample)?;
            } else {
                for nic in &up_nics {
                    let src = nic.ipv4_addresses.first().cloned();
                    let sample = ping::ping_once(target, Some(nic.id.clone()), src).await?;
                    self.db.insert_ping(&sample)?;
                }
            }
        }
        Ok(())
    }

    async fn tick_http(&self) -> Result<()> {
        let settings = self.settings.read().clone();
        let default_nic = settings.default_nic_id.clone();
        for url in &settings.http_targets {
            let sample = http_latency::measure_http(url, default_nic.clone(), None).await?;
            self.db.insert_http_latency(&sample)?;
        }
        Ok(())
    }

    async fn tick_traffic(&self) -> Result<()> {
        let mut pt = self.process_traffic.lock().await;
        for f in pt.sample()? {
            self.db.insert_traffic_flow(&f)?;
        }
        Ok(())
    }

    fn tick_system_persist(&self) -> Result<()> {
        self.system.refresh_live();
        let cpu = self.system.cpu_snapshot();
        let mem = self.system.memory_snapshot();
        let disks = self.system.disk_snapshots(None);
        let ts = Utc::now().timestamp();
        self.db.persist_system_tick(
            ts,
            cpu.utilization,
            mem.utilization,
            mem.used_bytes,
            &disks,
        )?;
        Ok(())
    }

    pub async fn handle(&self, req: RpcRequest) -> RpcResponse {
        match self.handle_inner(req).await {
            Ok(r) => r,
            Err(e) => RpcResponse::Error {
                message: format!("{e:#}"),
            },
        }
    }

    async fn handle_inner(&self, req: RpcRequest) -> Result<RpcResponse> {
        Ok(match req {
            RpcRequest::Ping => RpcResponse::Pong,
            RpcRequest::GetStatus => {
                let mode = self.settings.read().capture_mode;
                RpcResponse::Status(ServiceStatus {
                    running: *self.running.read(),
                    pipe_connected: true,
                    capture_mode: mode.as_str().into(),
                    message: String::new(),
                })
            }
            RpcRequest::GetSettings => RpcResponse::Settings(self.settings.read().clone()),
            RpcRequest::SetSettings { settings } => {
                let mut settings = settings;
                settings.clamp_strip_prefs();
                self.db.save_settings(&settings)?;
                *self.settings.write() = settings;
                RpcResponse::Ok
            }
            RpcRequest::SetCaptureMode { mode } => {
                let mut s = self.settings.read().clone();
                s.capture_mode = mode;
                self.db.save_settings(&s)?;
                *self.settings.write() = s;
                RpcResponse::Ok
            }
            RpcRequest::ListNics => {
                let nics = self.nic.list()?;
                for n in &nics {
                    let _ = self.db.upsert_nic(n);
                }
                RpcResponse::Nics(nics)
            }
            RpcRequest::GetNic { nic_id } => match self.nic.find(&nic_id)? {
                Some(n) => RpcResponse::Nic(n),
                None => RpcResponse::Error {
                    message: format!("NIC not found: {nic_id}"),
                },
            },
            RpcRequest::SetNicEnabled { nic_id, enabled } => {
                self.nic.set_enabled(&nic_id, enabled)?;
                RpcResponse::Ok
            },
            RpcRequest::GetBandwidthHistory {
                nic_id,
                range,
                start_ts,
                end_ts,
            } => {
                let tr = TimeRange::from_history(range, start_ts, end_ts);
                RpcResponse::BandwidthHistory(
                    self.db.bandwidth_history(nic_id.as_deref(), &tr)?,
                )
            }
            RpcRequest::GetPingHistory {
                nic_id,
                range,
                start_ts,
                end_ts,
            } => {
                let tr = TimeRange::from_history(range, start_ts, end_ts);
                RpcResponse::PingHistory(self.db.ping_history(nic_id.as_deref(), &tr)?)
            }
            RpcRequest::GetHttpLatencyHistory {
                nic_id,
                range,
                start_ts,
                end_ts,
            } => {
                let tr = TimeRange::from_history(range, start_ts, end_ts);
                RpcResponse::HttpLatencyHistory(
                    self.db.http_latency_history(nic_id.as_deref(), &tr)?,
                )
            }
            RpcRequest::GetLinkEvents {
                nic_id,
                range,
                start_ts,
                end_ts,
            } => {
                let tr = TimeRange::from_history(range, start_ts, end_ts);
                RpcResponse::LinkEvents(self.db.link_events(nic_id.as_deref(), &tr)?)
            }
            RpcRequest::GetAppUsage {
                range,
                start_ts,
                end_ts,
                group_by,
                nic_id,
            } => {
                let tr = TimeRange::from_history(range, start_ts, end_ts);
                let settings = self.settings.read().clone();
                RpcResponse::AppUsage(self.db.app_usage(
                    &tr,
                    &group_by,
                    nic_id.as_deref(),
                    &settings.ignored_apps,
                    &settings.ignored_ips,
                    &settings.ignored_urls,
                    settings.ignore_private_ips,
                )?)
            }
            RpcRequest::GetAppUsageSeries {
                range,
                start_ts,
                end_ts,
                group_by,
                nic_id,
            } => {
                let tr = TimeRange::from_history(range, start_ts, end_ts);
                let settings = self.settings.read().clone();
                RpcResponse::AppUsageSeries(self.db.app_usage_series(
                    &tr,
                    &group_by,
                    nic_id.as_deref(),
                    &settings.ignored_apps,
                    &settings.ignored_ips,
                    &settings.ignored_urls,
                    settings.ignore_private_ips,
                )?)
            }
            RpcRequest::RunPing {
                target,
                nic_id,
                count,
                packet_size,
            } => {
                let src = if let Some(ref id) = nic_id {
                    self.nic
                        .find(id)?
                        .and_then(|n| n.ipv4_addresses.first().cloned())
                } else {
                    None
                };
                let sample = ping::ping(
                    &target,
                    nic_id,
                    src,
                    count.unwrap_or(1),
                    packet_size,
                )
                .await?;
                self.db.insert_ping(&sample)?;
                RpcResponse::PingResult(sample)
            }
            RpcRequest::RunHttpLatency { url, nic_id } => {
                let sample = http_latency::measure_http(&url, nic_id, None).await?;
                self.db.insert_http_latency(&sample)?;
                RpcResponse::HttpLatencyResult(sample)
            }
            RpcRequest::RunTraceroute {
                target,
                nic_id,
                max_hops,
            } => {
                let mut result =
                    traceroute::traceroute(&target, nic_id, max_hops.unwrap_or(30)).await?;
                let id = self.db.insert_traceroute(&result)?;
                result.id = id;
                RpcResponse::Traceroute(result)
            }
            RpcRequest::RunNslookup { query } => {
                RpcResponse::Nslookup(nslookup::nslookup(&query).await?)
            }
            RpcRequest::RunSpeedtest {
                nic_id,
                server_id,
                accept_eula,
            } => {
                let mut settings = self.settings.read().clone();
                if accept_eula {
                    settings.speedtest_eula_accepted = true;
                    self.db.save_settings(&settings)?;
                    *self.settings.write() = settings.clone();
                }
                if !settings.speedtest_eula_accepted && !accept_eula {
                    return Ok(RpcResponse::Error {
                        message: "Accept Ookla Speedtest EULA/GDPR first".into(),
                    });
                }
                let mut result = speedtest::run_speedtest(
                    nic_id,
                    server_id,
                    settings.speedtest_cli_path.clone(),
                    true,
                )
                .await?;
                let id = self.db.insert_speedtest(&result)?;
                result.id = id;
                RpcResponse::Speedtest(result)
            }
            RpcRequest::GetSpeedtestHistory {
                range,
                start_ts,
                end_ts,
            } => {
                let tr = TimeRange::from_history(range, start_ts, end_ts);
                RpcResponse::SpeedtestHistory(self.db.speedtest_history(&tr)?)
            }
            RpcRequest::AcceptSpeedtestEula => {
                let mut s = self.settings.read().clone();
                s.speedtest_eula_accepted = true;
                self.db.save_settings(&s)?;
                *self.settings.write() = s;
                RpcResponse::Ok
            }
            RpcRequest::GetCpuSnapshot => {
                self.system.refresh_live();
                RpcResponse::CpuSnapshot(self.system.cpu_snapshot())
            }
            RpcRequest::GetMemorySnapshot => {
                self.system.refresh_live();
                RpcResponse::MemorySnapshot(self.system.memory_snapshot())
            }
            RpcRequest::GetDisks { kind } => {
                self.system.refresh_live();
                RpcResponse::Disks(self.system.disk_snapshots(Some(kind)))
            }
            RpcRequest::GetHardwareInventory => {
                RpcResponse::HardwareInventory(crate::hardware_inventory::collect())
            }
            RpcRequest::GetThermalSnapshot => {
                RpcResponse::ThermalSnapshot(crate::thermal::snapshot())
            }
            RpcRequest::GetCpuHistory {
                range,
                start_ts,
                end_ts,
            } => {
                let tr = TimeRange::from_history(range, start_ts, end_ts);
                RpcResponse::CpuHistory(self.db.cpu_history(&tr, range)?)
            }
            RpcRequest::GetMemoryHistory {
                range,
                start_ts,
                end_ts,
            } => {
                let tr = TimeRange::from_history(range, start_ts, end_ts);
                RpcResponse::MemoryHistory(self.db.memory_history(&tr, range)?)
            }
            RpcRequest::GetDiskHistory {
                disk_id,
                kind,
                range,
                start_ts,
                end_ts,
            } => {
                let tr = TimeRange::from_history(range, start_ts, end_ts);
                RpcResponse::DiskHistory(self.db.disk_history(
                    disk_id.as_deref(),
                    Some(kind),
                    &tr,
                    range,
                )?)
            }
        })
    }

    #[allow(dead_code)]
    pub fn history_range_demo() -> HistoryRange {
        HistoryRange::Today
    }
}
