#!/usr/bin/env python3
"""tauri-cli refuses to build when @tauri-apps/api and the tauri crate differ in major.minor (VERIFIED 2026-10-31:
`make gui` failed with tauri 2.12.1 next to @tauri-apps/api 2.9.0). Keep the two pins on the same release line."""
import json, pathlib, re, sys
root = pathlib.Path(__file__).resolve().parents[2]
js = json.loads((root / 'desktop/package.json').read_text())['dependencies']['@tauri-apps/api']
m = re.search(r'^tauri\s*=\s*\{[^}]*version\s*=\s*"=?([0-9.]+)"', (root / 'desktop/native/Cargo.toml').read_text(), re.M)
if not m:
    sys.exit('desktop/native/Cargo.toml: no tauri version pin found')
minor = lambda v: v.lstrip('^~=').split('.')[:2]
if minor(js) != minor(m.group(1)):
    sys.exit(f'@tauri-apps/api {js} (desktop/package.json) and tauri {m.group(1)} (desktop/native/Cargo.toml) differ in '
             f'major.minor; pin @tauri-apps/api to {m.group(1)} with `cd desktop && bun add --exact @tauri-apps/api@{m.group(1)}`')
print(f'tauri {m.group(1)} and @tauri-apps/api {js} share a release line')
