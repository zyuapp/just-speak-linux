#!/usr/bin/env bash
# Package a tested, committed Linux x86_64 build. Does not publish anything.
set -euo pipefail
project_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
if [[ ${1:-} == --help ]]; then
    printf 'Usage: %s [OUTPUT_DIRECTORY]\nBuild the ASR-only release binary and commit source changes first.\n' "$0"
    exit 0
fi
if (( $# > 1 )); then printf 'Expected at most one output directory.\n' >&2; exit 2; fi
python3 - "$project_dir" "${1:-$project_dir/dist}" <<'PY'
import gzip, hashlib, io, json, os, pathlib, re, shutil, subprocess, sys, tarfile, tempfile

project = pathlib.Path(sys.argv[1])
sys.path.insert(0, str(project / 'scripts'))
from package_ui import stage_ui
output = pathlib.Path(sys.argv[2]).absolute()
package = (project / 'Cargo.toml').read_text().split('[package]', 1)[1].split('\n[', 1)[0]
version = re.search(r'^version\s*=\s*"([0-9]+\.[0-9]+\.[0-9]+)"\s*$', package, re.M).group(1)
binary = project / 'target/release/just-speak'
if not binary.is_file():
    raise SystemExit('Missing release binary; build with the ASR-only native runtime first.')
reported = subprocess.check_output([str(binary), '--version'], text=True).strip()
if reported != 'just-speak ' + version:
    raise SystemExit(f'Binary version {reported!r} does not match Cargo.toml {version}.')
symbols = subprocess.run(['nm', '-C', '--defined-only', str(binary)], check=True, text=True, capture_output=True).stdout
if not symbols.strip():
    raise SystemExit('Release symbol table is missing; cannot verify that the TTS dependencies are absent.')
if re.search(r'espeak[_:]|piper::|piper_phonemize', symbols, re.I):
    raise SystemExit('Refusing to package a binary that contains eSpeak/Piper TTS code. Use the ASR-only native build.')
for args in [['git', 'diff', '--quiet'], ['git', 'diff', '--cached', '--quiet']]:
    if subprocess.run(args, cwd=project).returncode:
        raise SystemExit('Commit source changes before creating a release.')
untracked = subprocess.check_output(['git', 'ls-files', '--others', '--exclude-standard'], cwd=project, text=True).strip()
if untracked:
    raise SystemExit('Commit or explicitly ignore untracked files before creating a release:\n' + untracked)
head_cargo = subprocess.check_output(['git', 'show', 'HEAD:Cargo.toml'], cwd=project, text=True)
if head_cargo != (project / 'Cargo.toml').read_text():
    raise SystemExit('Committed Cargo.toml does not match the release source.')
epoch = int(subprocess.check_output(['git', 'show', '-s', '--format=%ct', 'HEAD'], cwd=project, text=True))
output.mkdir(parents=True, exist_ok=True)
archive_name = f'just-speak-linux-v{version}-linux-x86_64.tar.gz'
source_name = f'just-speak-linux-v{version}-source.tar.gz'
with tempfile.TemporaryDirectory(prefix='just-speak-release-') as temporary:
    stage = pathlib.Path(temporary)
    def copy(source, relative):
        destination = stage / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, destination)
    copy(binary, 'bin/just-speak')
    stage_ui(project / 'ui', stage / 'share/just-speak/ui', version)
    for source in sorted((project / 'gtk').iterdir()):
        if source.is_file() and source.suffix in ('.js', '.css', '.json'):
            copy(source, 'share/just-speak/gtk/' + source.name)
    for name in ['just-speak.service', 'just-speak-overlay.service', 'hyprland.lua', 'README.md', 'just-speak.desktop']:
        copy(project / 'packaging' / name, 'share/just-speak/packaging/' + name)
    copy(project / 'scripts/download-model.sh', 'share/just-speak/scripts/download-model.sh')
    subprocess.run([sys.executable, str(project / 'scripts/collect-licenses.py'), str(stage / 'share/licenses/just-speak-linux')], cwd=project, check=True)
    files = sorted(path.relative_to(stage).as_posix() for path in stage.rglob('*') if path.is_file())
    if len(files) + 1 > 512 or sum((stage / name).stat().st_size for name in files) > 256 * 1024 * 1024:
        raise SystemExit('Release exceeds updater archive limits.')
    for name in files:
        size = (stage / name).stat().st_size
        if not re.fullmatch(r'[A-Za-z0-9/._-]{1,240}', name) or not 0 < size <= (256 * 1024 * 1024 if name == 'bin/just-speak' else 8 * 1024 * 1024):
            raise SystemExit('Release file violates updater archive limits: ' + name)
    manifest = {'format': 1, 'version': version, 'target': 'linux-x86_64', 'files': files}
    (stage / 'release.json').write_text(json.dumps(manifest, indent=2) + '\n')
    with (output / archive_name).open('wb') as raw, gzip.GzipFile(fileobj=raw, mode='wb', mtime=epoch, filename='') as zipped, tarfile.open(fileobj=zipped, mode='w', format=tarfile.USTAR_FORMAT) as archive:
        for name in sorted(files + ['release.json']):
            data = (stage / name).read_bytes()
            member = tarfile.TarInfo(name)
            member.size = len(data)
            member.mtime = epoch
            member.mode = 0o755 if name == 'bin/just-speak' or name.endswith('/download-model.sh') else 0o644
            archive.addfile(member, io.BytesIO(data))
subprocess.run(['git', 'archive', '--format=tar.gz', '--prefix=just-speak-linux-v' + version + '/', '--output=' + str(output / source_name), 'HEAD'], cwd=project, check=True)
shutil.copyfile(project / 'scripts/install-release.sh', output / 'install-release.sh')
assets = [archive_name, source_name, 'install-release.sh']
with (output / 'SHA256SUMS').open('w') as sums:
    for name in assets:
        digest = hashlib.sha256((output / name).read_bytes()).hexdigest()
        sums.write(digest + '  ' + name + '\n')
print(f'Created v{version} release assets in {output}')
for name in assets + ['SHA256SUMS']:
    print(f'  {name}: {(output / name).stat().st_size:,} bytes')
PY
