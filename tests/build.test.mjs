// Tests for guest/build.mjs and guest/policy.mjs without a VM: the builder runs under the host's
// Bun with explicit paths, on a fixture app with a real dependency, so the frozen install, build
// script and exclusions are real.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { createTar, extractTar } from '../guest/archive.mjs';
import { artifactExcluded, sourceExcluded, packageScript } from '../guest/policy.mjs';
const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const bun = process.env.BUN_EXECUTABLE || 'bun';
test('source policy excludes host dependencies; artifact policy retains installed dependencies', () => {
  assert.equal(sourceExcluded('node_modules/zod/index.js'), true);
  assert.equal(artifactExcluded('node_modules/zod/index.js'), false);
  for (const name of [
    '.env',
    '.npmrc',
    '.git/config',
    'var/counter.sqlite',
    'tmp/file',
    'input/foo',
    'output/foo',
    'etc/config',
  ])
    assert.equal(artifactExcluded(name), true);
  assert.equal(artifactExcluded('.env.example'), false);
});
test('actual Bun builder artifact includes a loadable production dependency and generated build output', (t) => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'defuss-build-test-'));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  const source = path.join(dir, 'source');
  fs.cpSync(path.join(repo, 'examples/hello'), source, {
    recursive: true,
    filter: (p) => !p.split(path.sep).includes('node_modules'),
  });
  const marker = path.join(dir, 'unexpected-install-script');
  const pkg = JSON.parse(fs.readFileSync(path.join(source, 'package.json'), 'utf8'));
  pkg.scripts.postinstall = `bun -e ${JSON.stringify(`require('fs').writeFileSync(${JSON.stringify(marker)},'bad')`)}`;
  pkg.scripts.build = 'bun generate.mjs';
  fs.writeFileSync(path.join(source, 'package.json'), JSON.stringify(pkg));
  fs.writeFileSync(
    path.join(source, 'generate.mjs'),
    "import fs from 'node:fs';fs.mkdirSync('dist',{recursive:true});fs.writeFileSync('dist/built.txt','built');\n",
  );
  fs.writeFileSync(path.join(source, '.env'), 'SECRET=should-not-ship');
  fs.mkdirSync(path.join(source, 'var'));
  fs.writeFileSync(path.join(source, 'var/secret'), 'data');
  const archive = path.join(dir, 'source.tar');
  createTar(source, archive);
  const release = crypto.randomUUID();
  const artifact = path.join(dir, 'app.tar');
  const built = spawnSync(
    bun,
    [
      path.join(repo, 'guest/build.mjs'),
      release,
      'true',
      path.join(dir, 'work'),
      archive,
      artifact,
    ],
    { encoding: 'utf8', timeout: 120000 },
  );
  assert.equal(built.status, 0, built.stdout + '\n' + built.stderr);
  const out = path.join(dir, 'out');
  extractTar(artifact, out);
  const app = path.join(out, 'opt/defuss/releases', release);
  assert.equal(fs.readFileSync(path.join(app, 'dist/built.txt'), 'utf8'), 'built');
  assert.ok(fs.existsSync(path.join(app, 'node_modules/zod/package.json')));
  assert.ok(!fs.existsSync(path.join(app, '.env')));
  assert.ok(!fs.existsSync(path.join(app, 'var')));
  assert.ok(!fs.existsSync(marker));
  const check = spawnSync(
    bun,
    ['-e', "const {z}=require('zod');console.log(z.string().parse('dependency-ok'))"],
    { cwd: app, encoding: 'utf8', timeout: 10000 },
  );
  assert.equal(check.status, 0, check.stderr);
  assert.equal(check.stdout.trim(), 'dependency-ok');
});
// Guest code runs only in the Linux VM; POSIX PATH and shebang semantics make Windows hosts
// irrelevant here.
test(
  'package scripts run through Bun Shell with node_modules/.bin first, propagating exit codes',
  { skip: process.platform === 'win32' },
  (t) => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'defuss-script-test-'));
    t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
    fs.mkdirSync(path.join(dir, 'node_modules/.bin'), { recursive: true });
    const pkg = { scripts: { start: 'tool && bun -e "process.exit(7)"' } };
    fs.writeFileSync(
      path.join(dir, 'node_modules/.bin/tool'),
      '#!/usr/bin/env bun\nconsole.log("tool-from-bin")\n',
      { mode: 0o755 },
    );
    const s = packageScript(dir, pkg, 'start', { PATH: process.env.PATH });
    const r = spawnSync(s.cmd[0] === process.execPath ? bun : s.cmd[0], s.cmd.slice(1), {
      cwd: dir,
      env: s.env,
      encoding: 'utf8',
      timeout: 30000,
    });
    assert.equal(r.stdout.trim(), 'tool-from-bin', r.stderr);
    assert.equal(r.status, 7);
    assert.throws(() => packageScript(dir, pkg, 'stop', {}), /scripts\.stop required/);
  },
);
