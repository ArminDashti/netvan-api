use anyhow::Result;
use chrono::Utc;
use netvan_core::types::NicInfo;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

#[cfg(windows)]
mod win {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};
    use windows::core::PWSTR;
    use windows::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, ERROR_SUCCESS};
    use windows::Win32::NetworkManagement::IpHelper::{
        GetAdaptersAddresses, GetIfEntry2, GAA_FLAG_INCLUDE_GATEWAYS, GAA_FLAG_INCLUDE_PREFIX,
        IF_TYPE_ETHERNET_CSMACD, IF_TYPE_IEEE80211, IF_TYPE_SOFTWARE_LOOPBACK,
        IP_ADAPTER_ADDRESSES_LH, MIB_IF_ROW2, MIB_IF_TABLE_LEVEL,
    };
    use windows::Win32::Networking::WinSock::{
        AF_INET, AF_INET6, AF_UNSPEC, SOCKADDR_IN, SOCKADDR_IN6,
    };

    pub fn list_nics(prev: &Mutex<HashMap<String, (u64, u64, Instant)>>) -> Result<Vec<NicInfo>> {
        unsafe {
            let mut size: u32 = 0;
            let flags = GAA_FLAG_INCLUDE_GATEWAYS | GAA_FLAG_INCLUDE_PREFIX;
            let err = GetAdaptersAddresses(AF_UNSPEC.0 as u32, flags, None, None, &mut size);
            if err != ERROR_BUFFER_OVERFLOW.0 && err != ERROR_SUCCESS.0 {
                anyhow::bail!("GetAdaptersAddresses size query failed: {err}");
            }
            let mut buf = vec![0u8; size as usize];
            let head = buf.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH;
            let err = GetAdaptersAddresses(AF_UNSPEC.0 as u32, flags, None, Some(head), &mut size);
            if err != ERROR_SUCCESS.0 {
                anyhow::bail!("GetAdaptersAddresses failed: {err}");
            }

            let mut out = Vec::new();
            let mut cur = head;
            let mut prev_map = prev.lock();
            while !cur.is_null() {
                let a = &*cur;
                let name = pwstr_to_string(a.FriendlyName);
                let description = pwstr_to_string(a.Description);
                let guid = adapter_name_to_string(a.AdapterName);
                let id = if guid.is_empty() {
                    format!(
                        "if-{}",
                        a.Ipv6IfIndex.max(a.Anonymous1.Anonymous.IfIndex)
                    )
                } else {
                    guid.clone()
                };

                let mac = if a.PhysicalAddressLength > 0 {
                    Some(
                        a.PhysicalAddress[..a.PhysicalAddressLength as usize]
                            .iter()
                            .map(|b| format!("{b:02X}"))
                            .collect::<Vec<_>>()
                            .join(":"),
                    )
                } else {
                    None
                };

                let mut ipv4 = Vec::new();
                let mut ipv6 = Vec::new();
                let mut unicast = a.FirstUnicastAddress;
                while !unicast.is_null() {
                    let u = &*unicast;
                    if let Some(addr) = sockaddr_to_string(u.Address.lpSockaddr) {
                        if addr.contains(':') {
                            ipv6.push(addr);
                        } else {
                            ipv4.push(addr);
                        }
                    }
                    unicast = u.Next;
                }

                let mut gateways = Vec::new();
                let mut gw = a.FirstGatewayAddress;
                while !gw.is_null() {
                    let g = &*gw;
                    if let Some(addr) = sockaddr_to_string(g.Address.lpSockaddr) {
                        gateways.push(addr);
                    }
                    gw = g.Next;
                }

                let mut dns = Vec::new();
                let mut d = a.FirstDnsServerAddress;
                while !d.is_null() {
                    let ds = &*d;
                    if let Some(addr) = sockaddr_to_string(ds.Address.lpSockaddr) {
                        dns.push(addr);
                    }
                    d = ds.Next;
                }

                // IF_TYPE_BLUETOOTH = 248 (not always exported by the windows crate).
                let media_type = match a.IfType {
                    x if x == IF_TYPE_ETHERNET_CSMACD => "Ethernet",
                    x if x == IF_TYPE_IEEE80211 => "Wi-Fi",
                    x if x == IF_TYPE_SOFTWARE_LOOPBACK => "Loopback",
                    131 => "Tunnel",
                    248 => "Bluetooth",
                    _ => "Other",
                }
                .to_string();

                let oper_status = match a.OperStatus.0 {
                    1 => "Up",
                    2 => "Down",
                    3 => "Testing",
                    4 => "Unknown",
                    5 => "Dormant",
                    6 => "NotPresent",
                    7 => "LowerLayerDown",
                    _ => "Unknown",
                }
                .to_string();

                let if_index = if a.Ipv6IfIndex != 0 {
                    a.Ipv6IfIndex
                } else {
                    a.Anonymous1.Anonymous.IfIndex
                };

                let (rx_bytes, tx_bytes, link_speed, admin_status) = if_counters(if_index);
                let now = Instant::now();
                let (rx_bps, tx_bps) = if let Some((prx, ptx, pts)) = prev_map.get(&id) {
                    let dt = now.duration_since(*pts).as_secs_f64().max(0.001);
                    let rx = if rx_bytes >= *prx {
                        (rx_bytes - *prx) as f64 * 8.0 / dt
                    } else {
                        0.0
                    };
                    let tx = if tx_bytes >= *ptx {
                        (tx_bytes - *ptx) as f64 * 8.0 / dt
                    } else {
                        0.0
                    };
                    (rx, tx)
                } else {
                    (0.0, 0.0)
                };
                prev_map.insert(id.clone(), (rx_bytes, tx_bytes, now));

                out.push(NicInfo {
                    id,
                    guid,
                    name,
                    description,
                    interface_index: if_index,
                    mac,
                    mtu: Some(a.Mtu),
                    media_type,
                    oper_status,
                    admin_status,
                    link_speed_bps: link_speed,
                    ipv4_addresses: ipv4,
                    ipv6_addresses: ipv6,
                    gateways,
                    dns_servers: dns,
                    dhcp_enabled: a.Dhcpv4Server.iSockaddrLength > 0
                        || !a.Dhcpv4Server.lpSockaddr.is_null(),
                    wifi_ssid: None,
                    wifi_bssid: None,
                    wifi_signal: None,
                    driver: None,
                    rx_bytes,
                    tx_bytes,
                    rx_bps,
                    tx_bps,
                });

                cur = a.Next;
            }
            Ok(out)
        }
    }

    /// Returns (rx_bytes, tx_bytes, link_speed_bps, admin_status).
    unsafe fn if_counters(index: u32) -> (u64, u64, Option<u64>, String) {
        let mut row = MIB_IF_ROW2 {
            InterfaceIndex: index,
            ..Default::default()
        };
        if GetIfEntry2(&mut row).is_ok() {
            // NET_IF_ADMIN_STATUS: 1=Up, 2=Down, 3=Testing
            let admin_status = match row.AdminStatus.0 {
                1 => "Up".into(),
                2 => "Down".into(),
                3 => "Testing".into(),
                _ => "Unknown".into(),
            };
            (
                row.InOctets,
                row.OutOctets,
                if row.ReceiveLinkSpeed > 0 {
                    Some(row.ReceiveLinkSpeed)
                } else {
                    None
                },
                admin_status,
            )
        } else {
            (0, 0, None, "Unknown".into())
        }
    }

    /// Enable or disable a NIC by friendly name (UAC elevation via RunAs).
    pub fn set_nic_enabled(friendly_name: &str, enabled: bool) -> Result<()> {
        use std::os::windows::process::CommandExt;
        use std::process::Command;

        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let cmd = if enabled {
            "Enable-NetAdapter"
        } else {
            "Disable-NetAdapter"
        };
        // Escape single quotes for PowerShell single-quoted string.
        let escaped = friendly_name.replace('\'', "''");
        let inner = format!("{cmd} -Name '{escaped}' -Confirm:$false");
        let arglist = format!(
            "-NoProfile -ExecutionPolicy Bypass -Command {}",
            powershell_single_quote(&inner)
        );
        let status = Command::new("powershell")
            .args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                &format!(
                    "Start-Process -FilePath powershell -Verb RunAs -Wait -WindowStyle Hidden -ArgumentList {}",
                    powershell_single_quote(&arglist)
                ),
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .status()?;
        if !status.success() {
            anyhow::bail!(
                "Failed to {} adapter '{friendly_name}' (exit {:?}). Administrator approval may be required.",
                if enabled { "enable" } else { "disable" },
                status.code()
            );
        }
        Ok(())
    }

    fn powershell_single_quote(s: &str) -> String {
        format!("'{}'", s.replace('\'', "''"))
    }

    unsafe fn pwstr_to_string(p: PWSTR) -> String {
        if p.is_null() {
            return String::new();
        }
        p.to_string().unwrap_or_default()
    }

    unsafe fn adapter_name_to_string(p: windows::core::PSTR) -> String {
        if p.is_null() {
            return String::new();
        }
        p.to_string().unwrap_or_default()
    }

    unsafe fn sockaddr_to_string(
        sa: *const windows::Win32::Networking::WinSock::SOCKADDR,
    ) -> Option<String> {
        if sa.is_null() {
            return None;
        }
        let family = (*sa).sa_family;
        if family == AF_INET {
            let sin = &*(sa as *const SOCKADDR_IN);
            let octets = u32::from_be(sin.sin_addr.S_un.S_addr).to_be_bytes();
            Some(Ipv4Addr::new(octets[0], octets[1], octets[2], octets[3]).to_string())
        } else if family == AF_INET6 {
            let sin6 = &*(sa as *const SOCKADDR_IN6);
            let bytes = sin6.sin6_addr.u.Byte;
            Some(Ipv6Addr::from(bytes).to_string())
        } else {
            None
        }
    }

    // silence unused import warning for MIB_IF_TABLE_LEVEL
    #[allow(dead_code)]
    const _: MIB_IF_TABLE_LEVEL = MIB_IF_TABLE_LEVEL(0);
}

