#!/usr/bin/env bash
set -euo pipefail

usage() {
    cat <<'EOF'
Usage: ./scripts/release.sh [--no-push]

Creates an annotated git tag from the version in Cargo.toml.

Options:
  --no-push   Create the tag locally but do not push it to origin.
EOF
}

push_tag=true

while [[ $# -gt 0 ]]; do
    case "$1" in
        --no-push)
            push_tag=false
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            usage >&2
            exit 1
            ;;
    esac
    shift
done

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

if ! git diff --quiet || ! git diff --cached --quiet; then
    echo "Working tree must be clean before creating a release tag." >&2
    exit 1
fi

version="$(./scripts/get-version.sh)"
tag="v$version"

if git rev-parse "$tag" >/dev/null 2>&1; then
    echo "Tag $tag already exists." >&2
    exit 1
fi

git tag -a "$tag" -m "Release $tag"
echo "Created tag $tag"

if [[ "$push_tag" == true ]]; then
    git push origin "$tag"
    echo "Pushed $tag to origin"
else
    echo "Tag was created locally only"
fi