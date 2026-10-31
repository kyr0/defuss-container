import { z } from 'zod';
import { Database } from 'bun:sqlite';
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';

const log = (level: string, message: string) => console.log(`${new Date().toISOString()} ${level} hello ${message}`);
const read = (path: string) => (existsSync(path) ? readFileSync(path, 'utf8') : null);
// VERIFIED: the supervisor empties tmp before every start (e2e), so a marker left by the previous run is gone.
const tmpEmptyAtStart = readdirSync('./tmp').length === 0;
writeFileSync('./tmp/marker', String(Date.now()));

mkdirSync('./var', { recursive: true });
const db = new Database('./var/counter.sqlite', { create: true });
db.exec('PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; CREATE TABLE IF NOT EXISTS counter(id INTEGER PRIMARY KEY, value INTEGER NOT NULL); INSERT OR IGNORE INTO counter VALUES(1,0)');

function inputWritable() {
  try {
    writeFileSync('./input/.write-probe', 'x');
    return true;
  } catch {
    return false;
  }
}

const server = Bun.serve({
  hostname: '0.0.0.0',
  port: z.coerce.number().int().min(1).max(65535).parse(process.env.PORT || 3000),
  async fetch(request) {
    const url = new URL(request.url);
    if (url.pathname === '/health') return Response.json({ ok: true, runtime: Bun.version });
    if (url.pathname === '/version') return new Response((read('./version.txt') ?? 'dev').trim());
    if (url.pathname === '/counter' && request.method === 'POST') {
      db.query('UPDATE counter SET value=value+1 WHERE id=1').run();
      return Response.json(db.query('SELECT value FROM counter WHERE id=1').get());
    }
    if (url.pathname === '/counter') return Response.json(db.query('SELECT value FROM counter WHERE id=1').get());
    if (url.pathname === '/host') {
      const port = Number(process.env.HOST_TEST_PORT || 8123);
      return new Response(await (await fetch(`http://host:${port}/`)).text());
    }
    if (url.pathname === '/output') {
      writeFileSync('./output/hello.txt', 'hello from Bun\n');
      return new Response('written');
    }
    if (url.pathname === '/shares') {
      return Response.json({
        input: read('./input/hello.txt'),
        etc: read('./etc/config.txt'),
        input_writable: inputWritable(),
        tmp_empty_at_start: tmpEmptyAtStart,
        app: process.env.DEFUSS_APP,
        node_env: process.env.NODE_ENV,
      });
    }
    return new Response('defuss-container example\n');
  },
});
log('INFO', `listening address=${server.hostname}:${server.port}`);

function stop() {
  log('INFO', 'stopping');
  server.stop(true);
  db.close();
  process.exit(0);
}
process.on('SIGTERM', stop);
process.on('SIGINT', stop);
