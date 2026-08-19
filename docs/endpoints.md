# netvan-api HTTP / WebSocket

Localhost API for **netvan-webui**. Collectors and tools run only inside the `netvan-api` Windows service.

Default bind: `127.0.0.1:8000`

## Endpoints

| Method | Path | Notes |
|--------|------|-------|
| GET | `/api/health` | `{ "ok": true, "service": "netvan-api" }` |
| GET | `/api/data-dir` | `{ "path": "..." }` |
| POST | `/api/rpc` | Body = `RpcRequest`; response = `RpcResponse` (same shapes as desktop Netvan) |
| WS | `/api/ws/tools` | Live tools streams |

## RPC methods (`POST /api/rpc`)

| Method | Params | Response |
|--------|--------|----------|
| `Ping` | — | `Pong` |
| `GetStatus` | — | `Status` |
| `GetSettings` | — | `Settings` |
| `SetSettings` | `{ settings }` | `Ok` |
| `SetCaptureMode` | `{ mode }` | `Ok` |
| `ListNics` | — | `Nics` |
| `GetNic` | `{ nic_id }` | `Nic` |
| `SetNicEnabled` | `{ nic_id, enabled }` | `Ok` |
| `GetBandwidthHistory` | `{ nic_id?, range, start_ts?, end_ts? }` | `BandwidthHistory` |
| `GetPingHistory` | same | `PingHistory` |
| `GetHttpLatencyHistory` | same | `HttpLatencyHistory` |
| `GetLinkEvents` | same | `LinkEvents` |
| `GetAppUsage` | `{ range, start_ts?, end_ts?, group_by, nic_id? }` | `AppUsage` |
| `GetAppUsageSeries` | same | `AppUsageSeries` |
| `RunPing` | `{ target, nic_id?, count?, packet_size? }` | `PingResult` |
| `RunHttpLatency` | `{ url, nic_id? }` | `HttpLatencyResult` |
| `RunTraceroute` | `{ target, nic_id?, max_hops? }` | `Traceroute` |
| `RunNslookup` | `{ query }` | `Nslookup` |
| `RunSpeedtest` | `{ nic_id?, server_id?, accept_eula }` | `Speedtest` |
| `GetSpeedtestHistory` | `{ range, start_ts?, end_ts? }` | `SpeedtestHistory` |
| `AcceptSpeedtestEula` | — | `Ok` |
| `GetCpuSnapshot` | — | `CpuSnapshot` |
| `GetMemorySnapshot` | — | `MemorySnapshot` |
| `GetDisks` | `{ kind: ssd\|hdd }` | `Disks` |
| `GetHardwareInventory` | — | `HardwareInventory` |
| `GetThermalSnapshot` | — | `ThermalSnapshot` |
| `GetCpuHistory` | `{ range, start_ts?, end_ts? }` | `CpuHistory` |
| `GetMemoryHistory` | same | `MemoryHistory` |
| `GetDiskHistory` | `{ disk_id?, kind, range, start_ts?, end_ts? }` | `DiskHistory` |

History `range`: `today` | `yesterday` | `week` | `months` | `all` | `custom`.

`GetThermalSnapshot` returns `{ sensors: [{ id, hardware_kind, hardware_name, sensor_name, celsius }] }` where `hardware_kind` is `cpu` | `gpu` | `motherboard` | `storage` | `memory` | `other`. Live only; `celsius` may be null.

## WebSocket `/api/ws/tools`

Client → server:

```json
{ "type": "ping_live", "target": "1.1.1.1", "count": 4, "packet_size": null }
{ "type": "traceroute_live", "target": "1.1.1.1", "max_hops": 30 }
{ "type": "speedtest_live", "nic_id": null, "server_id": null, "accept_eula": true }
{ "type": "cancel_speedtest" }
{ "type": "lookup_ip_info", "ip": "8.8.8.8" }
```

Server → client (tagged `type`): `ping_line`, `ping_done`, `traceroute_hop`, `traceroute_done`, `speedtest_progress`, `speedtest_done`, `ip_info`, `error`.

## Data directory

Default: `%ProgramData%\Netvan\NetvanApi\netvan-web.db`  
Override: `NETVAN_API_DATA_DIR`
