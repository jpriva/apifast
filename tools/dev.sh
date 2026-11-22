#!/usr/bin/env sh
# Runs apifast from the source, for development (docs/14-desarrollo.md), with
# the variables of the repository's .env (or the file APIFAST_ENV_FILE names):
#
#   tools/dev.sh                      the interface, debug build
#   tools/dev.sh gui ~/my-collection  the interface, opening that collection
#   tools/dev.sh --release gui        the interface, optimized
#   tools/dev.sh cli run ~/my-coll    the command line, with its arguments
#
# .env holds KEY=VALUE lines (see .env.example); it is read, never run, and a
# variable already set in the shell wins over it. APIFAST_DATA_DIR and
# APIFAST_CONFIG_DIR, when relative, are relative to the repository, so data
# stays apart from an installed apifast whatever folder this runs from. Only
# this script reads .env: an installed apifast does not.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
env_file=${APIFAST_ENV_FILE:-$root/.env}

# KEY=VALUE, with an optional `export `, quotes around the value, blank lines
# and # comments.
load_env() { # file
    while IFS= read -r line || [ -n "$line" ]; do
        line=$(printf '%s' "$line" | sed 's/^[[:space:]]*//; s/[[:space:]]*$//')
        case "$line" in '' | '#'*) continue ;; esac
        line=${line#export }
        key=${line%%=*}
        value=${line#*=}
        case "$key" in
            *[!A-Za-z0-9_]* | '' | [0-9]*)
                echo "dev.sh: $1: not a KEY=VALUE line: $line" >&2
                exit 2
                ;;
        esac
        case "$value" in
            \"*\") value=${value#\"}; value=${value%\"} ;;
            \'*\') value=${value#\'}; value=${value%\'} ;;
        esac
        # The shell's own value wins.
        if eval "[ -z \"\${$key+set}\" ]"; then
            export "$key=$value"
        fi
    done <"$1"
}

if [ -f "$env_file" ]; then
    load_env "$env_file"
    loaded=$env_file
else
    loaded="none ($env_file does not exist; copy .env.example)"
fi

for var in APIFAST_DATA_DIR APIFAST_CONFIG_DIR; do
    eval "dir=\${$var:-}"
    case "$dir" in
        '' | /*) ;;
        *) export "$var=$root/$dir" ;;
    esac
done

profile=""
if [ "${1:-}" = "--release" ]; then
    profile="--release"
    shift
fi
case "${APIFAST_DEV_RELEASE:-}" in 1 | true | yes) profile="--release" ;; esac

what=${1:-gui}
[ $# -gt 0 ] && shift
case "$what" in
    gui) package=apifast-gui ;;
    cli) package=apifast-cli ;;
    *)
        echo "usage: tools/dev.sh [--release] [gui|cli] [arguments...]" >&2
        exit 2
        ;;
esac

{
    echo "dev.sh: env file: $loaded"
    echo "dev.sh: data: ${APIFAST_DATA_DIR:-the installed apifast's (~/.local/share/apifast)}"
    echo "dev.sh: settings: ${APIFAST_CONFIG_DIR:-the installed apifast's (~/.config/apifast)}"
} >&2
exec cargo run --quiet $profile --manifest-path "$root/Cargo.toml" -p "$package" -- "$@"
