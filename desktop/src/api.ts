// The daemon API as this page sees it: the record shapes it reads and the one native command that
// carries every request.
//
// Requests go through the native plugin instead of fetch(): the daemon's bearer token stays in Rust
// and never reaches JavaScript (docs/SECURITY.md), and the bridge in native/src/lib.rs forwards only
// GET and POST calls under /v1/ from the packaged UI's origin.
import { invoke } from '@tauri-apps/api/core';

// The fields the page reads; the full shapes are in src/model.rs.
export type App = {
  spec: {
    name: string;
    repository: string;
    git_ref: string;
    memory_mib: number;
    cpus: number;
    mounts: string;
    ports: { host: number; guest: number }[];
  };
  active_release: string | null;
  previous_release: string | null;
  desired_running: boolean;
  last_error: string | null;
};
export type Job = {
  id: string;
  app: string;
  state: string;
  action: { action: string };
  created_at?: number;
  error: string | null;
  result: unknown;
};
export type Release = { id: string; commit: string; created_at: number };
export type Backup = { id: string; created_at: number; bytes: number };

// VERIFIED: desktop/defuss-tauri.json grants this command to the loopback UI origin, and
// tests/ui.test.mjs checks that every command the page invokes is granted.
const NATIVE_COMMAND = 'plugin:defuss-container|daemon_request';

export function api<T>(method: 'GET' | 'POST', path: string, body?: unknown): Promise<T> {
  return invoke<T>(NATIVE_COMMAND, { method, path, body: body ?? null });
}

// The daemon accepts one queued or running job per application (a partial unique index,
// docs/API.md), so the UI disables what would queue a second one instead of letting it fail.
export const active = (j: Job) => j.state === 'queued' || j.state === 'running';
