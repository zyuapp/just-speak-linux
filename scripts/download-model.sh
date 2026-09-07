#!/usr/bin/env bash
# Explicit one-time download; recognition never accesses the network.
set -euo pipefail
umask 077

model_name='sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8'
archive_name="${model_name}.tar.bz2"
model_url="https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/${archive_name}"
# Published by GitHub for official release asset 283097678, checked 2026-09-06:
# https://api.github.com/repos/k2-fsa/sherpa-onnx/releases/assets/283097678
model_sha256='157c157bc51155e03e37d2466522a3a737dd9c72bb25f36eb18912964161e1ad'

usage() {
    printf 'Usage: %s [MODEL_DIRECTORY]\n' "$0"
    printf 'Downloads and verifies Parakeet TDT 0.6B v2 INT8 (about 483 MB download, 661 MB installed).\n'
    printf 'Default: ${XDG_DATA_HOME:-$HOME/.local/share}/just-speak/models/parakeet-tdt-0.6b-v2-int8\n'
}

if [[ ${1:-} == '--help' || ${1:-} == '-h' ]]; then
    usage
    exit 0
fi
if (( $# > 1 )); then
    usage >&2
    exit 2
fi

for program in curl python3 flock sha256sum mktemp; do
    command -v "$program" >/dev/null || { printf 'Required command is missing: %s\n' "$program" >&2; exit 1; }
done

destination=${1:-${XDG_DATA_HOME:-${HOME:?HOME must be set}/.local/share}/just-speak/models/parakeet-tdt-0.6b-v2-int8}
destination=$(python3 - "$destination" <<'PY'
import os, sys
print(os.path.abspath(sys.argv[1]))
PY
)
parent=$(dirname -- "$destination")
mkdir -p -- "$parent"
# Concurrent invocations must not publish over another download. The descriptor
# lock is automatically released even if an earlier download was interrupted.
exec 9>"${destination}.download.lock"
flock -n 9 || { printf 'Another model download is active for %s\n' "$destination" >&2; exit 1; }

validate_files() {
    local directory=$1 name
    for name in encoder.int8.onnx decoder.int8.onnx joiner.int8.onnx tokens.txt; do
        [[ -f "$directory/$name" && -s "$directory/$name" && ! -L "$directory/$name" ]] || return 1
    done
}

if [[ -e "$destination" || -L "$destination" ]]; then
    if [[ ! -L "$destination" ]] && validate_files "$destination" &&
       [[ -f "$destination/.archive-sha256" ]] && [[ $(<"$destination/.archive-sha256") == "$model_sha256" ]]; then
        printf 'Model already installed: %s\n' "$destination"
        exit 0
    fi
    printf 'Destination exists and is not a complete verified installation: %s\nChoose a new directory, or move the existing directory aside before retrying.\n' "$destination" >&2
    exit 1
fi

staging=$(mktemp -d -- "$parent/.just-speak-download.XXXXXXXX")
trap 'rm -rf -- "$staging"' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
printf 'Downloading Parakeet TDT 0.6B v2 INT8 from the official sherpa-onnx release…\n'
curl --fail --location --proto '=https' --proto-redir '=https' --tlsv1.2 \
    --retry 3 --connect-timeout 20 --output "$staging/$archive_name" "$model_url"
printf '%s  %s\n' "$model_sha256" "$staging/$archive_name" | sha256sum --check --status || {
    printf 'Model archive SHA256 verification failed; nothing was installed.\n' >&2
    exit 1
}

printf 'Checksum verified; extracting model…\n'
python3 - "$staging/$archive_name" "$staging" "$model_name" <<'PY'
import pathlib, shutil, sys, tarfile

archive_path, staging, model_name = sys.argv[1:]
staging = pathlib.Path(staging)
with tarfile.open(archive_path, 'r:bz2') as archive:
    for member in archive:
        path = pathlib.PurePosixPath(member.name)
        if path.is_absolute() or '..' in path.parts or not path.parts or path.parts[0] != model_name:
            raise SystemExit(f'Unsafe archive path: {member.name}')
        target = staging.joinpath(*path.parts)
        if member.isdir():
            target.mkdir(mode=0o700, parents=True, exist_ok=True)
        elif member.isfile():
            target.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
            # Extract regular files only; do not apply archive ownership/modes.
            with archive.extractfile(member) as source, target.open('xb') as output:
                shutil.copyfileobj(source, output)
        else:
            raise SystemExit(f'Unsupported archive entry: {member.name}')
PY

validate_files "$staging/$model_name" || { printf 'Archive is missing required model files.\n' >&2; exit 1; }
printf '%s\n' "$model_sha256" > "$staging/$model_name/.archive-sha256"
# Staging and destination share a filesystem; publication is one atomic rename.
mv -T -- "$staging/$model_name" "$destination"
printf 'Model installed: %s\n' "$destination"
