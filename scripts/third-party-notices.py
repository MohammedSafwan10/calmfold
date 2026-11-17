#!/usr/bin/env python3
"""Bundle dependency declarations and available upstream license/notice texts.
Run after building on the target host; no extra Python/Cargo dependencies.
This is an attribution inventory, not a legal compatibility audit or SBOM.
"""
import hashlib
import json
from pathlib import Path
import subprocess
import sys

output = Path(sys.argv[1])
host = next(line.split(': ', 1)[1] for line in subprocess.check_output(['rustc','-vV'],encoding='utf-8').splitlines() if line.startswith('host: '))
metadata = json.loads(subprocess.check_output(['cargo','metadata','--locked','--format-version','1','--filter-platform',host],encoding='utf-8'))
active = {node['id'] for node in metadata['resolve']['nodes']}
own = set(metadata['workspace_members'])
texts = {}
lines = ['Ottero — third-party notices', '',
         'Dependency declarations for the build host '+host+'. Includes build/test dependencies resolved for the workspace.',
         'Upstream components retain their own licenses. This inventory does not replace the original terms.', '']

def add(path, root):
    path = path.resolve()
    if not path.is_relative_to(root.resolve()) or not path.is_file(): return None
    body = path.read_text(encoding='utf-8', errors='replace')
    digest = hashlib.sha256(body.encode()).hexdigest()
    texts.setdefault(digest, body)
    return digest

for package in sorted(metadata['packages'], key=lambda p:(p['name'],p['version'])):
    if package['id'] not in active or package['id'] in own: continue
    root = Path(package['manifest_path']).parent
    files = [p for p in root.rglob('*') if p.is_file() and (p.name.lower().startswith(('license','licence','copying','notice','copyright')) or p.name.lower() == 'ofl.txt')]
    for directory in ['LICENSES','licenses']:
        if (root/directory).is_dir(): files += [p for p in (root/directory).rglob('*') if p.is_file()]
    if package.get('license_file'): files.append(root/package['license_file'])
    refs = sorted({ref for p in files if (ref:=add(p,root)) is not None})
    lines += [package['name']+' '+package['version'], 'Declared license: '+(package.get('license') or 'See upstream license file'),
              'Upstream: '+(package.get('repository') or 'https://crates.io/crates/'+package['name']),
              'License text references: '+(', '.join(refs) or 'No standalone text in the Cargo package; consult upstream'), '']
root = Path(__file__).resolve().parent.parent
for path in [root/'docs/licenses/lucide-LICENSE.txt', root/'crates/core/data/holidays-LICENSE.txt', *sorted((root/'crates/ui/assets/fonts').glob('*/OFL.txt'))]:
    ref=add(path,root)
    lines += [str(path.relative_to(root)), 'License text reference: '+str(ref), '']
for digest, body in sorted(texts.items()): lines += ['='*72, 'License text '+digest, '='*72, body, '']
output.parent.mkdir(parents=True,exist_ok=True)
output.write_text('\n'.join(lines),encoding='utf-8')
print(f'Wrote dependency declarations and {len(texts)} distinct license texts to {output.name}')
