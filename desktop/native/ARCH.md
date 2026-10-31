# Native bridge architecture

`src/lib.rs` adds one plugin, `defuss-container`, with one command to the defuss-tauri host.

- **Origin pinning.** The plugin allows navigation only to `http://127.0.0.1` and remembers the first origin it sees, which is the packaged UI's asset server. `daemon_request` compares the calling window's origin with it and refuses everything else. A page that navigates elsewhere therefore cannot drive the daemon.
- **Same client as the CLI.** Each call runs `defuss_container::client::Client::connect(util::home())` and `Client::request` on a blocking thread. Connection discovery, bearer token handling, path validation and error text match the CLI exactly; `make e2e` exercises this client from the GUI through `defuss-container api`.
- **No secrets in JavaScript.** The token is read from `connection.json` per call and never returned. A daemon restart rotates it; the next call reads the new one.
- **No lifecycle ownership.** The bridge never starts or stops the daemon or the engine. Closing the window has no effect on running jobs or VMs.

Why a plugin instead of an app command: the UI's loopback origin is remote to Tauri, and only a plugin command has a permission (`allow-daemon-request`) that a remote capability can grant ([../ARCH.md](../ARCH.md)).

Why per-call connection instead of a cached client: the daemon rotates its token on every start, and re-reading `connection.json` makes reconnecting after a restart automatic without extra state.
