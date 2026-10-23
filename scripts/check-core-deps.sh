#!/usr/bin/env bash
# Checks that focus-core stays pure: no workspace/path/git dependencies,
# only allowlisted direct dependencies, and no I/O crates anywhere in its tree.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# Reads a list file, dropping comments, whitespace and blank lines.
read_list() {
  sed -e 's/#.*//' -e 's/[[:space:]]//g' "$1" | grep -v '^$' || true
}

# Prints "name source" for each package in focus-core's tree, root excluded.
core_tree() {
  cargo tree --manifest-path "$root/Cargo.toml" -p focus-core \
    -e normal,build --all-features --target all --prefix none --format '{p}' "$@" |
    tr -d '\r' | sed -e 's/ (\*)$//' | grep -v -e '^$' -e '^\[' | tail -n +2
}

allowed="$(read_list "$root/scripts/focus-core-allowed-deps.txt")"
denied="$(read_list "$root/scripts/focus-core-denied-deps.txt")"
# Captured up front, not via process substitution, so a cargo failure fails the script.
direct="$(core_tree --depth 1 | sort -u)"
full="$(core_tree | sort -u)"
status=0

while IFS= read -r line; do
  [[ -z "$line" ]] && continue
  name="${line%% *}"
  if [[ "$line" == *"("* ]]; then
    echo "error: focus-core depends on a path or git crate: $line" >&2
    status=1
  elif ! grep -qxF "$name" <<<"$allowed"; then
    echo "error: focus-core depends on '$name', which is not in scripts/focus-core-allowed-deps.txt" >&2
    status=1
  fi
done <<<"$direct"

while IFS= read -r line; do
  [[ -z "$line" ]] && continue
  name="${line%% *}"
  if grep -qxF "$name" <<<"$denied"; then
    echo "error: focus-core pulls in I/O crate '$name' (transitively)" >&2
    status=1
  fi
done <<<"$full"

if [[ $status -eq 0 ]]; then
  echo "focus-core dependencies OK"
fi
exit "$status"
