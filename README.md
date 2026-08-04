# netvan-api

Local Windows service that owns Netvan collectors, SQLite history, and a localhost JSON-RPC + WebSocket API for **netvan-webui**.

Does **not** modify the desktop [Netvan](https://github.com/ArminDashti/netvan) app. Uses a separate data directory so both can coexist.

## Requirements

- Windows
- Rust toolchain (edition 2021)
- Admin rights for `install` / `uninstall`

## Build

```powershell
cargo build -p netvan-api --release
```

Binary: `target\release\netvan-api.exe`

## CLI

```text
netvan-api.exe install
netvan-api.exe uninstall
netvan-api.exe start
netvan-api.exe stop
netvan-api.exe run
netvan-api.exe status
```

| Command | Purpose |
|---------|---------|
| `install` | Register Windows service named **`netvan-api`** (auto-start, restart on failure) |
| `uninstall` | Remove the service |
| `start` / `stop` | Control the service |
| `run` | Foreground mode (debug) |
| `status` | Installed? Running/Stopped? |

SCM **service name:** `netvan-api`  
**Display name:** Netvan API

## Listen address

Default: `http://127.0.0.1:8000` (localhost only)

| Env | Default |
|-----|---------|
| `NETVAN_API_BIND` | `127.0.0.1:8000` |
| `NETVAN_API_DATA_DIR` | `%ProgramData%\Netvan\NetvanApi\` |

SQLite file: `netvan-web.db` under the data dir.

## HTTP API

| Method | Path | Notes |
|--------|------|-------|
| GET | `/api/health` | Liveness |
| GET | `/api/data-dir` | Data directory path |
| POST | `/api/rpc` | Body = `RpcRequest` JSON; response = `RpcResponse` |
| WS | `/api/ws/tools` | Live ping / traceroute / speedtest / IP lookup |

See [docs/endpoints.md](docs/endpoints.md).

### WebSocket client messages (JSON)

```json
{ "type": "ping_live", "target": "1.1.1.1", "count": 4 }
{ "type": "traceroute_live", "target": "1.1.1.1", "max_hops": 30 }
{ "type": "speedtest_live", "nic_id": null, "server_id": null, "accept_eula": true }
{ "type": "cancel_speedtest" }
{ "type": "lookup_ip_info", "ip": "1.1.1.1" }
```

Server streams tagged events (`ping_line`, `traceroute_hop`, `speedtest_progress`, `*_done`, `error`).

## Dev

```powershell
cargo run -p netvan-api -- run
```

Then point **netvan-webui** at `http://127.0.0.1:8000`.
