#!/usr/bin/env bash

set -euo pipefail

repo_root="$(git rev-parse --show-toplevel 2>/dev/null)" || {
    echo "[pre-commit] not inside a Git repository" >&2
    exit 1
}
source_hook="$repo_root/scripts/pre-commit"
hook_dir="$(git -C "$repo_root" rev-parse --git-path hooks)"
target_hook="$hook_dir/pre-commit"

if [[ ! -f "$source_hook" ]]; then
    echo "[pre-commit] missing $source_hook" >&2
    exit 1
fi

mkdir -p "$hook_dir"
if [[ -f "$target_hook" ]]; then
    timestamp="$(date +%Y%m%d_%H%M%S)"
    backup="$target_hook.backup.$timestamp"
    cp "$target_hook" "$backup"
    echo "[pre-commit] backed up existing hook to $backup"
fi

cp "$source_hook" "$target_hook"
chmod +x "$target_hook"

echo "[pre-commit] installed fast M0 guard:"
echo "  cargo fmt --all -- --check"
echo "  cargo check --locked --workspace"
