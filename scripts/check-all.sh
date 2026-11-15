#!/usr/bin/env bash
# The Linux-side verification ladder (AGENTS.md §5). Every rung must pass.
#
# Usage: scripts/check-all.sh [--no-frontend] [--no-e2e] [--no-install] [--ci]
#   --no-frontend  skip rungs 5-7 (frontend)
#   --no-e2e       skip rung 7 (Playwright screenshots)
#   --no-install   skip `pnpm install` (when it has already been run)
#   --ci           CI mode: implies --no-install when node_modules/ exists, and
#                  a missing package.json is an error instead of a skip
#
# .github/workflows/ci.yml runs these rungs as separate steps so a failure is
# attributable to one rung; keep the commands here and there in sync.
# scripts/check-all.ps1 is the Windows-host equivalent.
set -euo pipefail
cd "$(dirname "$0")/.."

usage() { sed -n '2,9p' "$0" | sed 's/^# \{0,1\}//'; }

FRONTEND=1
E2E=1
INSTALL=1
CI_MODE=0
for arg in "$@"; do
  case "$arg" in
    --no-frontend) FRONTEND=0 ;;
    --no-e2e) E2E=0 ;;
    --no-install) INSTALL=0 ;;
    --ci) CI_MODE=1 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown option: $arg" >&2; usage >&2; exit 2 ;;
  esac
done
if [[ $CI_MODE -eq 1 && -d node_modules ]]; then
  INSTALL=0
fi

LINUX_CRATES=(-p stem-core -p stem-platform -p stem-service)

step() { printf '\n\033[1;34m==> %s\033[0m\n' "$*"; }

step "1/7 rustfmt"
cargo fmt --all --check

step "2/7 clippy (host target: core, platform mock, service)"
cargo clippy "${LINUX_CRATES[@]}" --all-targets -- -D warnings

step "3/7 tests (host target)"
cargo test "${LINUX_CRATES[@]}"

step "3b/7 TypeScript bindings are up to date"
# The `ts` feature's tests export the bindings into src/lib/ipc/generated/
# (TS_RS_EXPORT_DIR in .cargo/config.toml). The files are tracked, so any
# modified, added or deleted file after the export means stale bindings.
cargo test -p stem-core --features ts --quiet
git rev-parse --is-inside-work-tree >/dev/null
if [[ -n "$(git status --porcelain -- src/lib/ipc/generated)" ]]; then
  echo "src/lib/ipc/generated/ changed after export; run 'cargo test -p stem-core --features ts' and commit the result:" >&2
  git --no-pager status --short -- src/lib/ipc/generated >&2
  git --no-pager diff -- src/lib/ipc/generated
  exit 1
fi

step "3c/7 settings reference (docs/SETTINGS.md) is up to date"
# crates/stem-core/tests/settings_reference.rs rewrites docs/SETTINGS.md from
# the doc comments and defaults in crates/stem-core/src/settings.rs when
# STEM_UPDATE_SETTINGS_REFERENCE=1 (without it, rung 3 already fails on a
# stale page). The file is tracked, so a change after the rewrite means it was
# stale or never committed.
STEM_UPDATE_SETTINGS_REFERENCE=1 cargo test -p stem-core --test settings_reference --quiet
if [[ -n "$(git status --porcelain -- docs/SETTINGS.md)" ]]; then
  echo "docs/SETTINGS.md changed after regeneration; run 'STEM_UPDATE_SETTINGS_REFERENCE=1 cargo test -p stem-core --test settings_reference' and commit the result:" >&2
  git --no-pager status --short -- docs/SETTINGS.md >&2
  git --no-pager diff -- docs/SETTINGS.md
  exit 1
fi

step "4/7 clippy (Windows target, whole workspace incl. Tauri app)"
# tauri-build compiles a Windows resource file in its build script; on a
# non-Windows host that needs `llvm-rc` on PATH (apt/brew package `llvm`) or
# RC_x86_64_pc_windows_msvc pointing at it, otherwise it panics with
# NotAttempted("llvm-rc").
if [[ "$(uname -s)" != MINGW* && "$(uname -s)" != MSYS* ]] \
  && ! command -v llvm-rc >/dev/null 2>&1 \
  && [[ -z "${RC_x86_64_pc_windows_msvc:-}" ]]; then
  echo "rung 4 needs llvm-rc: install the 'llvm' package or set RC_x86_64_pc_windows_msvc=/path/to/llvm-rc-NN" >&2
  exit 1
fi
cargo clippy --workspace --all-targets --target x86_64-pc-windows-msvc -- -D warnings
# The diagnostic probes (feature `probes`) are not in the line above:
# their examples need the feature (`required-features`), so lint them here.
cargo clippy -p stem-platform --all-targets --features probes --target x86_64-pc-windows-msvc -- -D warnings
# Their unit tests (module windows::probe) run only on a Windows host: the
# module is Windows-only, and rung 3 runs without the feature.
case "$(uname -s)" in
  MINGW* | MSYS*) cargo test -p stem-platform --features probes --lib windows::probe:: ;;
  *) echo "(probe unit tests skipped: they need a Windows host)" ;;
esac

if [[ $FRONTEND -eq 0 ]]; then
  echo "(frontend rungs 5-7 skipped: --no-frontend)"
elif [[ ! -f package.json ]]; then
  if [[ $CI_MODE -eq 1 ]]; then
    echo "package.json not found; frontend rungs 5-7 cannot run" >&2
    exit 1
  fi
  echo "(frontend rungs 5-7 skipped: no package.json yet)"
else
  step "5/7 frontend: install, check, lint, test"
  if [[ $INSTALL -eq 1 ]]; then
    pnpm install --frozen-lockfile
  else
    echo "(pnpm install skipped)"
  fi
  pnpm check
  pnpm lint
  pnpm test

  step "6/7 frontend: production build"
  pnpm build

  if [[ $E2E -eq 1 ]]; then
    step "7/7 frontend: Playwright screenshots (mock mode)"
    pnpm e2e:screenshots
  else
    echo "(rung 7 skipped: --no-e2e)"
  fi
fi

printf '\n\033[1;32mAll Linux-side checks passed.\033[0m\n'
