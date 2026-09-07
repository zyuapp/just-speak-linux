#!/usr/bin/env bash
# User-local, desktop-aware installer. Safe to execute with curl ... | bash.
set -euo pipefail
umask 077
repository=${JUST_SPEAK_REPOSITORY:-zyuapp/just-speak-linux}
prefix=${JUST_SPEAK_PREFIX:-${HOME:?HOME is not set}/.local}
version=''
local_archive=''
download_model=1
start_service=1
install_bar=1
bind_f10=0
usage() {
    cat <<'HELP'
Usage: install-release.sh [OPTIONS]
Installs the latest stable Linux x86_64 release into ~/.local, downloads the
483 MB local speech model, and enables the user daemon. On Omarchy it also
installs the bar plugin. Settings/history use the shared GTK interface.

  --version VERSION    Install a specific stable release, e.g. 0.2.0
  --archive FILE       Use an explicitly trusted local release archive; requires --version
  --prefix DIRECTORY  Install into this user-owned prefix instead of ~/.local
  --no-model           Skip the model download
  --no-start           Write integration files but leave the daemon stopped
  --no-bar             Skip Omarchy bar integration
  --bind-f10           Configure F10 through the running app on supported Omarchy
  --help               Show this help

Requires curl, Python 3, systemd user services, PipeWire's pw-record and wpctl,
and wl-copy. The settings window and shortcut recorder additionally need GJS and GTK 4. Ubuntu
GNOME hold-to-talk and automatic paste are experimental and unverified.
No sudo, package installation, or keybinding changes occur by default.
HELP
}
while (( $# )); do
    case "$1" in
        --version|--archive|--prefix)
            (( $# >= 2 )) || { printf 'Missing value for %s\n' "$1" >&2; exit 2; }
            case "$1" in --version) version=${2#v};; --archive) local_archive=$2;; --prefix) prefix=$2;; esac
            shift 2 ;;
        --no-model) download_model=0; shift ;;
        --no-start) start_service=0; shift ;;
        --no-bar) install_bar=0; shift ;;
        --bind-f10) bind_f10=1; shift ;;
        --help|-h) usage; exit 0 ;;
        *) printf 'Unknown option: %s\n' "$1" >&2; usage >&2; exit 2 ;;
    esac
