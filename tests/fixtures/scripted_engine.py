#!/usr/bin/env python3
"""Scripted stand-in for the smolvm CLI, used by the lifecycle tests in src/service.rs.

The test writes this file next to its state and passes it to Service as the engine executable. Each call is
appended to `commands` (one JSON argv per line), and machine state (exists, running) lives in `state.json`.
Files a test creates beside it script engine behavior:

- crash-release: the workload of this release exits at once, so `--stop-on-exit` powers the machine off.
- fail-release: the supervisor probe reports this release as exited.
- exec-not-ready: after `start --no-workload`, this many execs fail before one succeeds.
- broken-boots: this many provision-only boots never accept exec until the machine stops.
- fail-backup, fail-restore: the supervisor's backup or restore command fails.
"""

import json
import pathlib
import sys

root = pathlib.Path(__file__).parent
args = sys.argv[1:]
with (root / "commands").open("a") as f:
    f.write(json.dumps(args) + "\n")
state = root / "state.json"
s = json.loads(state.read_text()) if state.exists() else {"exists": False, "running": False}
verb = args[1]
if verb == "ls":
    if s["exists"]:
        print("defuss-test")
elif verb == "status":
    print(json.dumps({"state": "Running" if s["running"] else "Stopped"}))
elif verb == "create":
    s["exists"] = True
elif verb == "start":
    launch = json.loads((root.parent / "state/apps/hello/control/launch.json").read_text())
    crash = (root / "crash-release").read_text() if (root / "crash-release").exists() else ""
    # A crashing workload with --stop-on-exit powers the machine off right after start.
    s["running"] = "--no-workload" in args or launch["release"] != crash
    if "--no-workload" in args and (root / "exec-not-ready").exists():
        (root / "exec-pending").write_text((root / "exec-not-ready").read_text())
    # A broken provision-only boot: no exec succeeds until the machine stops (observed in smolvm
    # 1.32.4).
    broken = int((root / "broken-boots").read_text()) if (root / "broken-boots").exists() else 0
    if "--no-workload" in args and broken:
        (root / "broken-boots").write_text(str(broken - 1))
        (root / "exec-pending").write_text("1000000")
elif verb == "stop":
    s["running"] = False
    if (root / "exec-pending").exists():
        (root / "exec-pending").unlink()
elif verb == "delete":
    s["exists"] = False
elif verb == "logs":
    print("fixture logs")
elif verb == "exec":
    if not s["running"]:
        sys.stderr.write("machine is not running\n")
        sys.exit(1)
    pending = int((root / "exec-pending").read_text()) if (root / "exec-pending").exists() else 0
    if pending:
        (root / "exec-pending").write_text(str(pending - 1))
        sys.stderr.write("executable file `/usr/local/bin/bun` not found in $PATH\n")
        sys.exit(255)
    if args[-1] == "--version":
        print("1.4.4")
        state.write_text(json.dumps(s))
        sys.exit(0)
    launch = json.loads((root.parent / "state/apps/hello/control/launch.json").read_text())
    command = args[args.index("--") + 1 :]
    if command[-1] == "probe":
        fail = (root / "fail-release").read_text() if (root / "fail-release").exists() else ""
        print(
            json.dumps(
                {
                    "state": "exited" if launch["release"] == fail else "running",
                    "release": launch["release"],
                }
            )
        )
    elif command[2] == "backup":
        if (root / "fail-backup").exists():
            sys.exit(8)
        (root.parent / "state/apps/hello/backups" / (command[3] + ".tar")).write_bytes(bytes(1024))
    elif command[2] == "restore" and (root / "fail-restore").exists():
        sys.exit(9)
state.write_text(json.dumps(s))
