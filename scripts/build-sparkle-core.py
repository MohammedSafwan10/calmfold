#!/usr/bin/env python3
"""Build the pinned Sparkle core without WebKit or standard updater UI.
Uses ad-hoc sealing only: no signing identity, team or Keychain is used.
"""
import hashlib
import importlib.util
from pathlib import Path
import shutil
import subprocess
import tarfile
import urllib.request

ROOT = Path(__file__).resolve().parent.parent
COMMIT = "ab7d71291238df6e0492143b120c9b696a35f603"
SHA256 = "d7e5599d56ddb917cddbb4a719fb534ad5d2d7e80a653446d0eea6a38920b302"

def build():
    spec = importlib.util.spec_from_file_location("fetch_sparkle", ROOT / "scripts/fetch-sparkle.py")
    module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module)
    sdk = module.fetch()
    output = sdk / "core/Sparkle.framework"
    if not output.exists():
        archive = sdk / "Sparkle-source.tar.gz"
        if not archive.exists():
            request = urllib.request.Request(f"https://codeload.github.com/sparkle-project/Sparkle/tar.gz/{COMMIT}", headers={"User-Agent": "Calmfold-build"})
            with urllib.request.urlopen(request, timeout=60) as response, archive.open("wb") as file: shutil.copyfileobj(response, file)
        with archive.open("rb") as file:
            if hashlib.file_digest(file, "sha256").hexdigest() != SHA256: raise RuntimeError("Sparkle source checksum mismatch")
        source = sdk / f"source/Sparkle-{COMMIT}"
        if not source.exists():
            with tarfile.open(archive) as tar: tar.extractall(sdk / "source", filter="data")
        derived = sdk / "core-build"
        log = sdk / "core-build.log"
        with log.open("w") as file:
            result = subprocess.run(["xcodebuild", "-project", str(source / "Sparkle.xcodeproj"), "-scheme", "Sparkle", "-configuration", "Release", "-derivedDataPath", str(derived), "ARCHS=arm64", "ONLY_ACTIVE_ARCH=YES", "SPARKLE_BUILD_UI_BITS=0", "CODE_SIGN_IDENTITY=-", "DEVELOPMENT_TEAM=", "CODE_SIGNING_REQUIRED=NO", "build"], stdout=file, stderr=subprocess.STDOUT)
        if result.returncode:
            print("\n".join(log.read_text().splitlines()[-80:])); raise RuntimeError("Sparkle core build failed")
        output.parent.mkdir(exist_ok=True)
        shutil.copytree(derived / "Build/Products/Release/Sparkle.framework", output, symlinks=True)
    for path in output.rglob("*"):
        if not path.is_file() or path.is_symlink(): continue
        with path.open("rb") as file: magic = file.read(4)
        if magic not in [b"\xcf\xfa\xed\xfe", b"\xfe\xed\xfa\xcf", b"\xca\xfe\xba\xbe"]: continue
        linked = subprocess.check_output(["otool", "-L", str(path)], text=True)
        if "WebKit.framework" in linked: raise RuntimeError(f"Unexpected WebKit linkage in {path.name}")
    subprocess.run(["codesign", "--verify", "--deep", "--strict", str(output)], check=True)
    print("Verified Sparkle core: no WebKit linkage")
    return output

if __name__ == "__main__": build()
