// The supervisor runs this hook, then sends SIGTERM to the start process group.
console.log('Stopping hello; SQLite closes in its SIGTERM handler.');
