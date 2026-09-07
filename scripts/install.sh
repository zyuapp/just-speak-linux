#!/usr/bin/env bash
# Install a committed local build through the same validated release transaction.
set -euo pipefail
project_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
output=$(mktemp -d "${TMPDIR:-/tmp}/just-speak-local-release.XXXXXXXX")
trap 'rm -rf -- "$output"' EXIT
"$project_dir/scripts/create-release.sh" "$output"
version=$("$project_dir/target/release/just-speak" --version)
version=${version#just-speak }
exec_args=(--archive "$output/just-speak-linux-v$version-linux-x86_64.tar.gz" --version "$version" --no-model --no-start --no-bar)
bash "$project_dir/scripts/install-release.sh" "${exec_args[@]}" "$@"
