use serde::{Deserialize, Serialize};

use crate::history::HistoryRange;
use crate::settings::{AppSettings, CaptureMode};
use crate::types::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum RpcRequest {
    Ping,
    GetStatus,
    GetSettings,
    SetSettings { settings: AppSettings },
    SetCaptureMode { mode: CaptureMode },
    ListNics,
    GetNic { nic_id: String },
    SetNicEnabled { nic_id: String, enabled: bool },
    GetBandwidthHistory {
        nic_id: Option<String>,
        range: HistoryRange,
        start_ts: Option<i64>,
        end_ts: Option<i64>,
    },
    GetPingHistory {
        nic_id: Option<String>,
        range: HistoryRange,
        start_ts: Option<i64>,
        end_ts: Option<i64>,
    },
    GetHttpLatencyHistory {
        nic_id: Option<String>,
        range: HistoryRange,
        start_ts: Option<i64>,
        end_ts: Option<i64>,
    },
    GetLinkEvents {
        nic_id: Option<String>,
        range: HistoryRange,
        start_ts: Option<i64>,
        end_ts: Option<i64>,
    },
    GetAppUsage {
        range: HistoryRange,
        start_ts: Option<i64>,
        end_ts: Option<i64>,
        group_by: String,
        #[serde(default)]
        nic_id: Option<String>,
    },
    GetAppUsageSeries {
        range: HistoryRange,
        start_ts: Option<i64>,
        end_ts: Option<i64>,
        group_by: String,
        #[serde(default)]
        nic_id: Option<String>,
    },
    RunPing {
        target: String,
        nic_id: Option<String>,
        count: Option<u32>,
        packet_size: Option<u32>,
    },
    RunHttpLatency {
        url: String,
        nic_id: Option<String>,
    },
    RunTraceroute {
        target: String,
        nic_id: Option<String>,
        max_hops: Option<u8>,
    },
    RunNslookup { query: String },
    RunSpeedtest {
        nic_id: Option<String>,
        server_id: Option<String>,
        accept_eula: bool,
    },
    GetSpeedtestHistory {
        range: HistoryRange,
        start_ts: Option<i64>,
        end_ts: Option<i64>,
    },
    AcceptSpeedtestEula,
    GetCpuSnapshot,
    GetMemorySnapshot,
    GetDisks { kind: DiskKind },
    GetHardwareInventory,
    GetThermalSnapshot,
    GetCpuHistory {
        range: HistoryRange,
        start_ts: Option<i64>,
        end_ts: Option<i64>,
    },
    GetMemoryHistory {
        range: HistoryRange,
        start_ts: Option<i64>,
        end_ts: Option<i64>,
    },
    GetDiskHistory {
        disk_id: Option<String>,
        kind: DiskKind,
        range: HistoryRange,
        start_ts: Option<i64>,
        end_ts: Option<i64>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum RpcResponse {
    Ok,
    Pong,
    Status(ServiceStatus),
    Settings(AppSettings),
    Nics(Vec<NicInfo>),
    Nic(NicInfo),
    BandwidthHistory(Vec<UsagePoint>),
    PingHistory(Vec<PingSample>),
    HttpLatencyHistory(Vec<HttpLatencySample>),
    LinkEvents(Vec<LinkEvent>),
    AppUsage(Vec<AppUsageRow>),
    AppUsageSeries(Vec<AppUsageSeriesPoint>),
    PingResult(PingSample),
    HttpLatencyResult(HttpLatencySample),
    Traceroute(TracerouteResult),
    Nslookup(NslookupResult),
    Speedtest(SpeedtestResult),
    SpeedtestHistory(Vec<SpeedtestResult>),
    CpuSnapshot(CpuSnapshot),
    MemorySnapshot(MemorySnapshot),
    Disks(Vec<DiskSnapshot>),
    HardwareInventory(HardwareInventory),
    ThermalSnapshot(ThermalSnapshot),
    CpuHistory(SystemMetricHistory),
    MemoryHistory(SystemMetricHistory),
    DiskHistory(SystemMetricHistory),
    Error { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcEnvelope {
    pub id: u64,
    pub request: RpcRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcReply {
    pub id: u64,
    pub response: RpcResponse,
}

/// Length-prefixed JSON framing helpers for named pipe IPC.
pub fn encode_message(value: &impl Serialize) -> anyhow::Result<Vec<u8>> {
    let json = serde_json::to_vec(value)?;
    let mut out = Vec::with_capacity(4 + json.len());
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(&json);
    Ok(out)
}

pub fn decode_frame(buf: &[u8]) -> Option<(usize, &[u8])> {
    if buf.len() < 4 {
        return None;
    }
    let len = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    if buf.len() < 4 + len {
        return None;
    }
    Some((4 + len, &buf[4..4 + len]))
}
