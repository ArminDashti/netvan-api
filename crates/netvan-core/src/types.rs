use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NicInfo {
    pub id: String,
    pub guid: String,
    pub name: String,
    pub description: String,
    pub interface_index: u32,
    pub mac: Option<String>,
    pub mtu: Option<u32>,
    pub media_type: String,
    pub oper_status: String,
    pub admin_status: String,
    pub link_speed_bps: Option<u64>,
    pub ipv4_addresses: Vec<String>,
    pub ipv6_addresses: Vec<String>,
    pub gateways: Vec<String>,
    pub dns_servers: Vec<String>,
    pub dhcp_enabled: bool,
    pub wifi_ssid: Option<String>,
    pub wifi_bssid: Option<String>,
    pub wifi_signal: Option<i32>,
    pub driver: Option<String>,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_bps: f64,
    pub tx_bps: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BandwidthSample {
    pub nic_id: String,
    pub ts: i64,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_bps: f64,
    pub tx_bps: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PingSample {
    pub nic_id: Option<String>,
    pub target: String,
    pub ts: i64,
    pub rtt_ms: Option<f64>,
    pub success: bool,
    pub error: Option<String>,
    #[serde(default)]
    pub raw_output: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpLatencySample {
    pub nic_id: Option<String>,
    pub url: String,
    pub ts: i64,
    pub dns_ms: Option<f64>,
    pub connect_ms: Option<f64>,
    pub tls_ms: Option<f64>,
    pub ttfb_ms: Option<f64>,
    pub total_ms: Option<f64>,
    pub status_code: Option<u16>,
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkEvent {
    pub nic_id: String,
    pub ts: i64,
    pub event: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TracerouteHop {
    pub hop: u8,
    pub address: Option<String>,
    pub hostname: Option<String>,
    pub rtt_ms: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TracerouteResult {
    pub id: i64,
    pub nic_id: Option<String>,
    pub target: String,
    pub ts: i64,
    pub hops: Vec<TracerouteHop>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeedtestResult {
    pub id: i64,
    pub nic_id: Option<String>,
    pub ts: i64,
    pub server_id: Option<String>,
    pub server_name: Option<String>,
    pub download_mbps: f64,
    pub upload_mbps: f64,
    pub ping_ms: f64,
    pub jitter_ms: Option<f64>,
    pub packet_loss: Option<f64>,
    pub raw_json: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppUsageRow {
    pub process_name: String,
    pub process_path: Option<String>,
    pub remote_ip: Option<String>,
    pub host: Option<String>,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub nic_id: Option<String>,
}

/// Hourly usage samples for period grids (Apps / IP / Host).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppUsageSeriesPoint {
    pub hour_ts: i64,
    pub process_name: String,
    pub remote_ip: Option<String>,
    pub host: Option<String>,
    pub bytes_in: u64,
    pub bytes_out: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrafficFlow {
    pub ts: i64,
    pub process_name: String,
    pub process_path: Option<String>,
    pub pid: Option<u32>,
    pub local_addr: String,
    pub remote_addr: String,
    pub protocol: String,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub nic_id: Option<String>,
    pub host: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DnsQueryRecord {
    pub ts: i64,
    pub name: String,
    pub resolved_ips: Vec<String>,
    pub process_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NslookupResult {
    pub query: String,
    pub records: Vec<String>,
    pub raw: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsagePoint {
    pub ts: i64,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceStatus {
    pub running: bool,
    pub pipe_connected: bool,
    pub capture_mode: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiskKind {
    Ssd,
    Hdd,
}

impl DiskKind {
    pub fn as_str(self) -> &'static str {
        match self {
            DiskKind::Ssd => "ssd",
            DiskKind::Hdd => "hdd",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "ssd" => Some(DiskKind::Ssd),
            "hdd" => Some(DiskKind::Hdd),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuSnapshot {
    pub brand: String,
    pub vendor_id: String,
    pub physical_cores: Option<u32>,
    pub logical_cores: u32,
    pub frequency_mhz: Option<u64>,
    pub utilization: f64,
    pub per_core: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySnapshot {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
    pub utilization: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskSnapshot {
    pub id: String,
    pub name: String,
    pub mount_point: String,
    pub file_system: String,
    pub kind: DiskKind,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
    pub utilization: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemMetricPoint {
    pub ts: i64,
    pub value: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricSummary {
    pub avg: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub sample_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemMetricHistory {
    pub series: Vec<SystemMetricPoint>,
    pub summary: MetricSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuInfo {
    pub brand: String,
    pub model: String,
    pub base_speed_mhz: Option<u64>,
    pub physical_cores: Option<u32>,
    pub logical_processors: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryInfo {
    pub brand: String,
    pub model: String,
    pub size_bytes: u64,
    pub speed_mhz: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskInfo {
    pub brand: String,
    pub model: String,
    pub capacity_bytes: u64,
    pub kind: DiskKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuInfo {
    pub brand: String,
    pub model: String,
    pub memory_bytes: Option<u64>,
    pub cuda_cores: Option<u32>,
    pub tensor_cores: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MotherboardInfo {
    pub brand: String,
    pub model: String,
    pub ram_slots: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareInventory {
    pub cpu: CpuInfo,
    pub memory: MemoryInfo,
    pub disks: Vec<DiskInfo>,
    pub gpus: Vec<GpuInfo>,
    pub motherboard: MotherboardInfo,
}