done
[[ $(uname -s) == Linux && $(uname -m) == x86_64 ]] || { printf 'Only Linux x86_64 is supported.\n' >&2; exit 1; }
(( EUID != 0 )) || { printf 'Run this installer as your desktop user, without sudo.\n' >&2; exit 1; }
[[ $prefix == /* && $prefix != *$'\n'* ]] || { printf 'The install prefix must be an absolute path without newlines.\n' >&2; exit 1; }
if (( bind_f10 && ! start_service )); then printf '%s\n' '--bind-f10 requires starting the daemon; omit --no-start.' >&2; exit 2; fi
if [[ -n $local_archive && -z $version ]]; then printf '%s\n' '--archive requires --version.' >&2; exit 2; fi
for program in curl python3 systemctl pw-record wpctl wl-copy; do
    if ! command -v "$program" >/dev/null; then
        printf 'Required command missing: %s\n' "$program" >&2
        printf 'Ubuntu 22.04+: sudo apt install curl python3 pipewire-bin wireplumber wl-clipboard gjs gir1.2-gtk-4.0\nArch/Omarchy: sudo pacman -S curl python pipewire wireplumber wl-clipboard gjs gtk4\n' >&2
        exit 1
    fi
done
if ! command -v gjs >/dev/null; then
    printf 'The settings window and shortcut recorder need GJS and GTK 4 (Ubuntu: gjs gir1.2-gtk-4.0; Arch: gjs gtk4). The daemon can still run.\n' >&2
fi
export JUST_SPEAK_PREFIX=$prefix
config_dir=${XDG_CONFIG_HOME:-$HOME/.config}
data_dir=${XDG_DATA_HOME:-$HOME/.local/share}
[[ $config_dir == /* && $data_dir == /* && $config_dir$data_dir != *$'\n'* ]] || { printf 'XDG config/data directories must be absolute paths without newlines.\n' >&2; exit 1; }
staging=$(mktemp -d "${TMPDIR:-/tmp}/just-speak-install.XXXXXXXX")
service_was_active=0
installed=0
cleanup() {
    local code=$?
    if (( code != 0 && service_was_active && ! installed )); then
        systemctl --user start just-speak.service || true
    fi
    rm -rf -- "$staging"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
fetch() {
    curl --fail --location --silent --show-error --proto '=https' --proto-redir '=https' \
        --tlsv1.2 --connect-timeout 15 --max-time 180 --retry 1 --max-filesize "$3" --output "$2" "$1"
}
if [[ -z $local_archive ]]; then
    api_suffix=$(python3 - "$repository" "$version" <<'PY'
import re, sys
repository, version = sys.argv[1:]
if not re.fullmatch(r'[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+', repository) or any(p in ('.', '..') or len(p) > 100 for p in repository.split('/')):
    raise SystemExit('Invalid GitHub repository name.')
if version and not re.fullmatch(r'(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)', version):
    raise SystemExit('Only stable semantic versions are supported.')
print('tags/v' + version if version else 'latest')
PY
)
    printf 'Checking stable releases for %s…\n' "$repository"
    fetch "https://api.github.com/repos/$repository/releases/$api_suffix" "$staging/release-api.json" 1048576
    python3 - "$staging/release-api.json" "$repository" "$version" "$staging" <<'PY'
import json, pathlib, re, sys
metadata, repository, requested, temporary = sys.argv[1:]
r = json.loads(pathlib.Path(metadata).read_text())
version = r.get('tag_name', '').removeprefix('v')
if r.get('draft', True) or r.get('prerelease', True) or not re.fullmatch(r'(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)', version) or r['tag_name'] != 'v' + version or (requested and version != requested):
    raise SystemExit('GitHub did not return the requested stable release.')
name = 'just-speak-linux-v' + version + '-linux-x86_64.tar.gz'
base = 'https://github.com/' + repository + '/releases/download/v' + version + '/'
for asset_name, size_limit in [(name, 128 * 1024 * 1024), ('SHA256SUMS', 1024 * 1024)]:
    matches = [a for a in r.get('assets', []) if a.get('name') == asset_name]
    if len(matches) != 1 or matches[0].get('browser_download_url') != base + asset_name or not 0 < matches[0].get('size', 0) <= size_limit:
        raise SystemExit('Release asset is missing, duplicated, oversized, or from an unexpected URL: ' + asset_name)
pathlib.Path(temporary, 'version').write_text(version)
PY
    version=$(<"$staging/version")
    archive_name="just-speak-linux-v$version-linux-x86_64.tar.gz"
    release_base="https://github.com/$repository/releases/download/v$version"
    printf 'Downloading JustSpeak %s…\n' "$version"
    fetch "$release_base/$archive_name" "$staging/release.tar.gz" 134217728
    fetch "$release_base/SHA256SUMS" "$staging/SHA256SUMS" 1048576
    python3 - "$staging/release.tar.gz" "$staging/SHA256SUMS" "$archive_name" <<'PY'
import hashlib, pathlib, re, sys
archive, sums, name = sys.argv[1:]
checksums = []
for line in pathlib.Path(sums).read_text().splitlines():
    fields = line.split()
    if len(fields) == 2 and fields[1].removeprefix('*') == name:
        if not re.fullmatch('[0-9a-fA-F]{64}', fields[0]):
            raise SystemExit('Malformed release checksum.')
        checksums.append(fields[0].lower())
with open(archive, 'rb') as source:
    digest = hashlib.sha256()
    for block in iter(lambda: source.read(1024 * 1024), b''):
        digest.update(block)
if checksums != [digest.hexdigest()]:
    raise SystemExit('Release SHA256 check failed. Nothing was installed.')
PY
    local_archive=$staging/release.tar.gz
    printf 'Release checksum verified against publisher-provided SHA256SUMS.\n'
else
    printf 'Using explicitly trusted local archive: %s\n' "$local_archive"
fi
# Extract only the bootstrap executable. It validates the full archive again
# before applying anything. Never use tar.extractall on a release download.
python3 - "$local_archive" "$version" "$staging/bootstrap" <<'PY'
import json, pathlib, re, shutil, sys, tarfile
archive, version, executable = sys.argv[1:]
if not re.fullmatch(r'(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)', version):
    raise SystemExit('Invalid stable release version.')
if pathlib.Path(archive).stat().st_size > 128 * 1024 * 1024:
    raise SystemExit('Release archive is too large.')
seen, expanded, manifest, binary = set(), 0, None, None
required = {'bin/just-speak', 'release.json', 'share/just-speak/packaging/just-speak.service', 'share/just-speak/packaging/just-speak.desktop'}
with tarfile.open(archive, 'r:gz') as package:
    for index, member in enumerate(package):
        name = member.name
        if index >= 512 or not member.isfile() or not re.fullmatch(r'[A-Za-z0-9/._-]{1,240}', name) or name.startswith('/') or any(part in ('', '.', '..') for part in name.split('/')) or name in seen:
            raise SystemExit('Unsafe or duplicate release archive entry.')
        seen.add(name)
        expanded += member.size
        if not 0 < member.size <= 256 * 1024 * 1024 or expanded > 256 * 1024 * 1024:
            raise SystemExit('Release archive exceeds its extraction limit.')
        if name == 'release.json':
            if member.size > 1024 * 1024:
                raise SystemExit('Oversized release manifest.')
            manifest = json.load(package.extractfile(member))
        elif name == 'bin/just-speak':
            with package.extractfile(member) as source, open(executable, 'xb') as target:
                shutil.copyfileobj(source, target, 1024 * 1024)
            binary = pathlib.Path(executable)
if not required.issubset(seen) or not binary or not manifest or manifest.get('format') != 1 or manifest.get('target') != 'linux-x86_64' or manifest.get('version') != version or sorted(manifest.get('files', [])) != sorted(seen - {'release.json'}):
    raise SystemExit('Incomplete release or mismatched manifest.')
with binary.open('rb') as source:
    header = source.read(20)
if header[:6] != b'\x7fELF\x02\x01' or header[18:20] != b'\x3e\x00':
    raise SystemExit('Release executable is not Linux x86_64.')
binary.chmod(0o755)
PY
[[ $("$staging/bootstrap" --version) == "just-speak $version" ]] || { printf 'Release executable version mismatch.\n' >&2; exit 1; }
# Downloads and validation are done. Quiesce an existing daemon before migration.
# New versions gate capture; the old 0.1 prototype supports only the idle check.
if systemctl --user is-active --quiet just-speak.service; then
    service_was_active=1
    python3 - <<'PY'
import json, os, socket
path = os.path.join(os.environ.get('XDG_RUNTIME_DIR', '/run/user/' + str(os.getuid())), 'just-speak/control.sock')
def request(command):
    with socket.socket(socket.AF_UNIX) as client:
        client.settimeout(3)
        client.connect(path)
        client.sendall(json.dumps({'command': command}).encode() + b'\n')
        data = b''
        while b'\n' not in data and len(data) <= 16384:
            block = client.recv(4096)
            if not block:
                break
            data += block
        return json.loads(data.split(b'\n', 1)[0])
status = request('status')
if not status.get('ok') or status.get('status', {}).get('phase') in ('recording', 'transcribing', 'loading', 'updating'):
    raise SystemExit('JustSpeak is busy. Finish recording/loading and run the installer again.')
try:
    gated = request('begin_update')
    if not gated.get('ok'):
        raise SystemExit('JustSpeak refused to prepare for an update: ' + str(gated.get('error')))
except (ValueError, ConnectionError):
    # v0.1 closes the connection for the unknown BeginUpdate request.
    pass
PY
    systemctl --user stop just-speak.service
else
    runtime_dir=${XDG_RUNTIME_DIR:-/run/user/$(id -u)}
    if [[ -S $runtime_dir/just-speak/control.sock ]]; then
        printf 'A JustSpeak daemon is running outside its user service. Stop it before installing.\n' >&2
        exit 1
    fi
fi
"$staging/bootstrap" update install-archive "$local_archive" --version "$version"
installed=1
payload=$prefix/share/just-speak/updates/current
python3 - "$prefix" "$config_dir" "$data_dir" "$payload" <<'PY'
import os, pathlib, sys, tempfile
prefix, config, data, payload = map(pathlib.Path, sys.argv[1:])
def write(path, text, mode=0o644):
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(mode='w', dir=path.parent, delete=False) as output:
        temporary = pathlib.Path(output.name)
        output.write(text)
    temporary.chmod(mode)
    os.replace(temporary, path)
def unit_quote(text):
    return '"' + str(text).replace('\\', '\\\\').replace('"', '\\"').replace('%', '%%') + '"'
unit = (payload / 'share/just-speak/packaging/just-speak.service').read_text()
lines = []
for line in unit.splitlines():
    if line.startswith('ExecStart='):
        line = 'ExecStart=' + unit_quote(prefix / 'bin/just-speak') + ' daemon'
    if line.startswith('Environment=PATH='):
        line = 'Environment=' + unit_quote('PATH=' + str(prefix / 'bin') + ':/usr/local/bin:/usr/bin')
    lines.append(line)
    if line == '[Service]':
        for key in ['JUST_SPEAK_PREFIX', 'XDG_CONFIG_HOME', 'XDG_DATA_HOME', 'XDG_STATE_HOME']:
            if value := os.environ.get(key):
                lines.append('Environment=' + unit_quote(key + '=' + value))
write(config / 'systemd/user/just-speak.service', '\n'.join(lines) + '\n')
desktop = (payload / 'share/just-speak/packaging/just-speak.desktop').read_text()
def desktop_quote(text):
    return '"' + str(text).replace('\\', '\\\\\\\\').replace('"', '\\\\"').replace('`', '\\\\`').replace('$', '\\\\$').replace('%', '%%') + '"'
desktop = '\n'.join('Exec=' + desktop_quote(prefix / 'bin/just-speak') + ' window' if line.startswith('Exec=') else line for line in desktop.splitlines()) + '\n'
write(data / 'applications/just-speak.desktop', desktop)
PY
if (( download_model )); then
    "$prefix/bin/just-speak" model download
fi
systemctl --user daemon-reload
if (( start_service )); then
    systemctl --user enable --now just-speak.service
fi
if (( install_bar )) && command -v omarchy >/dev/null && command -v omarchy-shell >/dev/null; then
    plugin_dir=$config_dir/omarchy/plugins/local.just-speak
    mkdir -p -- "$(dirname -- "$plugin_dir")"
    if [[ -L $plugin_dir ]]; then
        if [[ $(readlink -- "$plugin_dir") != "$prefix/share/just-speak/ui" ]]; then
            mv -- "$plugin_dir" "$plugin_dir.backup.$(date +%s)"
            ln -s -- "$prefix/share/just-speak/ui" "$plugin_dir"
        fi
    elif [[ -e $plugin_dir ]]; then
        mv -- "$plugin_dir" "$plugin_dir.backup.$(date +%s)"
        ln -s -- "$prefix/share/just-speak/ui" "$plugin_dir"
    else
        ln -s -- "$prefix/share/just-speak/ui" "$plugin_dir"
    fi
    if (( start_service )); then
        omarchy-shell shell rescanPlugins
        omarchy plugin enable local.just-speak --section right
        # The bar plugin owns the recording overlay on current Omarchy.
        systemctl --user disable --now just-speak-overlay.service 2>/dev/null || true
    fi
else
    printf 'Desktop integration: shared GTK settings window installed. GNOME shortcuts and automatic paste remain experimental/unverified.\n'
fi
if (( bind_f10 )); then
    python3 - "$prefix/bin/just-speak" <<'PYREADY'
import json, subprocess, sys, time
for attempt in range(60):
    result = subprocess.run([sys.argv[1], 'status', '--json'], capture_output=True, text=True)
    try:
        status = json.loads(result.stdout)
        if result.returncode == 0 and status.get('phase') == 'idle' and status.get('model_ready'):
            break
    except ValueError:
        pass
    time.sleep(0.5)
else:
    raise SystemExit('Daemon did not become ready within 30 seconds. Once ready, run: just-speak shortcut set F10')
PYREADY
    "$prefix/bin/just-speak" shortcut set F10
fi
printf '\nInstalled JustSpeak %s into %s.\n' "$version" "$prefix"
printf 'Open settings: %s/bin/just-speak window\n' "$prefix"
if (( ! start_service )); then printf 'Start when ready: systemctl --user enable --now just-speak.service\n'; fi
if [[ :${PATH}: != *:"$prefix/bin":* ]]; then
    printf 'Add %s/bin to your shell PATH to run just-speak by name.\n' "$prefix"
fi
