use anyhow::{Context, Result};
use chrono::Utc;
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::sync::Arc;

use crate::history::{HistoryRange, TimeRange};
use crate::settings::{AppSettings, AppTheme, CaptureMode, StripSlot};
use crate::types::*;

const SCHEMA: &str = r#"
PRAGMA journal_mode=WAL;
PRAGMA foreign_keys=ON;

CREATE TABLE IF NOT EXISTS nics (
  id TEXT PRIMARY KEY,
  guid TEXT NOT NULL,
  name TEXT NOT NULL,
  description TEXT,
  interface_index INTEGER,
  mac TEXT,
  mtu INTEGER,
  media_type TEXT,
  oper_status TEXT,
  admin_status TEXT,
  link_speed_bps INTEGER,
  ipv4_json TEXT,
  ipv6_json TEXT,
  gateways_json TEXT,
  dns_json TEXT,
  dhcp_enabled INTEGER,
  wifi_ssid TEXT,
  wifi_bssid TEXT,
  wifi_signal INTEGER,
  driver TEXT,
  updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS nic_bandwidth_samples (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  nic_id TEXT NOT NULL,
  ts INTEGER NOT NULL,
  rx_bytes INTEGER NOT NULL,
  tx_bytes INTEGER NOT NULL,
  rx_bps REAL NOT NULL,
  tx_bps REAL NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_bw_nic_ts ON nic_bandwidth_samples(nic_id, ts);

CREATE TABLE IF NOT EXISTS nic_usage_hourly (
  nic_id TEXT NOT NULL,
  hour_ts INTEGER NOT NULL,
  rx_bytes INTEGER NOT NULL,
  tx_bytes INTEGER NOT NULL,
  PRIMARY KEY (nic_id, hour_ts)
);

CREATE TABLE IF NOT EXISTS ping_samples (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  nic_id TEXT,
  target TEXT NOT NULL,
  ts INTEGER NOT NULL,
  rtt_ms REAL,
  success INTEGER NOT NULL,
  error TEXT
);
CREATE INDEX IF NOT EXISTS idx_ping_ts ON ping_samples(ts);
CREATE INDEX IF NOT EXISTS idx_ping_nic_ts ON ping_samples(nic_id, ts);

CREATE TABLE IF NOT EXISTS http_latency_samples (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  nic_id TEXT,
  url TEXT NOT NULL,
  ts INTEGER NOT NULL,
  dns_ms REAL,
  connect_ms REAL,
  tls_ms REAL,
  ttfb_ms REAL,
  total_ms REAL,
  status_code INTEGER,
  success INTEGER NOT NULL,
  error TEXT
);
CREATE INDEX IF NOT EXISTS idx_http_ts ON http_latency_samples(ts);

CREATE TABLE IF NOT EXISTS nic_link_events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  nic_id TEXT NOT NULL,
  ts INTEGER NOT NULL,
  event TEXT NOT NULL,
  detail TEXT
);
CREATE INDEX IF NOT EXISTS idx_link_nic_ts ON nic_link_events(nic_id, ts);

CREATE TABLE IF NOT EXISTS traceroute_runs (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  nic_id TEXT,
  target TEXT NOT NULL,
  ts INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS traceroute_hops (
  run_id INTEGER NOT NULL,
  hop INTEGER NOT NULL,
  address TEXT,
  hostname TEXT,
  rtt_ms REAL,
  FOREIGN KEY(run_id) REFERENCES traceroute_runs(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS speedtest_runs (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  nic_id TEXT,
  ts INTEGER NOT NULL,
  server_id TEXT,
  server_name TEXT,
  download_mbps REAL NOT NULL,
  upload_mbps REAL NOT NULL,
  ping_ms REAL NOT NULL,
  jitter_ms REAL,
  packet_loss REAL,
  raw_json TEXT
);

CREATE TABLE IF NOT EXISTS traffic_flows (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  ts INTEGER NOT NULL,
  process_name TEXT NOT NULL,
  process_path TEXT,
  pid INTEGER,
  local_addr TEXT NOT NULL,
  remote_addr TEXT NOT NULL,
  protocol TEXT NOT NULL,
  bytes_in INTEGER NOT NULL,
  bytes_out INTEGER NOT NULL,
  nic_id TEXT,
  host TEXT
);
CREATE INDEX IF NOT EXISTS idx_flows_ts ON traffic_flows(ts);

CREATE TABLE IF NOT EXISTS dns_queries (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  ts INTEGER NOT NULL,
  name TEXT NOT NULL,
  resolved_ips TEXT,
  process_name TEXT
);

CREATE TABLE IF NOT EXISTS http_hosts (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  ts INTEGER NOT NULL,
  host TEXT NOT NULL,
  remote_ip TEXT,
  process_name TEXT
);

CREATE TABLE IF NOT EXISTS app_usage_hourly (
  hour_ts INTEGER NOT NULL,
  process_name TEXT NOT NULL,
  remote_ip TEXT NOT NULL DEFAULT '',
  host TEXT NOT NULL DEFAULT '',
  nic_id TEXT NOT NULL DEFAULT '',
  bytes_in INTEGER NOT NULL,
  bytes_out INTEGER NOT NULL,
  PRIMARY KEY (hour_ts, process_name, remote_ip, host, nic_id)
);

CREATE TABLE IF NOT EXISTS settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS disks (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  mount_point TEXT,
  file_system TEXT,
  kind TEXT NOT NULL,
  total_bytes INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS cpu_samples (
  ts INTEGER NOT NULL PRIMARY KEY,
  utilization REAL NOT NULL
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS memory_samples (
  ts INTEGER NOT NULL PRIMARY KEY,
  utilization REAL NOT NULL,
  used_bytes INTEGER NOT NULL
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS disk_samples (
  disk_id TEXT NOT NULL,
  ts INTEGER NOT NULL,
  utilization REAL NOT NULL,
  used_bytes INTEGER NOT NULL,
  PRIMARY KEY (disk_id, ts),
  FOREIGN KEY (disk_id) REFERENCES disks(id) ON DELETE CASCADE
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS cpu_stats_hourly (
  hour_ts INTEGER NOT NULL PRIMARY KEY,
  avg REAL NOT NULL,
  min REAL NOT NULL,
  max REAL NOT NULL,
  sample_count INTEGER NOT NULL
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS memory_stats_hourly (
  hour_ts INTEGER NOT NULL PRIMARY KEY,
  avg REAL NOT NULL,
  min REAL NOT NULL,
  max REAL NOT NULL,
  sample_count INTEGER NOT NULL
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS disk_stats_hourly (
  disk_id TEXT NOT NULL,
  hour_ts INTEGER NOT NULL,
  avg REAL NOT NULL,
  min REAL NOT NULL,
  max REAL NOT NULL,
  sample_count INTEGER NOT NULL,
  PRIMARY KEY (disk_id, hour_ts),
  FOREIGN KEY (disk_id) REFERENCES disks(id) ON DELETE CASCADE
) WITHOUT ROWID;
"#;

#[derive(Clone)]
pub struct Database {
    conn: Arc<Mutex<Connection>>,
}

impl Database {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path).context("open sqlite")?;
        conn.execute_batch(SCHEMA)?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.ensure_default_settings()?;
        Ok(db)
    }

    pub fn open_default() -> Result<Self> {
        crate::paths::ensure_data_dir()?;
        Self::open(crate::paths::db_path())
    }

    fn ensure_default_settings(&self) -> Result<()> {
        let settings = self.get_settings()?;
        self.save_settings(&settings)
    }

    pub fn get_settings(&self) -> Result<AppSettings> {
        let conn = self.conn.lock();
        let mut defaults = AppSettings::default();
        if let Some(v) = get_setting(&conn, "capture_mode")? {
            defaults.capture_mode = CaptureMode::parse(&v);
        }
        if let Some(v) = get_setting(&conn, "ping_targets")? {
            if let Ok(list) = serde_json::from_str::<Vec<String>>(&v) {
                defaults.ping_targets = list;
            }
        }
        if let Some(v) = get_setting(&conn, "http_targets")? {
            if let Ok(list) = serde_json::from_str::<Vec<String>>(&v) {
                defaults.http_targets = list;
            }
        }
        if let Some(v) = get_setting(&conn, "ping_interval_secs")? {
            if let Ok(n) = v.parse() {
                defaults.ping_interval_secs = n;
            }
        }
        if let Some(v) = get_setting(&conn, "http_interval_secs")? {
            if let Ok(n) = v.parse() {
                defaults.http_interval_secs = n;
            }
        }
        if let Some(v) = get_setting(&conn, "bandwidth_interval_ms")? {
            if let Ok(n) = v.parse() {
                defaults.bandwidth_interval_ms = n;
            }
        }
        if let Some(v) = get_setting(&conn, "retention_raw_days")? {
            if let Ok(n) = v.parse() {
                defaults.retention_raw_days = n;
            }
        }
        if let Some(v) = get_setting(&conn, "start_ui_with_windows")? {
            defaults.start_ui_with_windows = v == "1" || v == "true";
        }
        if let Some(v) = get_setting(&conn, "speedtest_cli_path")? {
            defaults.speedtest_cli_path = Some(v);
        }
        if let Some(v) = get_setting(&conn, "speedtest_eula_accepted")? {
            defaults.speedtest_eula_accepted = v == "1" || v == "true";
        }
        if let Some(v) = get_setting(&conn, "default_nic_id")? {
            defaults.default_nic_id = if v.is_empty() { None } else { Some(v) };
        }
        if let Some(v) = get_setting(&conn, "strip_width_px")? {
            if let Ok(n) = v.parse() {
                defaults.strip_width_px = n;
            }
        }
        if let Some(v) = get_setting(&conn, "strip_font_px")? {
            if let Ok(n) = v.parse() {
                defaults.strip_font_px = n;
            }
        }
        if let Some(v) = get_setting(&conn, "strip_slot")? {
            defaults.strip_slot = StripSlot::parse(&v);
        }
        if let Some(v) = get_setting(&conn, "strip_offset_px")? {
            if let Ok(n) = v.parse() {
                defaults.strip_offset_px = n;
            }
        }
        if let Some(v) = get_setting(&conn, "ignore_private_ips")? {
            defaults.ignore_private_ips = v == "1" || v == "true";
        }
        if let Some(v) = get_setting(&conn, "ignored_apps")? {
            if let Ok(list) = serde_json::from_str::<Vec<String>>(&v) {
                defaults.ignored_apps = list;
            }
        }
        if let Some(v) = get_setting(&conn, "ignored_ips")? {
            if let Ok(list) = serde_json::from_str::<Vec<String>>(&v) {
                defaults.ignored_ips = list;
            }
        }
        if let Some(v) = get_setting(&conn, "ignored_urls")? {
            if let Ok(list) = serde_json::from_str::<Vec<String>>(&v) {
                defaults.ignored_urls = list;
            }
        }
        if let Some(v) = get_setting(&conn, "theme")? {
            defaults.theme = AppTheme::parse(&v);
        }
        if let Some(v) = get_setting(&conn, "system_interval_ms")? {
            if let Ok(n) = v.parse() {
                defaults.system_interval_ms = n;
            }
        }
        if let Some(v) = get_setting(&conn, "system_persist_interval_ms")? {
            if let Ok(n) = v.parse() {
                defaults.system_persist_interval_ms = n;
            }
        }
        if let Some(v) = get_setting(&conn, "system_raw_retention_days")? {
            if let Ok(n) = v.parse() {
                defaults.system_raw_retention_days = n;
            }
        }
        if let Some(v) = get_setting(&conn, "system_hourly_retention_days")? {
            if let Ok(n) = v.parse() {
                defaults.system_hourly_retention_days = n;
            }
        }
        defaults.clamp_strip_prefs();
        Ok(defaults)
    }

    pub fn save_settings(&self, s: &AppSettings) -> Result<()> {
        let mut s = s.clone();
        s.clamp_strip_prefs();
        let conn = self.conn.lock();
        set_setting(&conn, "capture_mode", s.capture_mode.as_str())?;
        set_setting(
            &conn,
            "ping_targets",
            &serde_json::to_string(&s.ping_targets)?,
        )?;
        set_setting(
            &conn,
            "http_targets",
            &serde_json::to_string(&s.http_targets)?,
        )?;
        set_setting(
            &conn,
            "ping_interval_secs",
            &s.ping_interval_secs.to_string(),
        )?;
        set_setting(
            &conn,
            "http_interval_secs",
            &s.http_interval_secs.to_string(),
        )?;
        set_setting(
            &conn,
            "bandwidth_interval_ms",
            &s.bandwidth_interval_ms.to_string(),
        )?;
        set_setting(
            &conn,
            "retention_raw_days",
            &s.retention_raw_days.to_string(),
        )?;
        set_setting(
            &conn,
            "start_ui_with_windows",
            if s.start_ui_with_windows { "1" } else { "0" },
        )?;
        if let Some(ref p) = s.speedtest_cli_path {
            set_setting(&conn, "speedtest_cli_path", p)?;
        }
        set_setting(
            &conn,
            "speedtest_eula_accepted",
            if s.speedtest_eula_accepted { "1" } else { "0" },
        )?;
        set_setting(
            &conn,
            "default_nic_id",
            s.default_nic_id.as_deref().unwrap_or(""),
        )?;
        set_setting(
            &conn,
            "strip_width_px",
            &s.strip_width_px.to_string(),
        )?;
        set_setting(
            &conn,
            "strip_font_px",
            &s.strip_font_px.to_string(),
        )?;
        set_setting(&conn, "strip_slot", s.strip_slot.as_str())?;
        set_setting(
            &conn,
            "strip_offset_px",
            &s.strip_offset_px.to_string(),
        )?;
        set_setting(
            &conn,
            "ignore_private_ips",
            if s.ignore_private_ips { "1" } else { "0" },
        )?;
        set_setting(
            &conn,
            "ignored_apps",
            &serde_json::to_string(&s.ignored_apps)?,
        )?;
        set_setting(
            &conn,
            "ignored_ips",
            &serde_json::to_string(&s.ignored_ips)?,
        )?;
        set_setting(
            &conn,
            "ignored_urls",
            &serde_json::to_string(&s.ignored_urls)?,
        )?;
        set_setting(&conn, "theme", s.theme.as_str())?;
        set_setting(
            &conn,
            "system_interval_ms",
            &s.system_interval_ms.to_string(),
        )?;
        set_setting(
            &conn,
            "system_persist_interval_ms",
            &s.system_persist_interval_ms.to_string(),
        )?;
        set_setting(
            &conn,
            "system_raw_retention_days",
            &s.system_raw_retention_days.to_string(),
        )?;
        set_setting(
            &conn,
            "system_hourly_retention_days",
            &s.system_hourly_retention_days.to_string(),
        )?;
        Ok(())
    }

    pub fn upsert_nic(&self, nic: &NicInfo) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            r#"INSERT INTO nics (
                id, guid, name, description, interface_index, mac, mtu, media_type,
                oper_status, admin_status, link_speed_bps, ipv4_json, ipv6_json,
                gateways_json, dns_json, dhcp_enabled, wifi_ssid, wifi_bssid,
                wifi_signal, driver, updated_at
            ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21)
            ON CONFLICT(id) DO UPDATE SET
                name=excluded.name, description=excluded.description,
                interface_index=excluded.interface_index, mac=excluded.mac, mtu=excluded.mtu,
                media_type=excluded.media_type, oper_status=excluded.oper_status,
                admin_status=excluded.admin_status, link_speed_bps=excluded.link_speed_bps,
                ipv4_json=excluded.ipv4_json, ipv6_json=excluded.ipv6_json,
                gateways_json=excluded.gateways_json, dns_json=excluded.dns_json,
                dhcp_enabled=excluded.dhcp_enabled, wifi_ssid=excluded.wifi_ssid,
                wifi_bssid=excluded.wifi_bssid, wifi_signal=excluded.wifi_signal,
                driver=excluded.driver, updated_at=excluded.updated_at
            "#,
            params![
                nic.id,
                nic.guid,
                nic.name,
                nic.description,
                nic.interface_index,
                nic.mac,
                nic.mtu,
                nic.media_type,
                nic.oper_status,
                nic.admin_status,
                nic.link_speed_bps.map(|v| v as i64),
                serde_json::to_string(&nic.ipv4_addresses)?,
                serde_json::to_string(&nic.ipv6_addresses)?,
                serde_json::to_string(&nic.gateways)?,
                serde_json::to_string(&nic.dns_servers)?,
                nic.dhcp_enabled as i32,
                nic.wifi_ssid,
                nic.wifi_bssid,
                nic.wifi_signal,
                nic.driver,
                Utc::now().timestamp(),
            ],
        )?;
        Ok(())
    }

    pub fn list_nics_cached(&self) -> Result<Vec<NicInfo>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, guid, name, description, interface_index, mac, mtu, media_type,
             oper_status, admin_status, link_speed_bps, ipv4_json, ipv6_json, gateways_json,
             dns_json, dhcp_enabled, wifi_ssid, wifi_bssid, wifi_signal, driver
             FROM nics ORDER BY name",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(NicInfo {
                id: row.get(0)?,
                guid: row.get(1)?,
                name: row.get(2)?,
                description: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                interface_index: row.get::<_, Option<u32>>(4)?.unwrap_or(0),
                mac: row.get(5)?,
                mtu: row.get(6)?,
                media_type: row.get::<_, Option<String>>(7)?.unwrap_or_else(|| "Unknown".into()),
                oper_status: row.get::<_, Option<String>>(8)?.unwrap_or_else(|| "Unknown".into()),
                admin_status: row.get::<_, Option<String>>(9)?.unwrap_or_else(|| "Unknown".into()),
                link_speed_bps: row
                    .get::<_, Option<i64>>(10)?
                    .map(|v| v as u64),
                ipv4_addresses: parse_json_vec(row.get(11)?),
                ipv6_addresses: parse_json_vec(row.get(12)?),
                gateways: parse_json_vec(row.get(13)?),
                dns_servers: parse_json_vec(row.get(14)?),
                dhcp_enabled: row.get::<_, Option<i32>>(15)?.unwrap_or(0) != 0,
                wifi_ssid: row.get(16)?,
                wifi_bssid: row.get(17)?,
                wifi_signal: row.get(18)?,
                driver: row.get(19)?,
                rx_bytes: 0,
                tx_bytes: 0,
                rx_bps: 0.0,
                tx_bps: 0.0,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn insert_bandwidth(&self, s: &BandwidthSample) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO nic_bandwidth_samples (nic_id, ts, rx_bytes, tx_bytes, rx_bps, tx_bps)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![s.nic_id, s.ts, s.rx_bytes as i64, s.tx_bytes as i64, s.rx_bps, s.tx_bps],
        )?;
        let hour = s.ts - (s.ts % 3600);
        conn.execute(
            "INSERT INTO nic_usage_hourly (nic_id, hour_ts, rx_bytes, tx_bytes)
             VALUES (?1,?2,?3,?4)
             ON CONFLICT(nic_id, hour_ts) DO UPDATE SET
               rx_bytes = nic_usage_hourly.rx_bytes + excluded.rx_bytes,
               tx_bytes = nic_usage_hourly.tx_bytes + excluded.tx_bytes",
            params![s.nic_id, hour, (s.rx_bps / 8.0) as i64, (s.tx_bps / 8.0) as i64],
        )?;
        Ok(())
    }

    pub fn insert_ping(&self, s: &PingSample) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO ping_samples (nic_id, target, ts, rtt_ms, success, error)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                s.nic_id,
                s.target,
                s.ts,
                s.rtt_ms,
                s.success as i32,
                s.error
            ],
        )?;
        Ok(())
    }

    pub fn insert_http_latency(&self, s: &HttpLatencySample) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO http_latency_samples
             (nic_id, url, ts, dns_ms, connect_ms, tls_ms, ttfb_ms, total_ms, status_code, success, error)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![
                s.nic_id,
                s.url,
                s.ts,
                s.dns_ms,
                s.connect_ms,
                s.tls_ms,
                s.ttfb_ms,
                s.total_ms,
                s.status_code.map(|c| c as i64),
                s.success as i32,
                s.error
            ],
        )?;
        Ok(())
    }

    pub fn insert_link_event(&self, e: &LinkEvent) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO nic_link_events (nic_id, ts, event, detail) VALUES (?1,?2,?3,?4)",
            params![e.nic_id, e.ts, e.event, e.detail],
        )?;
        Ok(())
    }

    pub fn insert_traceroute(&self, r: &TracerouteResult) -> Result<i64> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO traceroute_runs (nic_id, target, ts) VALUES (?1,?2,?3)",
            params![r.nic_id, r.target, r.ts],
        )?;
        let id = conn.last_insert_rowid();
        for h in &r.hops {
            conn.execute(
                "INSERT INTO traceroute_hops (run_id, hop, address, hostname, rtt_ms)
                 VALUES (?1,?2,?3,?4,?5)",
                params![id, h.hop, h.address, h.hostname, h.rtt_ms],
            )?;
        }
        Ok(id)
    }

    pub fn insert_speedtest(&self, r: &SpeedtestResult) -> Result<i64> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO speedtest_runs
             (nic_id, ts, server_id, server_name, download_mbps, upload_mbps, ping_ms, jitter_ms, packet_loss, raw_json)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                r.nic_id,
                r.ts,
                r.server_id,
                r.server_name,
                r.download_mbps,
                r.upload_mbps,
                r.ping_ms,
                r.jitter_ms,
                r.packet_loss,
                r.raw_json
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn insert_traffic_flow(&self, f: &TrafficFlow) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO traffic_flows
             (ts, process_name, process_path, pid, local_addr, remote_addr, protocol, bytes_in, bytes_out, nic_id, host)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![
                f.ts,
                f.process_name,
                f.process_path,
                f.pid.map(|p| p as i64),
                f.local_addr,
                f.remote_addr,
                f.protocol,
                f.bytes_in as i64,
                f.bytes_out as i64,
                f.nic_id,
                f.host
            ],
        )?;
        let hour = f.ts - (f.ts % 3600);
        conn.execute(
            "INSERT INTO app_usage_hourly (hour_ts, process_name, remote_ip, host, nic_id, bytes_in, bytes_out)
             VALUES (?1,?2,?3,?4,?5,?6,?7)
             ON CONFLICT(hour_ts, process_name, remote_ip, host, nic_id) DO UPDATE SET
               bytes_in = app_usage_hourly.bytes_in + excluded.bytes_in,
               bytes_out = app_usage_hourly.bytes_out + excluded.bytes_out",
            params![
                hour,
                f.process_name,
                f.remote_addr.split(':').next().unwrap_or("").to_string(),
                f.host.clone().unwrap_or_default(),
                f.nic_id.clone().unwrap_or_default(),
                f.bytes_in as i64,
                f.bytes_out as i64
            ],
        )?;
        Ok(())
    }

    pub fn insert_dns_query(&self, name: &str, ips: &[String], process: Option<&str>) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO dns_queries (ts, name, resolved_ips, process_name) VALUES (?1,?2,?3,?4)",
            params![
                Utc::now().timestamp(),
                name,
                serde_json::to_string(ips)?,
                process
            ],
        )?;
        Ok(())
    }

    pub fn insert_http_host(
        &self,
        host: &str,
        remote_ip: Option<&str>,
        process: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO http_hosts (ts, host, remote_ip, process_name) VALUES (?1,?2,?3,?4)",
            params![Utc::now().timestamp(), host, remote_ip, process],
        )?;
        Ok(())
    }

    pub fn bandwidth_history(
        &self,
        nic_id: Option<&str>,
        range: &TimeRange,
    ) -> Result<Vec<UsagePoint>> {
        let conn = self.conn.lock();
        let mut sql = String::from(
            "SELECT ts, rx_bytes, tx_bytes FROM nic_bandwidth_samples WHERE 1=1",
        );
        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(id) = nic_id {
            sql.push_str(" AND nic_id = ?");
            params_vec.push(Box::new(id.to_string()));
        }
        append_range(&mut sql, &mut params_vec, range);
        sql.push_str(" ORDER BY ts ASC LIMIT 5000");
        let mut stmt = conn.prepare(&sql)?;
        let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(params_refs.as_slice(), |row| {
            Ok(UsagePoint {
                ts: row.get(0)?,
                rx_bytes: row.get::<_, i64>(1)? as u64,
                tx_bytes: row.get::<_, i64>(2)? as u64,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn ping_history(
        &self,
        nic_id: Option<&str>,
        range: &TimeRange,
    ) -> Result<Vec<PingSample>> {
        let conn = self.conn.lock();
        let mut sql = String::from(
            "SELECT nic_id, target, ts, rtt_ms, success, error FROM ping_samples WHERE 1=1",
        );
        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(id) = nic_id {
            sql.push_str(" AND nic_id = ?");
            params_vec.push(Box::new(id.to_string()));
        }
        append_range(&mut sql, &mut params_vec, range);
        sql.push_str(" ORDER BY ts ASC LIMIT 5000");
        let mut stmt = conn.prepare(&sql)?;
        let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(params_refs.as_slice(), |row| {
            Ok(PingSample {
                nic_id: row.get(0)?,
                target: row.get(1)?,
                ts: row.get(2)?,
                rtt_ms: row.get(3)?,
                success: row.get::<_, i32>(4)? != 0,
                error: row.get(5)?,
                raw_output: None,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn http_latency_history(
        &self,
        nic_id: Option<&str>,
        range: &TimeRange,
    ) -> Result<Vec<HttpLatencySample>> {
        let conn = self.conn.lock();
        let mut sql = String::from(
            "SELECT nic_id, url, ts, dns_ms, connect_ms, tls_ms, ttfb_ms, total_ms, status_code, success, error
             FROM http_latency_samples WHERE 1=1",
        );
        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(id) = nic_id {
            sql.push_str(" AND nic_id = ?");
            params_vec.push(Box::new(id.to_string()));
        }
        append_range(&mut sql, &mut params_vec, range);
        sql.push_str(" ORDER BY ts ASC LIMIT 5000");
        let mut stmt = conn.prepare(&sql)?;
        let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(params_refs.as_slice(), |row| {
            Ok(HttpLatencySample {
                nic_id: row.get(0)?,
                url: row.get(1)?,
                ts: row.get(2)?,
                dns_ms: row.get(3)?,
                connect_ms: row.get(4)?,
                tls_ms: row.get(5)?,
                ttfb_ms: row.get(6)?,
                total_ms: row.get(7)?,
                status_code: row.get::<_, Option<i64>>(8)?.map(|c| c as u16),
                success: row.get::<_, i32>(9)? != 0,
                error: row.get(10)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn link_events(
        &self,
        nic_id: Option<&str>,
        range: &TimeRange,
    ) -> Result<Vec<LinkEvent>> {
        let conn = self.conn.lock();
        let mut sql =
            String::from("SELECT nic_id, ts, event, detail FROM nic_link_events WHERE 1=1");
        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(id) = nic_id {
            sql.push_str(" AND nic_id = ?");
            params_vec.push(Box::new(id.to_string()));
        }
        append_range(&mut sql, &mut params_vec, range);
        sql.push_str(" ORDER BY ts DESC LIMIT 2000");
        let mut stmt = conn.prepare(&sql)?;
        let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(params_refs.as_slice(), |row| {
            Ok(LinkEvent {
                nic_id: row.get(0)?,
                ts: row.get(1)?,
                event: row.get(2)?,
                detail: row.get(3)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn app_usage(
        &self,
        range: &TimeRange,
        group_by: &str,
        nic_id: Option<&str>,
        ignored_apps: &[String],
        ignored_ips: &[String],
        ignored_urls: &[String],
        ignore_private_ips: bool,
    ) -> Result<Vec<AppUsageRow>> {
        let conn = self.conn.lock();
        let (select, group) = match group_by {
            "ip" => (
                "remote_ip as process_name, NULL as process_path, remote_ip, NULL as host, SUM(bytes_in), SUM(bytes_out), nic_id",
                "remote_ip, nic_id",
            ),
            "url" | "host" => (
                "COALESCE(host,'(unknown)') as process_name, NULL, NULL, host, SUM(bytes_in), SUM(bytes_out), nic_id",
                "host, nic_id",
            ),
            _ => (
                "process_name, NULL as process_path, NULL as remote_ip, NULL as host, SUM(bytes_in), SUM(bytes_out), nic_id",
                "process_name, nic_id",
            ),
        };
        let mut sql = format!("SELECT {select} FROM app_usage_hourly WHERE 1=1");
        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(start) = range.start_ts {
            sql.push_str(" AND hour_ts >= ?");
            params_vec.push(Box::new(start));
        }
        if let Some(end) = range.end_ts {
            sql.push_str(" AND hour_ts <= ?");
            params_vec.push(Box::new(end));
        }
        if let Some(id) = nic_id {
            if !id.is_empty() {
                sql.push_str(" AND nic_id = ?");
                params_vec.push(Box::new(id.to_string()));
            }
        }
        sql.push_str(&format!(
            " GROUP BY {group} ORDER BY SUM(bytes_in)+SUM(bytes_out) DESC LIMIT 200"
        ));
        let mut stmt = conn.prepare(&sql)?;
        let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(params_refs.as_slice(), |row| {
            Ok(AppUsageRow {
                process_name: row.get::<_, Option<String>>(0)?.unwrap_or_else(|| "(unknown)".into()),
                process_path: row.get(1)?,
                remote_ip: row.get(2)?,
                host: row.get(3)?,
                bytes_in: row.get::<_, i64>(4)? as u64,
                bytes_out: row.get::<_, i64>(5)? as u64,
                nic_id: row.get(6)?,
            })
        })?;
        let ignored_apps_l: Vec<String> = ignored_apps.iter().map(|s| s.to_lowercase()).collect();
        let ignored_ips_l: Vec<String> = ignored_ips.iter().map(|s| s.to_lowercase()).collect();
        let ignored_urls_l: Vec<String> = ignored_urls.iter().map(|s| s.to_lowercase()).collect();
        Ok(rows
            .filter_map(|r| r.ok())
            .filter(|row| {
                !is_ignored_usage_row(row, group_by, &ignored_apps_l, &ignored_ips_l, &ignored_urls_l)
                    && !(ignore_private_ips && is_private_usage_row(row, group_by))
            })
            .collect())
    }

    pub fn app_usage_series(
        &self,
        range: &TimeRange,
        group_by: &str,
        nic_id: Option<&str>,
        ignored_apps: &[String],
        ignored_ips: &[String],
        ignored_urls: &[String],
        ignore_private_ips: bool,
    ) -> Result<Vec<AppUsageSeriesPoint>> {
        let conn = self.conn.lock();
        let (select, group) = match group_by {
            "ip" => (
                "hour_ts, remote_ip as process_name, remote_ip, NULL as host, SUM(bytes_in), SUM(bytes_out)",
                "hour_ts, remote_ip",
            ),
            "url" | "host" => (
                "hour_ts, COALESCE(host,'(unknown)') as process_name, NULL as remote_ip, host, SUM(bytes_in), SUM(bytes_out)",
                "hour_ts, host",
            ),
            _ => (
                "hour_ts, process_name, NULL as remote_ip, NULL as host, SUM(bytes_in), SUM(bytes_out)",
                "hour_ts, process_name",
            ),
        };
        let mut sql = format!("SELECT {select} FROM app_usage_hourly WHERE 1=1");
        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(start) = range.start_ts {
            sql.push_str(" AND hour_ts >= ?");
            params_vec.push(Box::new(start));
        }
        if let Some(end) = range.end_ts {
            sql.push_str(" AND hour_ts <= ?");
            params_vec.push(Box::new(end));
        }
        if let Some(id) = nic_id {
            if !id.is_empty() {
                sql.push_str(" AND nic_id = ?");
                params_vec.push(Box::new(id.to_string()));
            }
        }
        sql.push_str(&format!(" GROUP BY {group} ORDER BY hour_ts ASC LIMIT 50000"));
        let mut stmt = conn.prepare(&sql)?;
        let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(params_refs.as_slice(), |row| {
            Ok(AppUsageSeriesPoint {
                hour_ts: row.get(0)?,
                process_name: row
                    .get::<_, Option<String>>(1)?
                    .unwrap_or_else(|| "(unknown)".into()),
                remote_ip: row.get(2)?,
                host: row.get(3)?,
                bytes_in: row.get::<_, i64>(4)? as u64,
                bytes_out: row.get::<_, i64>(5)? as u64,
            })
        })?;
        let ignored_apps_l: Vec<String> = ignored_apps.iter().map(|s| s.to_lowercase()).collect();
        let ignored_ips_l: Vec<String> = ignored_ips.iter().map(|s| s.to_lowercase()).collect();
        let ignored_urls_l: Vec<String> = ignored_urls.iter().map(|s| s.to_lowercase()).collect();
        Ok(rows
            .filter_map(|r| r.ok())
            .filter(|pt| {
                let row = AppUsageRow {
                    process_name: pt.process_name.clone(),
                    process_path: None,
                    remote_ip: pt.remote_ip.clone(),
                    host: pt.host.clone(),
                    bytes_in: pt.bytes_in,
                    bytes_out: pt.bytes_out,
                    nic_id: None,
                };
                !is_ignored_usage_row(
                    &row,
                    group_by,
                    &ignored_apps_l,
                    &ignored_ips_l,
                    &ignored_urls_l,
                ) && !(ignore_private_ips && is_private_usage_row(&row, group_by))
            })
            .collect())
    }

    pub fn speedtest_history(&self, range: &TimeRange) -> Result<Vec<SpeedtestResult>> {
        let conn = self.conn.lock();
        let mut sql = String::from(
            "SELECT id, nic_id, ts, server_id, server_name, download_mbps, upload_mbps, ping_ms, jitter_ms, packet_loss, raw_json
             FROM speedtest_runs WHERE 1=1",
        );
        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        append_range(&mut sql, &mut params_vec, range);
        sql.push_str(" ORDER BY ts DESC LIMIT 500");
        let mut stmt = conn.prepare(&sql)?;
        let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(params_refs.as_slice(), |row| {
            Ok(SpeedtestResult {
                id: row.get(0)?,
                nic_id: row.get(1)?,
                ts: row.get(2)?,
                server_id: row.get(3)?,
                server_name: row.get(4)?,
                download_mbps: row.get(5)?,
                upload_mbps: row.get(6)?,
                ping_ms: row.get(7)?,
                jitter_ms: row.get(8)?,
                packet_loss: row.get(9)?,
                raw_json: row.get(10)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn prune_raw(&self, retention_days: u32) -> Result<usize> {
        let cutoff = Utc::now().timestamp() - (retention_days as i64 * 86400);
        let conn = self.conn.lock();
        let mut total = 0usize;
        for table in [
            "nic_bandwidth_samples",
            "ping_samples",
            "http_latency_samples",
            "traffic_flows",
            "dns_queries",
            "http_hosts",
        ] {
            total += conn.execute(
                &format!("DELETE FROM {table} WHERE ts < ?1"),
                params![cutoff],
            )?;
        }
        Ok(total)
    }

    pub fn prune_system_metrics(
        &self,
        raw_retention_days: u32,
        hourly_retention_days: u32,
    ) -> Result<usize> {
        let now = Utc::now().timestamp();
        let raw_cutoff = now - (raw_retention_days as i64 * 86400);
        let hourly_cutoff = now - (hourly_retention_days as i64 * 86400);
        let conn = self.conn.lock();
        let mut total = 0usize;
        for table in ["cpu_samples", "memory_samples", "disk_samples"] {
            total += conn.execute(
                &format!("DELETE FROM {table} WHERE ts < ?1"),
                params![raw_cutoff],
            )?;
        }
        for table in ["cpu_stats_hourly", "memory_stats_hourly"] {
            total += conn.execute(
                &format!("DELETE FROM {table} WHERE hour_ts < ?1"),
                params![hourly_cutoff],
            )?;
        }
        total += conn.execute(
            "DELETE FROM disk_stats_hourly WHERE hour_ts < ?1",
            params![hourly_cutoff],
        )?;
        // Drop inventory rows with no remaining samples or hourly stats.
        total += conn.execute(
            "DELETE FROM disks WHERE id NOT IN (
                SELECT DISTINCT disk_id FROM disk_samples
                UNION
                SELECT DISTINCT disk_id FROM disk_stats_hourly
             ) AND updated_at < ?1",
            params![raw_cutoff],
        )?;
        Ok(total)
    }

    pub fn persist_system_tick(
        &self,
        ts: i64,
        cpu_util: f64,
        mem_util: f64,
        mem_used: u64,
        disks: &[DiskSnapshot],
    ) -> Result<()> {
        let conn = self.conn.lock();
        let tx = conn.unchecked_transaction()?;
        let hour = ts - (ts % 3600);

        tx.execute(
            "INSERT INTO cpu_samples (ts, utilization) VALUES (?1, ?2)
             ON CONFLICT(ts) DO UPDATE SET utilization = excluded.utilization",
            params![ts, cpu_util],
        )?;
        upsert_hourly(&tx, "cpu_stats_hourly", None, hour, cpu_util)?;

        tx.execute(
            "INSERT INTO memory_samples (ts, utilization, used_bytes) VALUES (?1, ?2, ?3)
             ON CONFLICT(ts) DO UPDATE SET
               utilization = excluded.utilization,
               used_bytes = excluded.used_bytes",
            params![ts, mem_util, mem_used as i64],
        )?;
        upsert_hourly(&tx, "memory_stats_hourly", None, hour, mem_util)?;

        for d in disks {
            tx.execute(
                "INSERT INTO disks (id, name, mount_point, file_system, kind, total_bytes, updated_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7)
                 ON CONFLICT(id) DO UPDATE SET
                   name=excluded.name,
                   mount_point=excluded.mount_point,
                   file_system=excluded.file_system,
                   kind=excluded.kind,
                   total_bytes=excluded.total_bytes,
                   updated_at=excluded.updated_at",
                params![
                    d.id,
                    d.name,
                    d.mount_point,
                    d.file_system,
                    d.kind.as_str(),
                    d.total_bytes as i64,
                    ts
                ],
            )?;
            tx.execute(
                "INSERT INTO disk_samples (disk_id, ts, utilization, used_bytes)
                 VALUES (?1,?2,?3,?4)
                 ON CONFLICT(disk_id, ts) DO UPDATE SET
                   utilization = excluded.utilization,
                   used_bytes = excluded.used_bytes",
                params![d.id, ts, d.utilization, d.used_bytes as i64],
            )?;
            upsert_hourly(&tx, "disk_stats_hourly", Some(&d.id), hour, d.utilization)?;
        }

        tx.commit()?;
        Ok(())
    }

    pub fn cpu_history(
        &self,
        range: &TimeRange,
        history_range: HistoryRange,
    ) -> Result<SystemMetricHistory> {
        self.metric_history("cpu", None, None, range, history_range)
    }

    pub fn memory_history(
        &self,
        range: &TimeRange,
        history_range: HistoryRange,
    ) -> Result<SystemMetricHistory> {
        self.metric_history("memory", None, None, range, history_range)
    }

    pub fn disk_history(
        &self,
        disk_id: Option<&str>,
        kind: Option<DiskKind>,
        range: &TimeRange,
        history_range: HistoryRange,
    ) -> Result<SystemMetricHistory> {
        self.metric_history("disk", disk_id, kind, range, history_range)
    }

    fn metric_history(
        &self,
        kind: &str,
        disk_id: Option<&str>,
        disk_kind: Option<DiskKind>,
        range: &TimeRange,
        history_range: HistoryRange,
    ) -> Result<SystemMetricHistory> {
        let use_hourly = should_use_hourly(history_range, range);
        let conn = self.conn.lock();
        if use_hourly {
            read_hourly_history(&conn, kind, disk_id, disk_kind, range)
        } else {
            read_raw_history(&conn, kind, disk_id, disk_kind, range)
        }
    }
}

fn upsert_hourly(
    tx: &rusqlite::Transaction<'_>,
    table: &str,
    disk_id: Option<&str>,
    hour_ts: i64,
    value: f64,
) -> Result<()> {
    if let Some(id) = disk_id {
        tx.execute(
            &format!(
                "INSERT INTO {table} (disk_id, hour_ts, avg, min, max, sample_count)
                 VALUES (?1, ?2, ?3, ?3, ?3, 1)
                 ON CONFLICT(disk_id, hour_ts) DO UPDATE SET
                   avg = ({table}.avg * {table}.sample_count + excluded.avg)
                         / ({table}.sample_count + 1),
                   min = MIN({table}.min, excluded.min),
                   max = MAX({table}.max, excluded.max),
                   sample_count = {table}.sample_count + 1"
            ),
            params![id, hour_ts, value],
        )?;
    } else {
        tx.execute(
            &format!(
                "INSERT INTO {table} (hour_ts, avg, min, max, sample_count)
                 VALUES (?1, ?2, ?2, ?2, 1)
                 ON CONFLICT(hour_ts) DO UPDATE SET
                   avg = ({table}.avg * {table}.sample_count + excluded.avg)
                         / ({table}.sample_count + 1),
                   min = MIN({table}.min, excluded.min),
                   max = MAX({table}.max, excluded.max),
                   sample_count = {table}.sample_count + 1"
            ),
            params![hour_ts, value],
        )?;
    }
    Ok(())
}

fn should_use_hourly(history_range: HistoryRange, range: &TimeRange) -> bool {
    match history_range {
        HistoryRange::Today | HistoryRange::Yesterday => false,
        HistoryRange::Week | HistoryRange::Months | HistoryRange::All => true,
        HistoryRange::Custom => {
            let start = range.start_ts.unwrap_or(0);
            let end = range.end_ts.unwrap_or(Utc::now().timestamp());
            (end - start) > 2 * 86400
        }
    }
}

fn read_raw_history(
    conn: &Connection,
    kind: &str,
    disk_id: Option<&str>,
    disk_kind: Option<DiskKind>,
    range: &TimeRange,
) -> Result<SystemMetricHistory> {
    let span = range_span_secs(range);
    let downsample = span > 3000 * 5; // denser than ~3000 points at 5s

    let (series_sql, summary_sql, params_vec) = match kind {
        "cpu" => {
            let mut p: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
            let mut where_sql = String::from(" WHERE 1=1");
            append_range_col(&mut where_sql, &mut p, range, "ts");
            let series = if downsample {
                format!(
                    "SELECT (ts / 60) * 60 AS ts, AVG(utilization) AS value
                     FROM cpu_samples{where_sql}
                     GROUP BY ts / 60 ORDER BY ts ASC"
                )
            } else {
                format!(
                    "SELECT ts, utilization FROM cpu_samples{where_sql} ORDER BY ts ASC LIMIT 5000"
                )
            };
            let summary = format!(
                "SELECT AVG(utilization), MIN(utilization), MAX(utilization), COUNT(*)
                 FROM cpu_samples{where_sql}"
            );
            (series, summary, p)
        }
        "memory" => {
            let mut p: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
            let mut where_sql = String::from(" WHERE 1=1");
            append_range_col(&mut where_sql, &mut p, range, "ts");
            let series = if downsample {
                format!(
                    "SELECT (ts / 60) * 60 AS ts, AVG(utilization) AS value
                     FROM memory_samples{where_sql}
                     GROUP BY ts / 60 ORDER BY ts ASC"
                )
            } else {
                format!(
                    "SELECT ts, utilization FROM memory_samples{where_sql} ORDER BY ts ASC LIMIT 5000"
                )
            };
            let summary = format!(
                "SELECT AVG(utilization), MIN(utilization), MAX(utilization), COUNT(*)
                 FROM memory_samples{where_sql}"
            );
            (series, summary, p)
        }
        "disk" => {
            let mut p: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
            let mut where_sql = String::from(
                " FROM disk_samples s JOIN disks d ON d.id = s.disk_id WHERE 1=1",
            );
            if let Some(id) = disk_id {
                where_sql.push_str(" AND s.disk_id = ?");
                p.push(Box::new(id.to_string()));
            }
            if let Some(k) = disk_kind {
                where_sql.push_str(" AND d.kind = ?");
                p.push(Box::new(k.as_str().to_string()));
            }
            append_range_col(&mut where_sql, &mut p, range, "s.ts");
            let series = if downsample {
                format!(
                    "SELECT (s.ts / 60) * 60 AS ts, AVG(s.utilization) AS value
                     {where_sql}
                     GROUP BY s.ts / 60 ORDER BY ts ASC"
                )
            } else {
                format!(
                    "SELECT s.ts, s.utilization {where_sql} ORDER BY s.ts ASC LIMIT 5000"
                )
            };
            let summary = format!(
                "SELECT AVG(s.utilization), MIN(s.utilization), MAX(s.utilization), COUNT(*)
                 {where_sql}"
            );
            (series, summary, p)
        }
        _ => anyhow::bail!("unknown metric kind: {kind}"),
    };

    let series = query_series(conn, &series_sql, &params_vec)?;
    let summary = query_summary(conn, &summary_sql, &params_vec)?;
    Ok(SystemMetricHistory { series, summary })
}

fn read_hourly_history(
    conn: &Connection,
    kind: &str,
    disk_id: Option<&str>,
    disk_kind: Option<DiskKind>,
    range: &TimeRange,
) -> Result<SystemMetricHistory> {
    let (series_sql, summary_sql, params_vec) = match kind {
        "cpu" => {
            let mut p: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
            let mut where_sql = String::from(" WHERE 1=1");
            append_range_col(&mut where_sql, &mut p, range, "hour_ts");
            let series = format!(
                "SELECT hour_ts, avg FROM cpu_stats_hourly{where_sql} ORDER BY hour_ts ASC"
            );
            let summary = format!(
                "SELECT CASE WHEN SUM(sample_count) > 0
                    THEN SUM(avg * sample_count) / SUM(sample_count) END,
                  MIN(min), MAX(max), COALESCE(SUM(sample_count), 0)
                 FROM cpu_stats_hourly{where_sql}"
            );
            (series, summary, p)
        }
        "memory" => {
            let mut p: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
            let mut where_sql = String::from(" WHERE 1=1");
            append_range_col(&mut where_sql, &mut p, range, "hour_ts");
            let series = format!(
                "SELECT hour_ts, avg FROM memory_stats_hourly{where_sql} ORDER BY hour_ts ASC"
            );
            let summary = format!(
                "SELECT CASE WHEN SUM(sample_count) > 0
                    THEN SUM(avg * sample_count) / SUM(sample_count) END,
                  MIN(min), MAX(max), COALESCE(SUM(sample_count), 0)
                 FROM memory_stats_hourly{where_sql}"
            );
            (series, summary, p)
        }
        "disk" => {
            let mut p: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
            let mut where_sql = String::from(
                " FROM disk_stats_hourly s JOIN disks d ON d.id = s.disk_id WHERE 1=1",
            );
            if let Some(id) = disk_id {
                where_sql.push_str(" AND s.disk_id = ?");
                p.push(Box::new(id.to_string()));
            }
            if let Some(k) = disk_kind {
                where_sql.push_str(" AND d.kind = ?");
                p.push(Box::new(k.as_str().to_string()));
            }
            append_range_col(&mut where_sql, &mut p, range, "s.hour_ts");
            let series = format!(
                "SELECT s.hour_ts, s.avg {where_sql} ORDER BY s.hour_ts ASC"
            );
            let summary = format!(
                "SELECT CASE WHEN SUM(s.sample_count) > 0
                    THEN SUM(s.avg * s.sample_count) / SUM(s.sample_count) END,
                  MIN(s.min), MAX(s.max), COALESCE(SUM(s.sample_count), 0)
                 {where_sql}"
            );
            (series, summary, p)
        }
        _ => anyhow::bail!("unknown metric kind: {kind}"),
    };

    let series = query_series(conn, &series_sql, &params_vec)?;
    let summary = query_summary(conn, &summary_sql, &params_vec)?;
    Ok(SystemMetricHistory { series, summary })
}

fn range_span_secs(range: &TimeRange) -> i64 {
    let start = range.start_ts.unwrap_or(0);
    let end = range.end_ts.unwrap_or(Utc::now().timestamp());
    (end - start).max(0)
}

fn append_range_col(
    sql: &mut String,
    params_vec: &mut Vec<Box<dyn rusqlite::ToSql>>,
    range: &TimeRange,
    col: &str,
) {
    if let Some(start) = range.start_ts {
        sql.push_str(&format!(" AND {col} >= ?"));
        params_vec.push(Box::new(start));
    }
    if let Some(end) = range.end_ts {
        sql.push_str(&format!(" AND {col} <= ?"));
        params_vec.push(Box::new(end));
    }
}

fn query_series(
    conn: &Connection,
    sql: &str,
    params_vec: &[Box<dyn rusqlite::ToSql>],
) -> Result<Vec<SystemMetricPoint>> {
    let mut stmt = conn.prepare(sql)?;
    let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
    let rows = stmt.query_map(params_refs.as_slice(), |row| {
        Ok(SystemMetricPoint {
            ts: row.get(0)?,
            value: row.get(1)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

fn query_summary(
    conn: &Connection,
    sql: &str,
    params_vec: &[Box<dyn rusqlite::ToSql>],
) -> Result<MetricSummary> {
    let mut stmt = conn.prepare(sql)?;
    let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
    stmt.query_row(params_refs.as_slice(), |row| {
        let count: i64 = row.get(3)?;
        Ok(MetricSummary {
            avg: row.get(0)?,
            min: row.get(1)?,
            max: row.get(2)?,
            sample_count: count.max(0) as u64,
        })
    })
    .optional()
    .map(|o| {
        o.unwrap_or(MetricSummary {
            avg: None,
            min: None,
            max: None,
            sample_count: 0,
        })
    })
    .map_err(Into::into)
}

fn get_setting(conn: &Connection, key: &str) -> Result<Option<String>> {
    let mut stmt = conn.prepare("SELECT value FROM settings WHERE key = ?1")?;
    let v = stmt
        .query_row(params![key], |row| row.get(0))
        .optional()?;
    Ok(v)
}

fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1,?2)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, value],
    )?;
    Ok(())
}

fn parse_json_vec(s: Option<String>) -> Vec<String> {
    s.and_then(|j| serde_json::from_str(&j).ok())
        .unwrap_or_default()
}

fn is_ignored_usage_row(
    row: &AppUsageRow,
    group_by: &str,
    ignored_apps: &[String],
    ignored_ips: &[String],
    ignored_urls: &[String],
) -> bool {
    match group_by {
        "ip" => {
            let ip = row
                .remote_ip
                .as_deref()
                .unwrap_or(row.process_name.as_str())
                .to_lowercase();
            ignored_ips.iter().any(|ig| ig == &ip)
        }
        "url" | "host" => {
            let host = row
                .host
                .as_deref()
                .unwrap_or(row.process_name.as_str())
                .to_lowercase();
            ignored_urls.iter().any(|ig| host == *ig || host.ends_with(&format!(".{}", ig)))
        }
        _ => {
            let name = row.process_name.to_lowercase();
            ignored_apps.iter().any(|ig| ig == &name)
        }
    }
}

/// True for private / loopback / link-local / CGNAT IPv4 (same rules as geo skip).
fn is_non_public_ipv4(ip: &str) -> bool {
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
        return true;
    }
    if a == 169 && b == 254 {
        return true;
    }
    if a == 172 && (16..=31).contains(&b) {
        return true;
    }
    if a == 192 && b == 168 {
        return true;
    }
    if a == 100 && (64..=127).contains(&b) {
        return true;
    }
    false
}

fn is_private_usage_row(row: &AppUsageRow, group_by: &str) -> bool {
    match group_by {
        "ip" => {
            let ip = row
                .remote_ip
                .as_deref()
                .unwrap_or(row.process_name.as_str());
            is_non_public_ipv4(ip)
        }
        _ => row
            .remote_ip
            .as_deref()
            .map(is_non_public_ipv4)
            .unwrap_or(false),
    }
}

fn append_range(
    sql: &mut String,
    params_vec: &mut Vec<Box<dyn rusqlite::ToSql>>,
    range: &TimeRange,
) {
    if let Some(start) = range.start_ts {
        sql.push_str(" AND ts >= ?");
        params_vec.push(Box::new(start));
    }
    if let Some(end) = range.end_ts {
        sql.push_str(" AND ts <= ?");
        params_vec.push(Box::new(end));
    }
}
