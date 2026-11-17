#!/usr/bin/env python3
"""Real disposable macOS apps: signed Sparkle install/relaunch and tamper rejection.
The installed app uses the pre-rename Calmfold names and the update the Ottero
names with the same bundle identifier, so a pass also proves the rename path
(ADR 0009).
No Keychain access, real app data, user signing identity or public test releases.
"""
import base64
import hashlib
import http.server
import json
import os
from pathlib import Path
import plistlib
import queue
import shutil
import signal
import subprocess
import tempfile
import threading
import time
import uuid

ROOT = Path(__file__).resolve().parent.parent
SDK = Path(os.environ.get("OTTERO_SPARKLE_DIR", ROOT / "target/tools/sparkle")).resolve()


def run(*args, **kwargs):
    return subprocess.run([str(a) for a in args], check=True, capture_output=True, **kwargs).stdout


def keypair(directory):
    seed = os.urandom(32)
    private = directory / "sparkle.key"
    private.write_bytes(base64.b64encode(seed))
    private.chmod(0o600)
    der = directory / "seed.der"
    der.write_bytes(bytes.fromhex("302e020100300506032b657004220420") + seed)
    der.chmod(0o600)
    public = run("openssl", "pkey", "-inform", "DER", "-in", der, "-pubout", "-outform", "DER")
    assert public[:12] == bytes.fromhex("302a300506032b6570032100")
    return private, base64.b64encode(public[12:]).decode()


class QuietHandler(http.server.SimpleHTTPRequestHandler):
    def log_message(self, fmt, *args):
        print("fixture HTTP: " + fmt % args, flush=True)


