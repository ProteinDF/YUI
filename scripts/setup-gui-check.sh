#!/usr/bin/env bash
# Prepare a real Linux desktop (Debian / Ubuntu) for checking the YUI GUI, such as
# View > Plugins... (see TESTING.md). It does not touch your own settings.
#
#   1. installs the system packages (needs sudo)
#   2. shows whether a Vulkan GPU driver is visible
#   3. builds yui-frontend (with --with-python: against Python 3.12 with libpython)
#   4. creates ~/yui-check/config with the example plugin enabled, for XDG_CONFIG_HOME
#
# Usage:
#   scripts/setup-gui-check.sh [--with-python] [--skip-apt] [--dry-run]
#
#   --with-python  build with --features python (needs uv; uses Python 3.12)
#   --skip-apt     do not install system packages
#   --dry-run      print what would run, and change nothing
set -euo pipefail

cd "$(dirname "$0")/.."

WITH_PYTHON=0
SKIP_APT=0
DRY_RUN=0
for arg in "$@"; do
    case "$arg" in
        --with-python) WITH_PYTHON=1 ;;
        --skip-apt) SKIP_APT=1 ;;
        --dry-run) DRY_RUN=1 ;;
        -h | --help)
            sed -n '2,15p' "$0"
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

PACKAGES=(pkg-config libxkbcommon-dev libwayland-dev libvulkan1 mesa-vulkan-drivers vulkan-tools)

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

step "2. GPU (Vulkan)"
if [[ $DRY_RUN -eq 1 ]]; then
    echo "[dry-run] vulkaninfo --summary"
elif command -v vulkaninfo >/dev/null && VULKAN_INFO="$(vulkaninfo --summary 2>/dev/null || true)" && grep -q deviceName <<<"$VULKAN_INFO"; then
    # 出力を変数に入れてから調べる（grep -q が先に終わると、pipefail で誤って失敗になるため）
    grep deviceName <<<"$VULKAN_INFO"
else
    echo "WARNING: no Vulkan device was found. The GUI cannot start without a GPU driver." >&2
    echo "  Intel / AMD: mesa-vulkan-drivers. NVIDIA: the proprietary driver. See TESTING.md." >&2
fi

step "3. Rust"
if ! command -v cargo >/dev/null; then
    echo "cargo was not found. Install Rust with rustup: https://rustup.rs" >&2
    exit 1
fi

step "4. Build"
if [[ $WITH_PYTHON -eq 1 ]]; then
    if ! command -v uv >/dev/null; then
        echo "uv was not found (needed to get Python 3.12 with libpython): https://docs.astral.sh/uv/" >&2
        exit 1
    fi
    PY="$(uv python find 3.12)"
    LIBDIR="$("$PY" -c "import sysconfig; print(sysconfig.get_config_var('LIBDIR'))")"
    export PYO3_PYTHON="$PY"
    export LD_LIBRARY_PATH="$LIBDIR${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
    echo "Python: $PY"
    echo "When you run the app, keep LD_LIBRARY_PATH=$LIBDIR"
    run cargo build -p yui-frontend --features python
else
    run cargo build -p yui-frontend
fi

step "5. Check folder (~/yui-check, not your own settings)"
CHECK="$HOME/yui-check/config"
if [[ $DRY_RUN -eq 1 ]]; then
    echo "[dry-run] create $CHECK/yui/plugins, copy examples/plugins/hello-yui, write settings.yaml"
else
    mkdir -p "$CHECK/yui/plugins"
    rm -rf "$CHECK/yui/plugins/hello-yui"
    cp -r examples/plugins/hello-yui "$CHECK/yui/plugins/"
    printf 'plugins:\n  enabled: [hello-yui]\n' > "$CHECK/yui/settings.yaml"
fi

echo
echo "Done. Start the app with the check folder:"
echo "  XDG_CONFIG_HOME=\"$CHECK\" ./target/debug/yui-frontend"
echo "Then open View > Plugins... and check the list: the state of hello-yui, the layout, and Disable."
