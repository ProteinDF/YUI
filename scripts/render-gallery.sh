#!/usr/bin/env bash
# 描画のギャラリーを、画面なしで描いて PNG に書き出す（見た目の確認の自動化）。
#
# 使い方: scripts/render-gallery.sh [出力先]    （出力先の既定: target/gallery）
#
# 機能ごとの場面（表現、分子表面、相互作用、CUBE、書き出しの設定、アニメーションのフレーム）を
# 描き、各場面の PNG、グループごとのコンタクトシート（sheet_*.png）、一覧（manifest.txt）を書く。
# 各場面で「描かれていること」を確かめ、失敗すれば非 0 で終わる。画像は人（や AI）が見て確かめる。
# wgpu のアダプターが必要（GPU がなければ、Mesa のソフトウェア描画で動く。TESTING.md の 7 章）。
# アダプターが見つからないときは、読み飛ばして 0 で終わる。
set -euo pipefail
cd "$(dirname "$0")/.."
out="${1:-target/gallery}"
mkdir -p "$out"
YUI_GALLERY_DIR="$(cd "$out" && pwd)" \
  cargo test -p yui-frontend gallery_renders_all_scenes -- --ignored --nocapture
echo "gallery: $out (see manifest.txt and sheet_*.png)"
