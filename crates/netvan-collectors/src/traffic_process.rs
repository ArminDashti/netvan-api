//! Mode A — per-process TCP traffic via Windows TCP ESTATS deltas.

use anyhow::Result;
use chrono::Utc;
use netvan_core::types::TrafficFlow;
use std::collections::{HashMap, HashSet};
use sysinfo::{ProcessesToUpdate, System};

#[cfg(windows)]
mod win {
    use super::*;
    use windows::Win32::Foundation::{BOOLEAN, CloseHandle, ERROR_INSUFFICIENT_BUFFER, NO_ERROR};
    use windows::Win32::NetworkManagement::IpHelper::{
        GetExtendedTcpTable, GetPerTcpConnectionEStats, MIB_TCPROW_LH, MIB_TCPROW_LH_0,
        MIB_TCPROW_OWNER_PID, SetPerTcpConnectionEStats, TCP_ESTATS_DATA_ROD_v0,
        TCP_ESTATS_DATA_RW_v0, TCP_TABLE_OWNER_PID_ALL, TcpConnectionEstatsData,
    };
    use windows::Win32::Networking::WinSock::AF_INET;
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    use windows::core::PWSTR;

    #[repr(C)]
    struct TcpTableOwnerPid {
        dw_num_entries: u32,
        table: [MIB_TCPROW_OWNER_PID; 1],
    }

    /// Absolute ESTATS counters for one TCP row (DataBytesIn / DataBytesOut).
    pub struct ConnSample {
        pub pid: u32,
        pub local: String,
        pub remote: String,
        pub process_name: String,
        pub process_path: Option<String>,
        pub bytes_in: u64,
        pub bytes_out: u64,
    }

    pub fn sample_connections(sys: &mut System) -> Result<Vec<ConnSample>> {
        sys.refresh_processes(ProcessesToUpdate::All, true);
        let mut out = Vec::new();

        unsafe {
            let mut size: u32 = 0;
            let _ = GetExtendedTcpTable(
                None,
                &mut size,
                false,
                AF_INET.0 as u32,
                TCP_TABLE_OWNER_PID_ALL,
                0,
            );
            if size == 0 {
                return Ok(out);
            }
            let mut buf = vec![0u8; size as usize];
            let status = GetExtendedTcpTable(
                Some(buf.as_mut_ptr() as *mut _),
                &mut size,
                false,
                AF_INET.0 as u32,
                TCP_TABLE_OWNER_PID_ALL,
                0,
            );
            if status != NO_ERROR.0 && status != ERROR_INSUFFICIENT_BUFFER.0 {
                // still try to parse if buffer filled
            }
            if size as usize > buf.len() {
                return Ok(out);
            }
            let table = &*(buf.as_ptr() as *const TcpTableOwnerPid);
            let count = table.dw_num_entries as usize;
            let base = buf.as_ptr().add(4) as *const MIB_TCPROW_OWNER_PID;
            for i in 0..count {
                let row = &*base.add(i);
                let pid = row.dwOwningPid;
                if pid == 0 {
                    continue;
                }
                let local = format_ipv4_port(row.dwLocalAddr, row.dwLocalPort);
                let remote = format_ipv4_port(row.dwRemoteAddr, row.dwRemotePort);
                if remote.starts_with("0.0.0.0") || remote.starts_with("127.") {
                    continue;
                }

                let tcp_row = MIB_TCPROW_LH {
                    Anonymous: MIB_TCPROW_LH_0 {
                        dwState: row.dwState,
                    },
                    dwLocalAddr: row.dwLocalAddr,
                    dwLocalPort: row.dwLocalPort,
                    dwRemoteAddr: row.dwRemoteAddr,
                    dwRemotePort: row.dwRemotePort,
                };

                let Some((bytes_in, bytes_out)) = read_estats_bytes(&tcp_row) else {
                    continue;
                };

                let (name, path) = process_info(sys, pid);
                out.push(ConnSample {
                    pid,
                    local,
                    remote,
                    process_name: name,
                    process_path: path,
                    bytes_in,
                    bytes_out,
                });
            }
        }
        Ok(out)
    }

    unsafe fn read_estats_bytes(row: &MIB_TCPROW_LH) -> Option<(u64, u64)> {
        let rw = TCP_ESTATS_DATA_RW_v0 {
            EnableCollection: BOOLEAN(1),
        };
        let rw_bytes = std::slice::from_raw_parts(
            (&rw as *const TCP_ESTATS_DATA_RW_v0) as *const u8,
            std::mem::size_of::<TCP_ESTATS_DATA_RW_v0>(),
        );
        // Enable collection (idempotent). Ignore failures — Get may still work if already enabled.
        let _ = SetPerTcpConnectionEStats(row, TcpConnectionEstatsData, rw_bytes, 0, 0);

        let mut rod = TCP_ESTATS_DATA_ROD_v0::default();
        let rod_bytes = std::slice::from_raw_parts_mut(
            (&mut rod as *mut TCP_ESTATS_DATA_ROD_v0) as *mut u8,
            std::mem::size_of::<TCP_ESTATS_DATA_ROD_v0>(),
        );
        let status = GetPerTcpConnectionEStats(
            row,
            TcpConnectionEstatsData,
            None,
            0,
            None,
            0,
            Some(rod_bytes),
            0,
        );
        if status != NO_ERROR.0 {
            return None;
        }
        Some((rod.DataBytesIn, rod.DataBytesOut))
    }

