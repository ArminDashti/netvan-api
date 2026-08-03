# netvan-core

Shared library: domain types, SQLite schema/migrations-on-open, settings, history range helpers, and length-prefixed JSON RPC types used by both the Tauri app and `netvan-service`.

Key modules: `db`, `ipc`, `history`, `settings`, `paths`, `types`.

## System metrics storage

Dual-retention design for CPU / memory / disk utilization:

- `disks` inventory (kind `ssd`/`hdd`, capacity, mount)
- Compact raw samples (`cpu_samples`, `memory_samples`, `disk_samples`) with `ts` / `(disk_id, ts)` primary keys, ~3-day retention
- Hourly rollups (`*_stats_hourly`) storing **avg / min / max / sample_count**, ~90-day retention
- History RPC returns `{ series, summary }` and picks raw vs hourly by range (Today/Yesterday → raw with optional minute downsample; longer → hourly)
