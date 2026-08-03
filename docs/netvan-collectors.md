# netvan-collectors

Windows collectors orchestrated by `CollectorEngine`:

- NIC enumeration + bandwidth + link events (media types: Ethernet, Wi-Fi, Bluetooth/`248`, Tunnel, Loopback, Other)
- ICMP ping (per-NIC bind via `ping -S`) — still runs as background work inside the app
- HTTP latency phases (DNS/TCP/TLS/TTFB/total) for URL targets via `reqwest` (in-process)
- Traceroute / nslookup
- Ookla Speedtest CLI wrapper
- Process traffic (Mode A) via TCP table + PID + **TCP ESTATS** (`SetPerTcpConnectionEStats` / `GetPerTcpConnectionEStats`) — emits per-sample **deltas** of `DataBytesIn` / `DataBytesOut` (UDP uncounted; soft-fail per row)
- Full mode (Mode B) DNS enrichment + WinDivert probe
- **System metrics** (`system_metrics`): CPU / memory / disk via `sysinfo` — live refresh (~2s) for snapshots; persist tick (~5s) writes raw samples + hourly avg/min/max rollups. Disks classified as SSD/HDD (`sysinfo::DiskKind`); removable/unknown skipped.

Child console tools (`ping`, `tracert`, `nslookup`, `ipconfig`, `where`, speedtest CLI) are spawned with Windows `CREATE_NO_WINDOW` via `win_cmd::hide_console` so they do not flash terminal windows while the UI stays a GUI app.

Background loops write samples into `netvan-core::Database`.
