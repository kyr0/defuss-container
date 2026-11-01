// Runs inside the disposable builder VM (service.rs build): /source holds the archived commit,
// /helper these programs (both read-only), and /export/app.tar is the only output the host accepts.
// Steps: unpack, check the package contract, frozen install without lifecycle scripts, optional
// scripts.build, production reinstall, pack.
// Arguments: <release id> <run build script: true or false>.
import fs from 'node:fs';
import path from 'node:path';
import { extractTar, createTar } from './archive.mjs';
// VERIFIED: the builder image has no /bin/sh; scripts.build runs through packageScript (`bun
// exec`).
import { sourceExcluded, artifactExcluded, packageScript } from './policy.mjs';
const [release, doBuild] = process.argv.slice(2);
if (!/^[0-9a-f-]{36}$/.test(release)) throw new Error('invalid release id');
// Optional paths let tests/build.test.mjs run the builder on the host without a VM.
const root = process.argv[4] || '/tmp/defuss-build';
const source = process.argv[5] || '/source/source.tar';
const output = process.argv[6] || '/export/app.tar';
fs.mkdirSync(root, { recursive: true });
extractTar(source, root, { exclude: sourceExcluded });
const pkg = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8'));
// Fail before the slow install: every deploy needs scripts.start and scripts.stop.
for (const k of ['start', 'stop']) packageScript(root, pkg, k, process.env);
if (!fs.existsSync(path.join(root, 'bun.lock')) && !fs.existsSync(path.join(root, 'bun.lockb')))
  // --frozen-lockfile installs exactly what the commit locked, so a release is reproducible from
  // its commit.
  throw new Error('commit bun.lock or bun.lockb before building');
function spawn(cmd, env, label) {
  const p = Bun.spawnSync(cmd, {
    cwd: root,
    stdout: 'inherit',
    stderr: 'inherit',
    env: { ...env, BUN_CONFIG_NO_CLEAR_TERMINAL: '1' },
  });
  if (p.exitCode !== 0) throw new Error(`${label} exited ${p.exitCode}`);
}
const run = (args) => spawn([process.execPath, ...args], process.env, `bun ${args[0]}`);
// --ignore-scripts: dependency lifecycle scripts (postinstall and the like) do not run (ARCH.md,
// Build pipeline).
run(['install', '--frozen-lockfile', '--ignore-scripts']);
if (doBuild === 'true') {
  const s = packageScript(root, pkg, 'build', process.env);
  spawn(s.cmd, s.env, 'scripts.build');
}
// Prune development dependencies by reinstalling from the committed lockfile.
fs.rmSync(path.join(root, 'node_modules'), { recursive: true, force: true });
run(['install', '--production', '--frozen-lockfile', '--ignore-scripts']);
// The host accepts only entries under this release's prefix (service.rs validate_app_layer).
createTar(root, output, { prefix: `opt/defuss/releases/${release}/`, exclude: artifactExcluded });
console.log(JSON.stringify({ release, bytes: fs.statSync(output).size }));
