#!/usr/bin/env bash
# Completion gate for every change (run it before opening a pull request).
# Runs the checks required by ROADMAP.md §1 and prints a summary table.
#
# Usage: scripts/check.sh [--quick] [--no-lint] [--trunk]
#   --quick    skip `cargo test --workspace`
#   --no-lint  skip `cargo fmt` and `cargo clippy`
#   --trunk    also run `trunk build` for the WASM frontend (requires trunk)
set -uo pipefail
cd "$(dirname "$0")/.."

quick=0 no_lint=0 trunk=0
for arg in "$@"; do
    case "$arg" in
        --quick)   quick=1 ;;
        --no-lint) no_lint=1 ;;
        --trunk)   trunk=1 ;;
        -h|--help) sed -n '2,10p' "$0"; exit 0 ;;
        *) echo "unknown option: $arg" >&2; exit 2 ;;
    esac
done

results=()
failed=0

run() {
    local name="$1"; shift
    echo
    echo "==> [$name] $*"
    if "$@"; then
        results+=("PASS  $name")
    else
        results+=("FAIL  $name")
        failed=1
    fi
}

run "cargo check (native)" cargo check --workspace --all-targets
run "cargo check (wasm32)" cargo check --target wasm32-unknown-unknown -p yui-frontend
if [[ $quick -eq 0 ]]; then
    run "cargo test" cargo test --workspace
fi
if [[ $no_lint -eq 0 ]]; then
    run "cargo fmt" cargo fmt --all -- --check
    run "cargo clippy (native)" cargo clippy --workspace --all-targets --keep-going -- -D warnings
    run "cargo clippy (wasm32)" cargo clippy --target wasm32-unknown-unknown -p yui-frontend -- -D warnings
fi
if [[ $trunk -eq 1 ]]; then
    run "trunk build" bash -c 'cd frontend && trunk build index.html'
fi
run "EN/JA doc sync" scripts/check-doc-sync.sh

echo
echo "==================== summary ===================="
printf '%s\n' "${results[@]}"
exit $failed
