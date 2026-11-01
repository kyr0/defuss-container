// The app VM's entrypoint (oci.rs sets it) and the daemon's maintenance tool inside the VM.
//
// Without arguments it boots the workload: verify and unpack the release that launch.json selects,
// link the data directories, map `host`, run scripts.start and report its state. With `backup`,
// `restore` or `probe` the daemon runs it through `machine exec`: backup and restore on a
// provision-only boot with the workload stopped, probe next to the running workload.
import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { extractTar, createTar } from './archive.mjs';
// VERIFIED: the distroless image has no /bin/sh; package scripts run through packageScript (`bun
// exec`).
import { packageScript } from './policy.mjs';
// Read-only share with launch.json, written by the daemon (service.rs write_launch).
const control = '/defuss-control';
// One line per event: ISO-8601 UTC timestamp, level, component, message, key=value fields.
const log = (level, message, fields = {}) =>
  console.log(
    [
      new Date().toISOString(),
      level,
      'defuss-supervisor',
      message,
      ...Object.entries(fields).map(([k, v]) => `${k}=${v}`),
    ].join(' '),
  );
const hash = (p) => {
  const h = createHash('sha256');
  const f = fs.openSync(p, 'r');
  try {
    const b = Buffer.alloc(1024 * 1024);
    let n;
    while ((n = fs.readSync(f, b, 0, b.length, null)) > 0) h.update(b.subarray(0, n));
  } finally {
    fs.closeSync(f);
  }
  return h.digest('hex');
};
const argv = process.argv.slice(2);
if (argv[0] === 'backup') {
  if (!/^[0-9a-f-]{36}$/.test(argv[1])) throw new Error('invalid backup');
  // `.defuss-*` entries are the supervisor's own (the restore staging directory), not app data.
  createTar('/app/var', `/defuss-backups/${argv[1]}.tar`, {
    exclude: (n) => n.split('/')[0].startsWith('.defuss-'),
  });
  process.exit(0);
}
if (argv[0] === 'restore') {
  if (!/^[0-9a-f-]{36}$/.test(argv[1]) || !/^[0-9a-f]{64}$/.test(argv[2]))
    throw new Error('invalid backup');
  const src = `/defuss-backups/${argv[1]}.tar`;
  if (hash(src) !== argv[2]) throw new Error('backup checksum mismatch');
  // Staging inside /app/var keeps it on the same filesystem, so moving the entries into place is a
  // rename.
  const next = '/app/var/.defuss-restore-new';
  fs.rmSync(next, { recursive: true, force: true });
  extractTar(src, next);
  // The daemon creates a complete pre-restore backup first. App stays stopped.
  for (const entry of fs.readdirSync('/app/var'))
    if (entry !== '.defuss-restore-new')
      fs.rmSync(`/app/var/${entry}`, { recursive: true, force: true });
  for (const entry of fs.readdirSync(next))
    fs.renameSync(path.join(next, entry), path.join('/app/var', entry));
  fs.rmdirSync(next);
  process.exit(0);
}
// The running supervisor keeps its state in this file; the daemon's readiness check and Start read
// it.
if (argv[0] === 'probe') {
  console.log(fs.readFileSync('/run/defuss-status.json', 'utf8'));
  process.exit(0);
}
const cfg = JSON.parse(fs.readFileSync(`${control}/launch.json`, 'utf8'));
if (!/^[0-9a-f-]{36}$/.test(cfg.release) || !/^[0-9a-f]{64}$/.test(cfg.sha256))
  throw new Error('invalid launch configuration');
const root = `/opt/defuss/releases/${cfg.release}`;
// .defuss-ready holds the checksum of the layer unpacked at `root`; a missing or different marker
// means the release is unpacked again from the checksummed layer in the read-only release share.
if (
  !fs.existsSync(`${root}/.defuss-ready`) ||
  fs.readFileSync(`${root}/.defuss-ready`, 'utf8') !== cfg.sha256
) {
  const src = `/defuss-releases/${cfg.release}/app.tar`;
  if (hash(src) !== cfg.sha256) throw new Error('release checksum mismatch');
  // Always re-extract into an empty staging tree, including first boot from OCI.
  const staging = `/opt/defuss/staging-${cfg.release}`;
  fs.rmSync(staging, { recursive: true, force: true });
  extractTar(src, staging);
  const prepared = `${staging}/opt/defuss/releases/${cfg.release}`;
  fs.rmSync(root, { recursive: true, force: true });
  fs.mkdirSync(path.dirname(root), { recursive: true });
  fs.renameSync(prepared, root);
  fs.rmSync(staging, { recursive: true, force: true });
  fs.writeFileSync(`${root}/.defuss-ready`, cfg.sha256);
}
// App code finds its data at ./var, ./tmp, ./input, ./output and ./etc in the release (README.md,
// Application contract). Only /app/var persists; it lives on the machine disk and survives deploys.
for (const dir of ['var', 'tmp', 'input', 'output', 'etc']) {
  fs.mkdirSync(`/app/${dir}`, { recursive: true });
  const target = `${root}/${dir}`;
  if (
    fs.existsSync(target) ||
    (() => {
      try {
        return fs.lstatSync(target).isSymbolicLink();
      } catch {
        return false;
      }
    })()
  )
    fs.rmSync(target, { recursive: true, force: true });
  fs.symlinkSync(`/app/${dir}`, target);
}
// Persist data only in /app/var; scratch is emptied on every workload start.
for (const entry of fs.readdirSync('/app/tmp'))
  fs.rmSync(`/app/tmp/${entry}`, { recursive: true, force: true });
