// The supervisor runs this hook, then sends SIGTERM to the start process group.
console.log(`${new Date().toISOString()} INFO hello stop hook: SQLite closes in the SIGTERM handler`);