def smoke(tamper):
    with tempfile.TemporaryDirectory(prefix="ottero-sparkle-test-") as tmp:
        root = Path(tmp)
        web = root / "web"
        web.mkdir()
        server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), lambda *a, **kw: QuietHandler(*a, directory=str(web), **kw))
        threading.Thread(target=server.serve_forever, daemon=True).start()
        base = f"http://127.0.0.1:{server.server_port}"
        key, public = keypair(root)
        log = root / "started.log"
        app_id = "com.nexdark.calmfold.updater-fixture." + uuid.uuid4().hex
        host = root / "installed/Calmfold.app"
        newer = root / "package/Ottero.app"
        fixture = root / "Fixture"
        run("xcrun", "swiftc", "-swift-version", "5", ROOT / "native/macos/UpdateFixture.swift", "-o", fixture)
        for app, version, name in [(host, "0.1.0", "Calmfold"), (newer, "0.1.1", "Ottero")]:
            (app / "Contents/MacOS").mkdir(parents=True)
            (app / "Contents/Frameworks").mkdir()
            shutil.copy2(fixture, app / "Contents/MacOS" / name)
            shutil.copytree(SDK / "core/Sparkle.framework", app / "Contents/Frameworks/Sparkle.framework", symlinks=True)
            info = {
                "CFBundleIdentifier": app_id, "CFBundleName": name,
                "CFBundleExecutable": name, "CFBundlePackageType": "APPL",
                "CFBundleVersion": version, "CFBundleShortVersionString": version,
                "SUPublicEDKey": public, "SUFeedURL": base + "/appcast.xml",
                "SURequireSignedFeed": True, "SUVerifyUpdateBeforeExtraction": True,
                "SUEnableAutomaticChecks": False, "SUAutomaticallyUpdate": False,
                "SUShowReleaseNotes": False, "FixtureLog": str(log),
                "NSAppTransportSecurity": {"NSAllowsArbitraryLoads": True},
            }
            (app / "Contents/Info.plist").write_bytes(plistlib.dumps(info))
            helper = app / f"Contents/Helpers/{name}Updater.app"
            (helper / "Contents/MacOS").mkdir(parents=True)
            shutil.copy2(SDK / "OtteroUpdater", helper / f"Contents/MacOS/{name}Updater")
            (helper / "Contents/Info.plist").write_bytes(plistlib.dumps({
                "CFBundleIdentifier": app_id + ".updater", "CFBundleExecutable": f"{name}Updater",
                "CFBundleName": f"{name} Update Bridge Fixture", "CFBundlePackageType": "APPL",
                "CFBundleVersion": version, "LSBackgroundOnly": True,
                "NSAppTransportSecurity": {"NSAllowsArbitraryLoads": True},
            }))
            run("/usr/bin/codesign", "--force", "--sign", "-", helper)
            # Ad-hoc sealing uses no identity or Keychain. Sparkle correctly
            # rejects an ARM executable's loose signature in an unsealed bundle.
            run("/usr/bin/codesign", "--force", "--sign", "-", app)
            run("/usr/bin/codesign", "--verify", "--deep", "--strict", app)
        archive = web / "new.zip"
        run("ditto", "-c", "-k", "--keepParent", newer, archive)
        signature = run(SDK / "bin/sign_update", "--ed-key-file", key, "-p", archive).decode().strip()
        cast = web / "appcast.xml"
        cast.write_text(f'''<?xml version="1.0" encoding="utf-8"?>
<rss version="2.0" xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle"><channel><title>Fixture</title><item>
<title>Fixture 0.1.1</title><sparkle:version>0.1.1</sparkle:version><sparkle:shortVersionString>0.1.1</sparkle:shortVersionString>
<sparkle:minimumSystemVersion>12.0</sparkle:minimumSystemVersion>
<enclosure url="{base}/new.zip" sparkle:edSignature="{signature}" length="{archive.stat().st_size}" type="application/octet-stream" />
</item></channel></rss>''')
        run(SDK / "bin/sign_update", "--ed-key-file", key, cast)
        if tamper:
            with archive.open("r+b") as file:
                file.seek(100); byte = file.read(1); file.seek(100); file.write(bytes([byte[0] ^ 1]))
        old = subprocess.Popen([str(host / "Contents/MacOS/Calmfold")], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        bridge = None
        try:
            deadline = time.monotonic() + 15
            while not log.exists():
                assert old.poll() is None, "old native fixture failed to start"
                assert time.monotonic() < deadline, "old app startup timeout"
                time.sleep(0.05)
            bridge = subprocess.Popen([str(host / "Contents/Helpers/CalmfoldUpdater.app/Contents/MacOS/CalmfoldUpdater"), str(host)], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, bufsize=1)
            incoming = queue.Queue()
            def read_events():
                for line in bridge.stdout:
                    incoming.put(line)
                incoming.put(None)
            threading.Thread(target=read_events, daemon=True).start()
            checked = False
            deadline = time.monotonic() + 90
            events = []
            while time.monotonic() < deadline:
                try:
                    line = incoming.get(timeout=1)
                except queue.Empty:
                    assert bridge.poll() is None, f"bridge exited: {bridge.stderr.read()}"
                    continue
                assert line, f"bridge closed: {bridge.stderr.read()}"
                event = json.loads(line); events.append(event)
                print(json.dumps({"tamper": tamper, **event}), flush=True)
                if event["event"] == "initialized" and not checked:
                    bridge.stdin.write(f"check {base}/appcast.xml 0.1.1\n"); bridge.stdin.flush(); checked = True
                if event["event"] == "failed":
                    assert tamper, f"valid update rejected: {event}"
                    assert old.poll() is None, "tampered update terminated old app"
                    assert plistlib.loads((host / "Contents/Info.plist").read_bytes())["CFBundleVersion"] == "0.1.0"
                    print("PASS: signed feed with tampered archive rejected; old app remains running", flush=True)
                    return
                if event["event"] == "ready":
                    assert not tamper, "tampered update reached install-ready"
                    bridge.stdin.write("install\n"); bridge.stdin.flush()
                if event["event"] == "installed":
                    assert event.get("relaunched") is True
                    boot_deadline = time.monotonic() + 15
                    while not (log.exists() and "0.1.1 " in log.read_text()):
                        assert time.monotonic() < boot_deadline, "new app didn't finish startup after native relaunch"
                        time.sleep(0.1)
                if log.exists() and "0.1.1 " in log.read_text():
                    assert not tamper
                    installed = plistlib.loads((host / "Contents/Info.plist").read_bytes())
                    assert installed["CFBundleVersion"] == "0.1.1" and installed["CFBundleExecutable"] == "Ottero"
                    assert (host / "Contents/Helpers/OtteroUpdater.app/Contents/MacOS/OtteroUpdater").exists()
                    assert not (root / "installed/Ottero.app").exists(), "Sparkle installs at the existing path"
                    print("PASS: real installed Sparkle app upgraded 0.1.0 (Calmfold names) -> 0.1.1 (Ottero names) and relaunched", flush=True)
                    return
            raise AssertionError(f"update timed out: {events}")
        finally:
            if bridge and bridge.poll() is None:
                bridge.terminate()
                try: bridge.wait(timeout=5)
                except subprocess.TimeoutExpired: bridge.kill(); bridge.wait()
            old.terminate() if old.poll() is None else None
            try: old.wait(timeout=5)
            except subprocess.TimeoutExpired: old.kill(); old.wait()
            if log.exists():
                for line in log.read_text().splitlines():
                    try: os.kill(int(line.split()[1]), signal.SIGTERM)
                    except (ProcessLookupError, ValueError, IndexError): pass
            server.shutdown(); server.server_close()


if __name__ == "__main__":
    smoke(False)
    smoke(True)
