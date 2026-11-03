#!/usr/bin/env bash
# The Linux-side verification ladder (AGENTS.md §5). Every rung must pass.
# Usage: scripts/check-all.sh [--no-frontend] [--no-e2e]
set -euo pipefail
cd "$(dirname "$0")/.."

FRONTEND=1
E2E=1
for arg in "$@"; do
  case "$arg" in
    --no-frontend) FRONTEND=0 ;;
    --no-e2e) E2E=0 ;;
    *) echo "unknown option: $arg" >&2; exit 2 ;;
  esac
done

LINUX_CRATES=(-p stem-core -p stem-platform -p stem-service)

step() { printf '\n\033[1;34m==> %s\033[0m\n' "$*"; }

step "1/7 rustfmt"
cargo fmt --all --check

step "2/7 clippy (host target: core, platform mock, service)"
cargo clippy "${LINUX_CRATES[@]}" --all-targets -- -D warnings

step "3/7 tests (host target)"
cargo test "${LINUX_CRATES[@]}"

step "3b/7 TypeScript bindings are up to date"
cargo test -p stem-core --features ts --quiet >/dev/null
if ! git diff --quiet -- src/lib/ipc/generated; then
  echo "src/lib/ipc/generated/ changed after export; commit the regenerated bindings." >&2
  git --no-pager diff --stat -- src/lib/ipc/generated
  exit 1
fi

step "4/7 clippy (Windows target, whole workspace incl. Tauri app)"
cargo clippy --workspace --all-targets --target x86_64-pc-windows-msvc -- -D warnings

if [[ $FRONTEND -eq 1 && -f package.json ]]; then
  step "5/7 frontend: install, check, lint, test"
  pnpm install --frozen-lockfile
  pnpm check
  pnpm lint
  pnpm test
  step "6/7 frontend: production build"
  pnpm build
  if [[ $E2E -eq 1 ]]; then
    step "7/7 frontend: Playwright screenshots (mock mode)"
    pnpm e2e:screenshots
  fi
else
  echo "(frontend steps skipped)"
fi

printf '\n\033[1;32mAll Linux-side checks passed.\033[0m\n'
