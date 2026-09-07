"""Stage release-specific QML types while preserving legacy updater paths."""

import json
from pathlib import Path
import re
import shutil


def stage_ui(source: Path, destination: Path, version: str) -> None:
    if not re.fullmatch(r'(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)', version):
        raise ValueError('Expected a stable release version')
    destination.mkdir(parents=True, exist_ok=True)
    components = sorted(source.glob('[A-Z]*.qml'))
    suffix = 'V' + version.replace('.', '_')
    names = {path.stem: path.stem + suffix for path in components}
    if 'Widget' not in names:
        raise ValueError('Missing bar widget')
    pattern = re.compile(r'\b(' + '|'.join(map(re.escape, names)) + r')\b')
    # Old updaters require these flat paths. The standalone overlay also uses
    # shell.qml and the original types. Keep them intact for compatibility.
    for path in sorted(source.iterdir()):
        if path.is_file() and (path.suffix == '.qml' or path.name == 'manifest.json'):
            shutil.copyfile(path, destination / path.name)
    # A new entry-point URL alone is insufficient: imported child components
    # can still come from the QML engine's cache. Rename the entire local type
    # graph, including its internal references, for every release.
    for path in components:
        text = pattern.sub(lambda match: names[match.group()], path.read_text())
        (destination / (names[path.stem] + '.qml')).write_text(text)
    manifest = json.loads((destination / 'manifest.json').read_text())
    if manifest['version'] != version:
        raise ValueError('Plugin and release versions differ')
    manifest['entryPoints']['barWidget'] = names['Widget'] + '.qml'
    (destination / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
