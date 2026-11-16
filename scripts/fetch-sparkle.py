#!/usr/bin/env python3
"""Fetch the pinned upstream SDK and verify its published SHA-256 before use."""
import hashlib
from pathlib import Path
import tarfile
import urllib.request

VERSION = "2.10.1"
SHA256 = "1a856d2a9edb8e628de88121d7be04cf0e2167f8641650b78fca00b14244cf27"
ROOT = Path(__file__).resolve().parent.parent

def fetch():
    sdk = ROOT / "target/tools/sparkle"
    sdk.mkdir(parents=True, exist_ok=True)
    archive = sdk / f"Sparkle-{VERSION}.tar.xz"
    if not archive.exists():
        request = urllib.request.Request(f"https://github.com/sparkle-project/Sparkle/releases/download/{VERSION}/{archive.name}", headers={"User-Agent": "Calmfold-build"})
        with urllib.request.urlopen(request, timeout=60) as response, archive.open("wb") as output:
            while chunk := response.read(64 * 1024): output.write(chunk)
    with archive.open("rb") as file:
        if hashlib.file_digest(file, "sha256").hexdigest() != SHA256:
            archive.unlink()
            raise RuntimeError("Sparkle SDK checksum mismatch")
    if not (sdk / "Sparkle.framework").exists():
        with tarfile.open(archive) as tar: tar.extractall(sdk, filter="data")
    print(f"Verified Sparkle {VERSION}")
    return sdk

if __name__ == "__main__": fetch()
