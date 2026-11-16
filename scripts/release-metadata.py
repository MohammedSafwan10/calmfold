#!/usr/bin/env python3
"""Create the catalog from actual native packages, never partial release assets."""
import hashlib
import json
from pathlib import Path
import re
import sys
import zipfile
import plistlib
import xml.etree.ElementTree as ET

version, directory = sys.argv[1:]
assert re.fullmatch(r"0|[1-9]\d*", version.split('.')[0]) and re.fullmatch(r"\d+\.\d+\.\d+", version), 'invalid version'
root = Path(directory)
base = f'https://github.com/MohammedSafwan10/calmfold-releases/releases/download/v{version}'
mac = root / f'Calmfold-{version}-macos-arm64.zip'
windows = list(root.glob(f'Calmfold-{version}-full.nupkg'))
assert len(windows) == 1, 'exact Windows full package required'
with zipfile.ZipFile(mac) as archive:
    info = plistlib.loads(archive.read('Calmfold.app/Contents/Info.plist'))
    assert info['CFBundleVersion'] == version and info['CFBundleShortVersionString'] == version
with zipfile.ZipFile(windows[0]) as archive:
    nuspecs = [n for n in archive.namelist() if n.endswith('.nuspec')]
    assert len(nuspecs) == 1
    metadata = ET.fromstring(archive.read(nuspecs[0]))
    assert any(node.tag.rsplit('}',1)[-1] == 'version' and node.text == version for node in metadata.iter())
entries = json.loads(Path('website/src/data/changelog.json').read_text())
notes = next(e for e in entries if e['version'] == version)
platforms = {}
for platform, artifact in [('macos-arm64', mac), ('windows-x64', windows[0])]:
    with artifact.open('rb') as file: digest = hashlib.file_digest(file, 'sha256').hexdigest()
    size = artifact.stat().st_size
    assert 0 < size <= 300 * 1024 * 1024, 'package exceeds client size limit'
    platforms[platform] = dict(url=f'{base}/{artifact.name}', sha256=digest, size=size)
assert (root/'appcast.xml').exists() and list(root.glob('*Setup.exe')) and list(root.glob('*Portable.zip'))
(root/'stable.json').write_text(json.dumps(dict(version=version,date=notes['date'],notes=[item for section in notes['sections'] for item in section['items']],platforms=platforms), indent=2)+'\n')
print('Verified package versions and generated complete release catalog')
