#!/usr/bin/env python3
"""Put the exact public source location beside each official binary package."""
import os
from pathlib import Path
import re
import subprocess
import sys

revision = os.environ.get('GITHUB_SHA') or subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()
assert re.fullmatch(r'[0-9a-f]{40}',revision), 'invalid source revision'
dirty = not os.environ.get('GITHUB_SHA') and bool(subprocess.check_output(['git','status','--porcelain','--','apps','crates','Cargo.toml','Cargo.lock','native','scripts']))
label = 'Development base revision (uncommitted changes present)' if dirty else 'Exact build revision'
root=Path(sys.argv[1]); root.mkdir(parents=True,exist_ok=True)
repo='https://github.com/MohammedSafwan10/ottero'
(root/'SOURCE.txt').write_text(f'''Ottero source code — GPL-3.0-only

{label}: {revision}
Source: {repo}/tree/{revision}
Source archive: {repo}/archive/{revision}.tar.gz

Build instructions, dependency locks and scripts are included in that revision.
Cargo obtains the pinned third-party dependencies from their public upstreams.
Third-party components retain their own licenses; see THIRD_PARTY_NOTICES.txt.
See LICENSE for the complete GNU GPL version 3 terms.
''',encoding='utf-8')
