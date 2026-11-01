// scripts.stop. VERIFIED: the supervisor runs this hook, then sends SIGTERM to the start process
// group (guest/supervisor.mjs stop). This app needs no work here, because server.ts closes its
// database in the SIGTERM handler.
console.log(
  `${new Date().toISOString()} INFO hello stop hook: SQLite closes in the SIGTERM handler`,
);
