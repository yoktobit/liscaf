#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cargo_toml="$repo_root/Cargo.toml"

version="$(sed -nE 's/^version = "([^"]+)"/\1/p' "$cargo_toml" | head -n 1)"

if [[ -z "$version" ]]; then
    echo "Failed to read package version from $cargo_toml" >&2
    exit 1
fi

printf '%s\n' "$version"