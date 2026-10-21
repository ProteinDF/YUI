#!/usr/bin/env bash
# ブラウザ版（WASM）のリリースビルドを作り、配信できる形にまとめる。
#
#   scripts/build-web.sh [--public-url /prefix/]
#
# 出力（リポジトリの target/web/ の下）:
#   dist/                    index.html、.js、.wasm（静的に配信できる一式）と、その圧縮したもの（.gz、brotli があれば .br）
#   yui-web-<version>.zip    dist/ を zip にしたもの
#
# 配信のしかた（README の "Serving the web build" も参照）:
#   - バックエンドが、画面と API を 1 つのプロセスで出す（ローカルで使う）:
#       YUI_WEB_ROOT=target/web/dist cargo run --release -p yui-backend
#       → http://127.0.0.1:8080/
#   - 静的なファイルだけを置く（GitHub Pages など。バックエンドなし）:
#       dist/ の中身を置くだけ。サブフォルダに置くときは、--public-url /prefix/ を付けてビルドする。
#       バックエンドがないので、単独の動作（ファイルを File → Open で読み込む）になる。
#       WebGPU が使えるブラウザが必要で、https か localhost で開くこと。
#
# 注意: バックエンドは、1 人の利用者向け。すべての API が、起動ごとのランダムな秘密を求める（起動時に表示される
#   http://127.0.0.1:8080/#token=... を開く）。秘密を渡した相手は、ターミナル（任意のコマンドの実行）とファイルの
#   読み出しを使える。公開するなら、TLS を終端するリバースプロキシの後ろに置く（architecture.md 7.4節）。
set -euo pipefail

cd "$(dirname "$0")/.."
root="$(pwd)"

public_url=""
while [ $# -gt 0 ]; do
    case "$1" in
        --public-url)
            [ $# -ge 2 ] || { echo "build-web: --public-url needs a value" >&2; exit 2; }
            public_url="$2"
            shift 2
            ;;
        -h | --help)
            sed -n '2,22p' "$0"
            exit 0
            ;;
        *)
            echo "build-web: unknown argument '$1' (see --help)" >&2
            exit 2
            ;;
    esac
done

if ! command -v trunk > /dev/null 2>&1; then
    echo "build-web: trunk is not installed (cargo install trunk --locked)" >&2
    exit 1
fi
if ! command -v python3 > /dev/null 2>&1; then
    echo "build-web: python3 is needed to make the zip" >&2
    exit 1
fi

version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
out="$root/target/web"
dist="$out/dist"
rm -rf "$dist"
mkdir -p "$out"

echo "build-web: building yui-frontend $version (release; the first build takes a few minutes)"
args=(build --release --dist "$dist")
[ -n "$public_url" ] && args+=(--public-url "$public_url")
(cd frontend && trunk "${args[@]}" index.html)

zip_path="$out/yui-web-$version.zip"
rm -f "$zip_path"
python3 - "$dist" "$zip_path" << 'PY'
import os, sys, zipfile
src, dst = sys.argv[1], sys.argv[2]
with zipfile.ZipFile(dst, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as z:
    for name in sorted(os.listdir(src)):
        z.write(os.path.join(src, name), name)
PY

# 隣に圧縮したもの（.gz、brotli があれば .br）を置く。`backend` は、ブラウザが受け付ける方式のものを返す
# （zip は、元のファイルのまま。静的な置き場所は、たいてい自分で圧縮する）。
python3 - "$dist" << 'PY'
import gzip, os, sys
src = sys.argv[1]
for name in sorted(os.listdir(src)):
    if name.endswith((".wasm", ".js", ".html")):
        path = os.path.join(src, name)
        with open(path, "rb") as f:
            data = f.read()
        with open(path + ".gz", "wb") as f:
            f.write(gzip.compress(data, compresslevel=9, mtime=0))
PY
if command -v brotli > /dev/null 2>&1; then
    for f in "$dist"/*.wasm "$dist"/*.js "$dist"/*.html; do
        brotli --best --force "$f" -o "$f.br"
    done
else
    echo "build-web: brotli is not installed; only .gz files are made (sudo apt install brotli, for the smaller .br)"
fi

echo "build-web: done"
du -sh "$dist" | sed 's/^/  dist: /'
ls -l "$zip_path" | awk '{print "  zip:  " $5 " bytes  " $NF}'
echo "  serve with: YUI_WEB_ROOT=target/web/dist cargo run --release -p yui-backend"