#[cfg(not(windows))]
mod win {
    use super::*;
    pub fn list_nics(_prev: &Mutex<HashMap<String, (u64, u64, Instant)>>) -> Result<Vec<NicInfo>> {
        Ok(Vec::new())
    }

    pub fn set_nic_enabled(_friendly_name: &str, _enabled: bool) -> Result<()> {
        anyhow::bail!("SetNicEnabled is only supported on Windows")
    }
}

pub struct NicCollector {
    prev: Arc<Mutex<HashMap<String, (u64, u64, Instant)>>>,
}

impl NicCollector {
    pub fn new() -> Self {
        Self {
            prev: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn list(&self) -> Result<Vec<NicInfo>> {
        win::list_nics(&self.prev)
    }

    pub fn find(&self, nic_id: &str) -> Result<Option<NicInfo>> {
        Ok(self.list()?.into_iter().find(|n| n.id == nic_id))
    }

    pub fn set_enabled(&self, nic_id: &str, enabled: bool) -> Result<()> {
        let nic = self
            .find(nic_id)?
            .ok_or_else(|| anyhow::anyhow!("NIC not found: {nic_id}"))?;
        #[cfg(windows)]
        {
            win::set_nic_enabled(&nic.name, enabled)
        }
        #[cfg(not(windows))]
        {
            let _ = (nic, enabled);
            anyhow::bail!("SetNicEnabled is only supported on Windows");
        }
    }

    pub fn sample_bandwidth(nics: &[NicInfo]) -> Vec<netvan_core::types::BandwidthSample> {
        let ts = Utc::now().timestamp();
        nics.iter()
            .filter(|n| n.oper_status == "Up" && n.media_type != "Loopback")
            .map(|n| netvan_core::types::BandwidthSample {
                nic_id: n.id.clone(),
                ts,
                rx_bytes: n.rx_bytes,
                tx_bytes: n.tx_bytes,
                rx_bps: n.rx_bps,
                tx_bps: n.tx_bps,
            })
            .collect()
    }
}

impl Default for NicCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_nics_on_windows() {
        let c = NicCollector::new();
        let nics = c.list().expect("list nics");
        assert!(!nics.is_empty(), "expected at least one adapter");
    }
}