    fn format_ipv4_port(addr: u32, port: u32) -> String {
        let a = u32::from_be(addr).to_be_bytes();
        let p = u16::from_be((port & 0xFFFF) as u16);
        format!("{}.{}.{}.{}:{}", a[0], a[1], a[2], a[3], p)
    }

    fn process_info(sys: &System, pid: u32) -> (String, Option<String>) {
        use sysinfo::Pid;
        if let Some(p) = sys.process(Pid::from_u32(pid)) {
            let name = p.name().to_string_lossy().to_string();
            let path = p.exe().map(|p| p.to_string_lossy().to_string());
            return (name, path);
        }
        unsafe {
            if let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                let mut buf = [0u16; 260];
                let mut size = buf.len() as u32;
                let ok = windows::Win32::System::Threading::QueryFullProcessImageNameW(
                    handle,
                    windows::Win32::System::Threading::PROCESS_NAME_FORMAT(0),
                    PWSTR(buf.as_mut_ptr()),
                    &mut size,
                );
                let _ = CloseHandle(handle);
                if ok.is_ok() {
                    let path = String::from_utf16_lossy(&buf[..size as usize]);
                    let name = std::path::Path::new(&path)
                        .file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| format!("pid-{pid}"));
                    return (name, Some(path));
                }
            }
        }
        (format!("pid-{pid}"), None)
    }
}

#[cfg(not(windows))]
mod win {
    use super::*;
    pub struct ConnSample {
        pub pid: u32,
        pub local: String,
        pub remote: String,
        pub process_name: String,
        pub process_path: Option<String>,
        pub bytes_in: u64,
        pub bytes_out: u64,
    }
    pub fn sample_connections(_sys: &mut System) -> Result<Vec<ConnSample>> {
        Ok(Vec::new())
    }
}

type ConnKey = (u32, String, String);

pub struct ProcessTrafficCollector {
    sys: System,
    /// Last absolute ESTATS counters per connection.
    last: HashMap<ConnKey, (u64, u64)>,
}

impl ProcessTrafficCollector {
    pub fn new() -> Self {
        Self {
            sys: System::new(),
            last: HashMap::new(),
        }
    }

    pub fn sample(&mut self) -> Result<Vec<TrafficFlow>> {
        let ts = Utc::now().timestamp();
        let samples = win::sample_connections(&mut self.sys)?;
        let mut seen: HashSet<ConnKey> = HashSet::new();
        let mut by_key: HashMap<(String, String), TrafficFlow> = HashMap::new();

        for s in samples {
            let key: ConnKey = (s.pid, s.local.clone(), s.remote.clone());
            seen.insert(key.clone());

            let (din, dout) = if let Some(&(pin, pout)) = self.last.get(&key) {
                let din = s.bytes_in.saturating_sub(pin);
                let dout = s.bytes_out.saturating_sub(pout);
                // Reject implausible per-sample deltas (poisoned/garbage ESTATS).
                const MAX_DELTA: u64 = 1_073_741_824; // 1 GiB per connection per sample
                let din = if din > MAX_DELTA { 0 } else { din };
                let dout = if dout > MAX_DELTA { 0 } else { dout };
                (din, dout)
            } else {
                // First sighting — establish baseline, no delta yet.
                (0, 0)
            };
            self.last.insert(key, (s.bytes_in, s.bytes_out));

            if din == 0 && dout == 0 {
                continue;
            }

            let flow_key = (s.process_name.clone(), s.remote.clone());
            by_key
                .entry(flow_key)
                .and_modify(|e| {
                    e.bytes_in = e.bytes_in.saturating_add(din);
                    e.bytes_out = e.bytes_out.saturating_add(dout);
                })
                .or_insert(TrafficFlow {
                    ts,
                    process_name: s.process_name,
                    process_path: s.process_path,
                    pid: Some(s.pid),
                    local_addr: s.local,
                    remote_addr: s.remote,
                    protocol: "TCP".into(),
                    bytes_in: din,
                    bytes_out: dout,
                    nic_id: None,
                    host: None,
                });
        }

        // Drop closed connections from baseline map.
        self.last.retain(|k, _| seen.contains(k));

        Ok(by_key.into_values().collect())
    }
}

impl Default for ProcessTrafficCollector {
    fn default() -> Self {
        Self::new()
    }
}
