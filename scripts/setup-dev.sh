#!/usr/bin/env bash
# Prepare a Debian / Ubuntu machine for developing YUI: the system packages, the Rust
# components and target, trunk, Python 3.12 (uv) and the GitHub CLI. See CONTRIBUTING.md.
# It changes nothing outside the tools it installs, and it does not log in to GitHub.
#
# Usage:
#   scripts/setup-dev.sh [--skip-apt] [--check] [--dry-run]
#
#   --skip-apt  do not install system packages
#   --check     after setup, run scripts/check.sh --trunk (the pre-PR check)
#   --dry-run   print what would run, and change nothing
set -euo pipefail

cd "$(dirname "$0")/.."

SKIP_APT=0
CHECK=0
DRY_RUN=0
for arg in "$@"; do
    case "$arg" in
        --skip-apt) SKIP_APT=1 ;;
        --check) CHECK=1 ;;
        --dry-run) DRY_RUN=1 ;;
        -h | --help)
            sed -n '2,11p' "$0"
            exit 0
            ;;
        *)
            echo "unknown option: $arg" >&2
            exit 2
            ;;
    esac
done

# 実行する（--dry-run のときは表示だけ）
run() {
    if [[ $DRY_RUN -eq 1 ]]; then
        echo "[dry-run] $*"
    else
        "$@"
    fi
}

step() {
    echo
    echo "==> $*"
}

# 無いときは、入れ方を案内して止まる
need() {
    if ! command -v "$1" >/dev/null; then
        echo "$1 was not found. $2" >&2
        exit 1
    fi
}

# 系統のパッケージ（ビルド、GUI、Python の共有ライブラリの接続に使う）
PACKAGES=(build-essential pkg-config libxkbcommon-dev libwayland-dev libvulkan1 mesa-vulkan-drivers vulkan-tools)

step "1. System packages"
if [[ $SKIP_APT -eq 1 ]]; then
    echo "skipped (--skip-apt)"
elif ! command -v apt-get >/dev/null; then
    echo "apt-get was not found: this script is for Debian / Ubuntu." >&2
    echo "Install these packages by hand: ${PACKAGES[*]}" >&2
    exit 1
else
    run sudo apt-get update
    run sudo apt-get install -y --no-install-recommends "${PACKAGES[@]}"
fi

step "2. Rust (rustfmt, clippy, wasm32 target)"
need cargo "Install Rust with rustup first: https://rustup.rs"
run rustup component add rustfmt clippy
run rustup target add wasm32-unknown-unknown

step "3. trunk (the web build)"
if command -v trunk >/dev/null; then
    echo "trunk is already installed: $(trunk --version)"
else
    run cargo install trunk --locked
fi

step "4. Python 3.12 (uv; for the core-py tests and the plugin tests)"
need uv "Install uv first: https://docs.astral.sh/uv/"
run uv python install 3.12

step "5. GitHub CLI (gh)"
if command -v gh >/dev/null; then
    if gh auth status >/dev/null 2>&1; then
        echo "gh is installed and logged in."
    else
        echo "gh is installed but not logged in. Run: gh auth login"
    fi
else
    echo "gh was not found. Install it: https://cli.github.com/ (then: gh auth login)"
fi

if [[ $CHECK -eq 1 ]]; then
    step "6. Pre-PR check (scripts/check.sh --trunk)"
    if [[ $DRY_RUN -eq 1 ]]; then
        echo "[dry-run] scripts/check.sh --trunk (with PYO3_PYTHON and LD_LIBRARY_PATH set for Python 3.12)"
    else
        PY="$(uv python find 3.12)"
        LIBDIR="$("$PY" -c "import sysconfig; print(sysconfig.get_config_var('LIBDIR'))")"
        export PYO3_PYTHON="$PY"
        export LD_LIBRARY_PATH="$LIBDIR${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
        scripts/check.sh --trunk
    fi
fi

echo
echo "Done. Next: scripts/check.sh --trunk before every pull request (see CONTRIBUTING.md)."
