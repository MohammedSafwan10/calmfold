#!/usr/bin/env python3
"""Real Windows portable installations: SDK upgrade/relaunch and authenticated rejection."""
import argparse
import hashlib
import http.server
import importlib.util
import json
import os
from pathlib import Path
import shutil
import ssl
import subprocess
import tempfile
import threading
import time
import zipfile

ROOT = Path(__file__).resolve().parent.parent
spec = importlib.util.spec_from_file_location("fixture_signing", ROOT / "scripts/update-test-signing.py")
signing = importlib.util.module_from_spec(spec)
spec.loader.exec_module(signing)

def wait_for(predicate, label, timeout=90):
    deadline = time.monotonic() + timeout
    while not predicate():
        if time.monotonic() >= deadline: raise AssertionError(f"Timed out: {label}")
        time.sleep(0.1)

class Handler(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *_): pass

def smoke(old_dir, new_dir, tamper):
    with tempfile.TemporaryDirectory(prefix="calmfold-velopack-test-") as tmp:
        root = Path(tmp)
        web = root / "web"; web.mkdir()
        portable = next(old_dir.glob("*Portable.zip"))
        install = root / "installation"; install.mkdir()
        with zipfile.ZipFile(portable) as archive: archive.extractall(install)
        executable = install / "current/Calmfold.exe"
        if not executable.exists():
            raise AssertionError("Velopack portable package is missing its real app executable")
        full = next(new_dir.glob("*-full.nupkg"))
        shutil.copy2(full, web / full.name)
        subprocess.run(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", str(root / "tls.key"), "-out", str(root / "tls.crt"), "-days", "1", "-subj", "/CN=localhost", "-addext", "subjectAltName=IP:127.0.0.1"], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), lambda *a, **kw: Handler(*a, directory=str(web), **kw))
        context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER); context.load_cert_chain(root / "tls.crt", root / "tls.key")
        server.socket = context.wrap_socket(server.socket, server_side=True)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        base = f"https://127.0.0.1:{server.server_port}"
        public, sign = signing.create(root)
        size = full.stat().st_size
        with full.open("rb") as file: digest = hashlib.file_digest(file, "sha256").hexdigest()
        feed = web / "stable.json"
        feed.write_text(json.dumps({"version": "1.0.1", "notes": ["Fixture upgrade"], "platforms": {"windows-x64": {"url": base + "/" + full.name, "sha256": digest, "size": size}}}))
        sign(feed)
        if tamper == "catalog": feed.write_text(feed.read_text().replace("Fixture upgrade", "Tampered catalog"))
        if tamper == "package":
            with (web / full.name).open("r+b") as file:
                file.seek(100); byte = file.read(1); file.seek(100); file.write(bytes([byte[0] ^ 1]))
        (root / "feed-url").write_text(base + "/stable.json")
        (root / "feed-key").write_text(public)
        (root / "userdata.txt").write_text("private fixture data stays outside installation")
        env = {**os.environ, "CALMFOLD_NATIVE_TEST_ROOT": str(root)}
        app = subprocess.Popen([str(executable), "--test-root", str(root)], env=env)
        try:
            wait_for(lambda: (root / "started.log").exists(), "old installed app startup")
            assert "1.0.0 " in (root / "started.log").read_text()
            second = subprocess.run([str(executable), "--test-root", str(root)], env=env, timeout=15, capture_output=True)
            assert second.returncode != 0, "second instance bypassed OS lock"
            (root / "check").touch()
            if tamper:
                wait_for(lambda: (root / "rejected").exists(), "tamper rejection")
                assert app.poll() is None and not (root / "ready").exists()
                print(f"PASS: {tamper} tampering rejected; old Windows app remains running", flush=True)
            else:
                wait_for(lambda: (root / "ready").exists(), "verified package ready")
                assert app.poll() is None, "download stopped the running app"
                assert (install / "current/sibling.txt").read_text() == "old sibling"
                (root / "install").touch()
                wait_for(lambda: "1.0.1 " in (root / "started.log").read_text(), "new Windows version relaunched")
                assert (install / "current/sibling.txt").read_text() == "new sibling"
                print("PASS: real Velopack install upgraded 1.0.0 -> 1.0.1, replaced siblings and relaunched", flush=True)
            assert (root / "userdata.txt").read_text() == "private fixture data stays outside installation"
        finally:
            (root / "stop").touch()
            if app.poll() is None:
                try: app.wait(timeout=5)
                except subprocess.TimeoutExpired: app.kill(); app.wait()
            time.sleep(0.5)
            if (root / "started.log").exists():
                for line in (root / "started.log").read_text().splitlines():
                    try: subprocess.run(["taskkill", "/PID", line.split()[1], "/T", "/F"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                    except (IndexError, OSError): pass
            server.shutdown(); server.server_close()

if __name__ == "__main__":
    if os.name != "nt": raise SystemExit("Run installed Velopack tests on Windows")
    parser = argparse.ArgumentParser(); parser.add_argument("old", type=Path); parser.add_argument("new", type=Path); args = parser.parse_args()
    for case in [None, "catalog", "package"]: smoke(args.old, args.new, case)
