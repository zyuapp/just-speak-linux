#!/usr/bin/env python3
"""Collect pinned native and Rust notices for binary and distro packages.

Usage: SHERPA_ONNX_LIB_DIR=/path/native/lib python3 scripts/collect-licenses.py DEST
Run after cargo build --locked has fetched the Linux dependency set.
"""
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys

if len(sys.argv) != 2:
    raise SystemExit(__doc__)
project = pathlib.Path(__file__).resolve().parent.parent
stage = pathlib.Path(sys.argv[1]).absolute()
if stage.exists():
    raise SystemExit('License destination already exists; use a fresh staging directory.')
stage.mkdir(parents=True)

def copy(source, relative):
    destination = stage / relative
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, destination)

for name in ['LICENSE', 'THIRD_PARTY_NOTICES.md']:
    copy(project / name, name)
native_library_dir = os.environ.get('SHERPA_ONNX_LIB_DIR')
if not native_library_dir:
    raise SystemExit('Set SHERPA_ONNX_LIB_DIR to the ASR-only runtime used for this build.')
native_notices = pathlib.Path(native_library_dir).resolve().parent / 'licenses/native'
if not (native_notices / 'inventory.json').is_file() or not (native_notices / 'onnxruntime/onnxruntime-ThirdPartyNotices.txt').is_file():
    raise SystemExit('Native runtime license inventory is incomplete; run scripts/build-runtime.sh first.')
for source in sorted(native_notices.rglob('*')):
    if source.is_symlink():
        raise SystemExit('Native notices must not contain symlinks: ' + str(source))
    if source.is_file():
        copy(source, 'native/' + source.relative_to(native_notices).as_posix())
# Resolve only Linux dependencies; include build-time packages too so the
# release retains their attribution without needing a separate cargo plugin.
metadata = json.loads(subprocess.check_output(
    ['cargo', 'metadata', '--locked', '--offline', '--format-version', '1', '--filter-platform', 'x86_64-unknown-linux-gnu'], cwd=project, text=True))
inventory = []
for package in sorted(metadata['packages'], key=lambda p: (p['name'], p['version'])):
    crate_dir = pathlib.Path(package['manifest_path']).parent
    key = re.sub(r'[^A-Za-z0-9._-]', '_', package['name'] + '-' + package['version'])
    candidates = set()
    for child in crate_dir.iterdir():
        if child.name.lower().startswith(('license', 'copying', 'notice')):
            if child.is_dir():
                candidates.update(path for path in child.rglob('*') if path.is_file())
            elif child.is_file():
                candidates.add(child)
    if package.get('license_file'):
        candidates.add(crate_dir / package['license_file'])
    if not candidates:
        raise SystemExit('No license text found for Rust package ' + key)
    packaged = []
    for index, source in enumerate(sorted(candidates)):
        if not source.is_file():
            raise SystemExit('Missing Rust license text: ' + str(source))
        # Flatten each crate's paths and sanitize unusual filenames to the
        # updater archive allowlist; inventory records the upstream name.
        filename = str(index) + '-' + re.sub(r'[^A-Za-z0-9._-]', '_', source.name)
        relative = 'rust/' + key + '/' + filename
        copy(source, relative)
        packaged.append({'upstream_file': source.name, 'file': relative})
    inventory.append({'name': package['name'], 'version': package['version'], 'license': package.get('license'), 'repository': package.get('repository'), 'source': package.get('source'), 'license_files': packaged})
(stage / 'rust/inventory.json').write_text(json.dumps(inventory, indent=2) + '\n')
print(f'Collected {len(inventory)} Rust package notices and native runtime notices in {stage}')