// Pinned virtio-net subnet maps its gateway TCP connections to the physical host. Any earlier
// `host` alias is dropped first, so the name always resolves to the gateway.
const hosts = fs
  .readFileSync('/etc/hosts', 'utf8')
  .split('\n')
  .map((l) => {
    const [body, comment] = l.split('#', 2);
    const parts = body.trim().split(/\s+/);
    if (parts.length > 1) {
      const rest = parts.slice(1).filter((x) => x !== 'host');
      return rest.length ? `${parts[0]} ${rest.join(' ')}${comment ? ' #' + comment : ''}` : '';
    }
    return l;
  })
  .join('\n');
fs.writeFileSync('/etc/hosts', hosts + '\n100.96.0.1 host\n');
// The DEFUSS_* variables come after the app's env, so the spec cannot redefine them (model.rs
// refuses them).
const env = {
  ...process.env,
  ...cfg.env,
  DEFUSS_APP: cfg.app,
  DEFUSS_VAR: '/app/var',
  DEFUSS_TMP: '/app/tmp',
  DEFUSS_INPUT: '/app/input',
  DEFUSS_OUTPUT: '/app/output',
  DEFUSS_ETC: '/app/etc',
  DEFUSS_HOST: 'host',
  TMPDIR: '/app/tmp',
  NODE_ENV: cfg.env?.NODE_ENV || 'production',
};
const pkg = JSON.parse(fs.readFileSync(`${root}/package.json`, 'utf8'));
const status = (state, extra = {}) =>
  fs.writeFileSync(
    '/run/defuss-status.json',
    JSON.stringify({ state, release: cfg.release, ...extra }),
  );
let stopping = false;
let child;
function signalGroup(signal) {
  try {
    process.kill(-child.pid, signal);
  } catch {
    try {
      child.kill(signal);
    } catch {}
  }
}
// Stop sequence (ARCH.md, Runtime): scripts.stop with half the stop timeout, then SIGTERM to the
// start script's process group, then SIGKILL after the other half. The engine's stop timeout is 5 s
// longer (provider.rs).
async function stop() {
  if (stopping) return;
  stopping = true;
  status('stopping');
  log('INFO', 'stop hook started');
  const s = packageScript(root, pkg, 'stop', env);
  const hook = Bun.spawn(s.cmd, {
    cwd: root,
    env: s.env,
    stdout: 'inherit',
    stderr: 'inherit',
    detached: true,
  });
  const timer = setTimeout(
    () => {
      try {
        process.kill(-hook.pid, 'SIGKILL');
      } catch {}
    },
    Math.max(1000, cfg.stop_timeout_seconds * 500),
  );
  await hook.exited;
  clearTimeout(timer);
  signalGroup('SIGTERM');
  const kill = setTimeout(
    () => signalGroup('SIGKILL'),
    Math.max(1000, cfg.stop_timeout_seconds * 500),
  );
  await child.exited;
  clearTimeout(kill);
  status('stopped');
  process.exit(0);
}
process.on('SIGTERM', stop);
process.on('SIGINT', stop);
// detached: the start script leads its own process group, so stop() can signal everything it
// started.
const start = packageScript(root, pkg, 'start', env);
child = Bun.spawn(start.cmd, {
  cwd: root,
  env: start.env,
  stdout: 'inherit',
  stderr: 'inherit',
  detached: true,
});
status('running', { pid: child.pid });
log('INFO', 'started', { release: cfg.release, pid: child.pid });
const code = await child.exited;
// A start script that exits on its own takes the supervisor with it; with --stop-on-exit the VM
// powers off, which readiness detects at once.
if (!stopping) {
  status('exited', { exit_code: code });
  log('WARN', 'start script exited', { release: cfg.release, code });
  process.exit(code);
}
