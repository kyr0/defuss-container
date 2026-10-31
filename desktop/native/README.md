# defuss-container-desktop-native

Rust extension that defuss-tauri links into the desktop app (listed under `rust.extensions` in `../defuss-tauri.json`). It registers the Tauri plugin `defuss-container` with one command for the packaged UI and nothing else.

## Command

```ts
import { invoke } from '@tauri-apps/api/core';
const apps = await invoke('plugin:defuss-container|daemon_request', { method: 'GET', path: '/v1/apps', body: null });
```

| Argument | Rule |
|---|---|
| `method` | `GET` or `POST` |
| `path` | starts with `/v1/`; no `?`, `#`, `\` or `..` segments |
| `body` | JSON value or `null` |

It resolves with the daemon's JSON response, or with a string for text endpoints (logs). It rejects with a message when the daemon is unreachable, answers with an error status, or the calling window's origin is not the packaged UI. Routes and status codes: [docs/API.md](../../docs/API.md).

The capability in `../defuss-tauri.json` grants `defuss-container:allow-daemon-request` to `http://127.0.0.1:*` in the `main` window; `build.rs` generates that permission into `permissions/` (gitignored).

The state directory comes from `DEFUSS_CONTAINER_HOME`, else the OS default, as for the CLI. The app inherits the environment of whatever launches it.

## Build

```sh
make native      # cargo check of this crate
make gui         # full app and bundle for the current OS
make start-gui   # open it against the `make start` daemon
```
