#!/usr/bin/env bash
# Installs a previously built release into ~/.local; does not activate services.
set -euo pipefail

project_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
prefix=${JUST_SPEAK_PREFIX:-"$HOME/.local"}
config_dir=${XDG_CONFIG_HOME:-"$HOME/.config"}

if [[ ! -x "$project_dir/target/release/just-speak" ]]; then
    printf '%s\n' 'Build first: cargo build --release --locked' >&2
    exit 1
fi
if [[ "$prefix" != /* || "$config_dir" != /* || "$prefix$config_dir" == *$'\n'* ]]; then
    printf '%s\n' 'Install prefix and XDG_CONFIG_HOME must be absolute paths without newlines.' >&2
    exit 1
fi

install -Dm755 "$project_dir/target/release/just-speak" "$prefix/bin/just-speak"
install -Dm644 "$project_dir/LICENSE" "$prefix/share/licenses/just-speak-linux/LICENSE"
install -Dm644 "$project_dir/THIRD_PARTY_NOTICES.md" "$prefix/share/licenses/just-speak-linux/THIRD_PARTY_NOTICES.md"
install -d "$prefix/share/just-speak/ui" "$prefix/share/just-speak/packaging" "$config_dir/systemd/user"
install -m644 "$project_dir"/ui/*.qml "$project_dir/ui/manifest.json" "$prefix/share/just-speak/ui/"
install -m644 "$project_dir/packaging/hyprland.lua" "$project_dir/packaging/README.md" "$prefix/share/just-speak/packaging/"

# systemd expands % specifiers even in quoted strings; escape them in user paths.
unit_quote() {
    local value=${1//\\/\\\\}
    value=${value//\"/\\\"}
    value=${value//%/%%}
    printf '"%s"' "$value"
}
exec_quote() {
    # ExecStart arguments expand dollars; its executable and Environment do not.
    local value=${1//\$/\$\$}
    unit_quote "$value"
}
{
    while IFS= read -r line; do
        case "$line" in
            ExecStart=*) printf 'ExecStart=%s daemon\n' "$(unit_quote "$prefix/bin/just-speak")" ;;
            Environment=PATH=*) printf 'Environment=%s\n' "$(unit_quote "PATH=$prefix/bin:/usr/local/bin:/usr/bin")" ;;
            *) printf '%s\n' "$line" ;;
        esac
    done < "$project_dir/packaging/just-speak.service"
} > "$config_dir/systemd/user/just-speak.service"
{
    while IFS= read -r line; do
        case "$line" in
            ExecStart=*) printf 'ExecStart=/usr/bin/quickshell --no-duplicate --path %s\n' "$(exec_quote "$prefix/share/just-speak/ui")" ;;
            Environment=PATH=*) printf 'Environment=%s\n' "$(unit_quote "PATH=$prefix/bin:/usr/local/bin:/usr/bin")" ;;
            *) printf '%s\n' "$line" ;;
        esac
    done < "$project_dir/packaging/just-speak-overlay.service"
} > "$config_dir/systemd/user/just-speak-overlay.service"

printf 'Installed JustSpeak to %s.\n' "$prefix"
printf '%s\n' 'Next: ensure the bin directory is on PATH, download the model, then activate the service.'
printf 'Integration instructions: %s/share/just-speak/packaging/README.md\n' "$prefix"
