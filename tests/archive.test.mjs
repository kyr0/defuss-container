// Tests for guest/archive.mjs on the host: Node runs the same module the builder and supervisor run
// in Bun. The refusal cases (escaping links, traversal, duplicates, tampered headers, size limits)
// are the guarantees the host relies on when it unpacks a builder's output or a backup.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createTar, extractTar } from '../guest/archive.mjs';
function temp(t) {
  const p = fs.mkdtempSync(path.join(os.tmpdir(), 'defuss-archive-'));
  t.after(() => fs.rmSync(p, { recursive: true, force: true }));
  return p;
}
test('tar round-trip handles binary files, nested Unicode, long names and executable permissions', (t) => {
  const p = temp(t),
    src = path.join(p, 'src');
  fs.mkdirSync(src);
  const dir = '日本語-' + 'a'.repeat(110);
  fs.mkdirSync(path.join(src, dir));
  const data = Buffer.from(Array.from({ length: 120000 }, (_, i) => i % 256));
  fs.writeFileSync(path.join(src, dir, 'payload'), data, { mode: 0o755 });
  createTar(src, path.join(p, 'a.tar'));
  extractTar(path.join(p, 'a.tar'), path.join(p, 'out'));
  assert.deepEqual(fs.readFileSync(path.join(p, 'out', dir, 'payload')), data);
  if (process.platform !== 'win32')
    assert.equal(fs.statSync(path.join(p, 'out', dir, 'payload')).mode & 0o777, 0o755);
});
test('tar empty directory survives', (t) => {
  const p = temp(t);
  fs.mkdirSync(path.join(p, 'src/empty'), { recursive: true });
  createTar(path.join(p, 'src'), path.join(p, 'a.tar'));
  extractTar(path.join(p, 'a.tar'), path.join(p, 'out'));
  assert.ok(fs.statSync(path.join(p, 'out/empty')).isDirectory());
});
test('tampered tar header rejected', (t) => {
  const p = temp(t);
  fs.mkdirSync(path.join(p, 'src'));
  fs.writeFileSync(path.join(p, 'src/a'), 'data');
  createTar(path.join(p, 'src'), path.join(p, 'a.tar'));
  const b = fs.readFileSync(path.join(p, 'a.tar'));
  b[0] ^= 1;
  fs.writeFileSync(path.join(p, 'a.tar'), b);
  assert.throws(() => extractTar(path.join(p, 'a.tar'), path.join(p, 'out')), /checksum/);
});
test('archive expansion limit enforced', (t) => {
  const p = temp(t);
  fs.mkdirSync(path.join(p, 'src'));
  fs.writeFileSync(path.join(p, 'src/a'), 'data');
  createTar(path.join(p, 'src'), path.join(p, 'a.tar'));
  assert.throws(
    () => extractTar(path.join(p, 'a.tar'), path.join(p, 'out'), { maxBytes: 3 }),
    /size limit/,
  );
});
test('exclusions omit secrets and data', (t) => {
  const p = temp(t);
  fs.mkdirSync(path.join(p, 'src'));
  fs.writeFileSync(path.join(p, 'src/.env'), 'secret');
  fs.writeFileSync(path.join(p, 'src/package.json'), '{}');
  createTar(path.join(p, 'src'), path.join(p, 'a.tar'), { exclude: (n) => n === '.env' });
  extractTar(path.join(p, 'a.tar'), path.join(p, 'out'));
  assert.ok(!fs.existsSync(path.join(p, 'out/.env')));
  assert.ok(fs.existsSync(path.join(p, 'out/package.json')));
});
test('relative dependency symlink roundtrip', { skip: process.platform === 'win32' }, (t) => {
  const p = temp(t);
  fs.mkdirSync(path.join(p, 'src/node_modules/.bin'), { recursive: true });
  fs.mkdirSync(path.join(p, 'src/node_modules/pkg'));
  fs.writeFileSync(path.join(p, 'src/node_modules/pkg/cli'), 'x');
  fs.symlinkSync('../pkg/cli', path.join(p, 'src/node_modules/.bin/cli'));
  createTar(path.join(p, 'src'), path.join(p, 'a.tar'));
  extractTar(path.join(p, 'a.tar'), path.join(p, 'out'));
  assert.equal(fs.readFileSync(path.join(p, 'out/node_modules/.bin/cli'), 'utf8'), 'x');
});
test('escaping symlink refused by packer', { skip: process.platform === 'win32' }, (t) => {
  const p = temp(t);
  fs.mkdirSync(path.join(p, 'src'));
  fs.symlinkSync('../outside', path.join(p, 'src/link'));
  assert.throws(() => createTar(path.join(p, 'src'), path.join(p, 'a.tar')), /escaping link/);
});
test('existing destination symlink refused', { skip: process.platform === 'win32' }, (t) => {
  const p = temp(t);
  fs.mkdirSync(path.join(p, 'src/dir'), { recursive: true });
  fs.writeFileSync(path.join(p, 'src/dir/file'), 'x');
  createTar(path.join(p, 'src'), path.join(p, 'a.tar'));
  fs.mkdirSync(path.join(p, 'out'));
  fs.mkdirSync(path.join(p, 'outside'));
  fs.symlinkSync('../outside', path.join(p, 'out/dir'));
  assert.throws(
    () => extractTar(path.join(p, 'a.tar'), path.join(p, 'out')),
    /symlink|destination/,
  );
  assert.ok(!fs.existsSync(path.join(p, 'outside/file')));
});
test('truncated tar rejected', (t) => {
  const p = temp(t);
  fs.writeFileSync(path.join(p, 'bad.tar'), Buffer.alloc(100));
  assert.throws(() => extractTar(path.join(p, 'bad.tar'), path.join(p, 'out')), /truncated/);
});
function maliciousTar(name, type = '0', link = '') {
  const b = Buffer.alloc(512);
  b.write(name, 0, 100);
  b.write('0000644\0', 100, 8);
  b.write('0000000\0', 108, 8);
  b.write('0000000\0', 116, 8);
  b.write('00000000000\0', 124, 12);
  b.write('00000000000\0', 136, 12);
  b.fill(32, 148, 156);
  b.write(type, 156, 1);
  b.write(link, 157, 100);
  b.write('ustar\0', 257, 6);
  b.write('00', 263, 2);
  const sum = b.reduce((s, v) => s + v, 0);
  b.write(sum.toString(8).padStart(6, '0') + '\0 ', 148, 8);
  return b;
}
test('archive traversal and absolute paths rejected before writes', (t) => {
  const p = temp(t);
  for (const [i, name] of [
    '../escape',
    '/etc/escape',
    'a/../../escape',
    'a\\..\\escape',
  ].entries()) {
    const file = path.join(p, `${i}.tar`);
    fs.writeFileSync(file, Buffer.concat([maliciousTar(name), Buffer.alloc(1024)]));
    assert.throws(() => extractTar(file, path.join(p, `out${i}`)), /unsafe archive/);
  }
});
test('duplicate archive entry rejected', (t) => {
  const p = temp(t);
  const f = path.join(p, 'dup.tar');
  fs.writeFileSync(f, Buffer.concat([maliciousTar('x'), maliciousTar('x'), Buffer.alloc(1024)]));
  assert.throws(() => extractTar(f, path.join(p, 'out')), /duplicate/);
});
test('absolute symlink from an external tar refused', (t) => {
  const p = temp(t);
  const f = path.join(p, 'link.tar');
  fs.writeFileSync(f, Buffer.concat([maliciousTar('x', '2', '/etc/passwd'), Buffer.alloc(1024)]));
  assert.throws(() => extractTar(f, path.join(p, 'out')), /unsafe link/);
});
