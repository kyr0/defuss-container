"""HTTPS smart-HTTP Git server for the e2e suite: `git http-backend` behind a TLS listener on 127.0.0.1.

WHY a local server instead of a hosted repository: the daemon only accepts HTTPS Git URLs and fetches with
`--depth=1`, which the dumb HTTP protocol cannot serve. A self-signed loopback server keeps the e2e hermetic
(no account, no push) while exercising the same fetch path as a real remote.
"""
from __future__ import annotations

import http.server
import os
import pathlib
import shutil
import ssl
import subprocess
import threading


def find_openssl() -> str:
    found = shutil.which("openssl")
    if found:
        return found
    # Git for Windows ships OpenSSL beside its other Unix tools.
    for candidate in (r"C:\Program Files\Git\usr\bin\openssl.exe", r"C:\Program Files\OpenSSL\bin\openssl.exe"):
        if os.path.exists(candidate):
            return candidate
    raise RuntimeError("openssl not found: needed to create the e2e Git server certificate")


def make_certificate(directory: pathlib.Path) -> tuple[pathlib.Path, pathlib.Path]:
    cert, key = directory / "cert.pem", directory / "key.pem"
    subprocess.run(
        [find_openssl(), "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "2", "-subj", "/CN=127.0.0.1",
         "-addext", "subjectAltName=IP:127.0.0.1", "-keyout", str(key), "-out", str(cert)],
        check=True, capture_output=True,
    )
    return cert, key


class _Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    server: "GitServer"

    def do_GET(self) -> None:
        self._backend()

    def do_POST(self) -> None:
        self._backend()

    def _body(self) -> bytes:
        if self.headers.get("Transfer-Encoding", "").lower() == "chunked":
            data = b""
            while True:
                size = int(self.rfile.readline().strip().split(b";")[0], 16)
                if size == 0:
                    self.rfile.readline()
                    return data
                data += self.rfile.read(size)
                self.rfile.readline()
        length = int(self.headers.get("Content-Length") or 0)
        return self.rfile.read(length) if length else b""

    def _backend(self) -> None:
        path, _, query = self.path.partition("?")
        body = self._body()
        env = {
            **os.environ, "GIT_PROJECT_ROOT": str(self.server.root), "GIT_HTTP_EXPORT_ALL": "1",
            "PATH_INFO": path, "QUERY_STRING": query, "REQUEST_METHOD": self.command,
            "CONTENT_TYPE": self.headers.get("Content-Type", ""), "CONTENT_LENGTH": str(len(body)),
            "REMOTE_ADDR": "127.0.0.1", "GIT_PROTOCOL": self.headers.get("Git-Protocol", ""),
            "HTTP_CONTENT_ENCODING": self.headers.get("Content-Encoding", ""),
        }
        result = subprocess.run(["git", "http-backend"], input=body, env=env, capture_output=True)
        out = result.stdout
        sep = b"\r\n\r\n" if b"\r\n\r\n" in out else b"\n\n"
        head, _, payload = out.partition(sep)
        status, headers = 200, []
        for line in head.decode("latin-1").splitlines():
            name, _, value = line.partition(":")
            if name.lower() == "status":
                status = int(value.strip().split()[0])
            elif name:
                headers.append((name.strip(), value.strip()))
        if result.returncode != 0 and not out:
            status, payload = 500, result.stderr
        self.send_response(status)
        for name, value in headers:
            if name.lower() != "content-length":
                self.send_header(name, value)
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def log_message(self, *args: object) -> None:  # requests are visible in the daemon's job logs instead
        pass


class GitServer(http.server.ThreadingHTTPServer):
    """Serves every bare repository under `root` at https://127.0.0.1:<port>/<name>.git."""

    daemon_threads = True

    def __init__(self, root: pathlib.Path, certs: pathlib.Path) -> None:
        super().__init__(("127.0.0.1", 0), _Handler)
        self.root = root
        self.cert, key = make_certificate(certs)
        context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        context.load_cert_chain(self.cert, key)
        self.socket = context.wrap_socket(self.socket, server_side=True)
        self.thread = threading.Thread(target=self.serve_forever, daemon=True)

    def url(self, name: str) -> str:
        return f"https://127.0.0.1:{self.server_address[1]}/{name}.git"

    def __enter__(self) -> "GitServer":
        self.thread.start()
        return self

    def __exit__(self, *exc: object) -> None:
        self.shutdown()
        self.server_close()
