# Local HTTP API v1

Connection discovery: `<DEFUSS_CONTAINER_HOME>/connection.json` contains `address` (HTTP 127.0.0.1 with the selected port) and `token`. All routes require `Authorization: Bearer TOKEN` and the exact endpoint `Host` header. Any browser `Origin` is rejected. Credentials rotate on daemon restart; reconnect clients after a 401. No CORS or unauthenticated health endpoint. Request body limit: 1 MiB. JSON responses are `Cache-Control: no-store`.

| Method / route | Result |
|---|---|
| GET `/v1/health` | Software/API version |
| GET `/v1/doctor` | Engine version, provider, hypervisor prerequisites |
| GET `/v1/apps` | Application records (desired state) |
| POST `/v1/apps` | Register AppSpec; 201 |
| GET `/v1/apps/{name}` | Application record and observed engine state/error |
| GET `/v1/apps/{name}/releases` | Successful build records |
| GET `/v1/apps/{name}/backups` | Backup metadata, checksums and local paths |
| GET `/v1/apps/{name}/logs` | Last 200 workload log lines, text/plain |
| POST `/v1/apps/{name}/actions` | Queue action; 202 and Job |
| GET `/v1/jobs` | Most recent 200 jobs |
| GET `/v1/jobs/{id}` | Durable job record |
| GET `/v1/jobs/{id}/log` | Bounded command log, text/plain |

Actions:

```json
{"action":"build"}
{"action":"deploy","release":"UUID"}
{"action":"start"}
{"action":"stop"}
{"action":"rollback"}
{"action":"backup"}
{"action":"restore","backup":"UUID"}
{"action":"delete","confirm":"APPLICATION_NAME"}
```

Job states: `queued`, `running`, `succeeded`, `failed`, `interrupted`. `result` is populated on success, `error` on failure. The daemon permits two concurrent workers globally and one outstanding operation per application (SQLite partial unique index). A client disconnect does not cancel a job. Restart marks outstanding jobs interrupted and reconciles registered deployed apps to persisted desired running/stopped state. Failed first deployments require inspection; they have no prior release to restore.

Standard errors have `{"error":{"message":"..."}}`; 401 authentication, 403 origin/Host rejection, 404 unknown record, 409 conflicting app/job, 400 validation/runtime precondition. Axum's malformed-JSON/schema rejection can be 422 with its own text body. Native client and CLI surface status/body instead of assuming every error is JSON.

App specs are strongly typed and reject unknown fields. No arbitrary shell/exec, arbitrary image, arbitrary provider, remote daemon bind, credential export, or generic filesystem endpoint is exposed.
