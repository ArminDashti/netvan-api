## Learned User Preferences

- Prefer Cursor multi-root workspace files in the dedicated `cursor-workspaces` GitHub repo, not beside individual project folders.
- When running the local netvan stack, enable hot reload on both backend and frontend.
- Prefer heavy-cache PWA as app-shell only: precache UI assets for offline load; do not cache API/RPC responses.

## Learned Workspace Facts

- `netvan-api` and `netvan-webui` are a split web stack (Windows service API + browser PWA), inspired by but not the same as the monolithic Tauri desktop `netvan` repo.
- The multi-root workspace file is `netvan.code-workspace` in `C:\Users\armin\GitHub\cursor-workspaces` (GitHub: `ArminDashti/cursor-workspaces`).
- Local defaults: API at `127.0.0.1:8000`, Vite web UI at `127.0.0.1:8001`; API data under `%ProgramData%\Netvan\NetvanApi\`.
- `netvan-api` `.cargo/config.toml` prefers `windows-gnu`; if only MSVC is installed, run with `--target x86_64-pc-windows-msvc`.
- `netvan-webui` uses `vite-plugin-pwa` with custom `src/sw.ts`; app-shell caching keeps cross-origin localhost API/WebSocket traffic NetworkOnly.

## Cursor Cloud specific instructions

This repo is a Windows-targeted Rust workspace (`netvan-api` service + `netvan-core` + `netvan-collectors`), but it compiles and runs on the Linux cloud VM thanks to `cfg(not(windows))` fallbacks. Standard commands live in `README.md`; the notes below are the non-obvious Linux-VM caveats.

- Toolchain: the base image ships an old Rust that fails on `edition2024` deps (e.g. `clap_lex`). The update script runs `rustup update stable` / `rustup default stable`; if a build complains about `edition2024`, re-run those.
- Target override (important): `.cargo/config.toml` forces `target = "x86_64-pc-windows-gnu"`, which is NOT installed here. Add `--target x86_64-unknown-linux-gnu` to every cargo command (`build`, `run`, `test`, `clippy`) to build/run natively on Linux. Do not edit `.cargo/config.toml`.
- Run the dev server: `NETVAN_API_DATA_DIR=/tmp/netvan-api-data cargo run -p netvan-api --target x86_64-unknown-linux-gnu -- run`. Setting `NETVAN_API_DATA_DIR` is required on Linux — otherwise the data dir resolves to a literal `C:\ProgramData\...` path created under the cwd. Default bind is `127.0.0.1:8000` (`NETVAN_API_BIND` to change). Use the `run` subcommand only; `install/start/stop/status` shell out to Windows `sc` and are Windows-only.
- Smoke-test with endpoints that work cross-platform: `GET /api/health`, `GET /api/data-dir`, and RPC (`POST /api/rpc`) `Ping`, `RunNslookup`, `GetCpuSnapshot`, `GetMemorySnapshot`, and the `*History` methods (SQLite-backed, populated by the background collectors).
- Known Linux limitations (expected, not bugs): NIC enumeration (`ListNics`) returns empty, `SetNicEnabled` and hardware inventory are stubs, and `RunPing`/`RunTraceroute` shell out to Windows `ping`/`tracert` syntax so they misbehave on Linux. The unit test `netvan-collectors::nic::tests::list_nics_on_windows` fails on Linux by design — run tests with `-- --skip list_nics_on_windows`.
