#!/usr/bin/env bash
# このマシンの OS 向けのインストーラーを、cargo-packager で作る。
#
#   scripts/build-installers.sh [formats]     # 例: deb,appimage（省略すると、OS の既定）
#
# 設定は frontend/Cargo.toml の [package.metadata.packager]。リリースビルドも、その中で行う
# （beforePackagingCommand）。出力は target/packager/ の下:
#   Linux: yui-frontend_<version>_amd64.deb、yui-frontend_<version>_x86_64.AppImage
#   macOS: .dmg    Windows: NSIS の setup .exe
# 各 OS のインストーラーは、その OS の上でしか作れない。3 つの OS を作るには、GitHub の
# 手動実行のワークフロー .github/workflows/release-installers.yml を使う（すべて作ると時間が
# かかるので、手動実行だけ）。署名と公証はしていない（README の "Installing" を参照）。
set -euo pipefail

cd "$(dirname "$0")/.."

if ! cargo packager --version > /dev/null 2>&1; then
    echo "build-installers: cargo-packager is not installed (cargo install cargo-packager --locked)" >&2
    exit 1
fi

args=(--release)
[ $# -ge 1 ] && args+=(-f "$1")

echo "build-installers: packaging yui $(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1) (the first release build takes a few minutes)"
cargo packager "${args[@]}"

echo "build-installers: done"
ls -l target/packager/ | awk 'NR > 1 && $NF !~ /^\./ {print "  " $5 " bytes  target/packager/" $NF}'
