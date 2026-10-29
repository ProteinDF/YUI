# YUI — 次世代大規模分子軌道・構造可視化プラットフォーム

[![CI](https://github.com/ProteinDF/YUI/actions/workflows/ci.yml/badge.svg)](https://github.com/ProteinDF/YUI/actions/workflows/ci.yml)

*[English version](README.md)*

「結 (YUI)」は、タンパク質超並列大規模量子化学計算ソフトウェア（ProteinDF/QCLObot）の計算結果および分子構造を、Webブラウザおよびネイティブ環境で高速に可視化・制御・共有するためのデータ駆動型マルチモーダルプラットフォームです。詳細な設計は [`architecture.ja.md`](./architecture.ja.md)、段階的な実装計画は [`ROADMAP.ja.md`](./ROADMAP.ja.md)、現状の機能を実際に試す手順は [`TESTING.ja.md`](./TESTING.ja.md) を参照してください。

---

## 🎯 コア哲学 & ターゲット
- **Data-Oriented MVI (Model-View-Intent):** すべての状態は `AppState` に集約され、一方向のデータフローで駆動します。`iced` の Elm Architecture がこれと直接対応します。
- **Single Source of Truth (SSOT):** 中間保存・通信は高速な **MessagePack**、人間によるインポート/エクスポートは **YAML**（相対パス解決）で行います。
- **Fearless Concurrency:** CPUマルチコア（Rayon/Tokio）による超並列パースと、GPU駆動レンダリング（WebGPU/Compute Shader + Indirect Draw）により、**最大約100万原子**規模の複合体を実用的なフレームレートで描画します。QM計算対象自体は数千〜数万原子規模ですが、それをクライオEM複合体等の巨大な構造的コンテキストの中に位置づけて表示することを主要ユースケースとします。
- **正準分子軌道計算の可視化研究支援:** MOブラウザ、軌道別ポピュレーション/寄与マッピング、QCLO成長ステップ間の収束・忠実度比較、複数計算の比較（差分表示）など、ProteinDF/QCLObotによる正準MO計算そのものの研究を支援する機能を備えます。
- **Collaboration:** 発表者(Host)が主導権を持ち、聴講者（Viewer）に操作権を委譲できる「プレゼンモード」をネイティブサポートします。
- **モデリング連携:** 力場最適化・ドッキング等は内製せず、設定可能な外部ツール（OpenBabel/RDKit等、ローカル実行 or HTTP API）と連携します。
- **論文品質の出力:** ベクター(SVG)出力に加え、SSAO→将来的なパストレースによるオフライン高解像度レンダリングをサポートします。
- **ライセンス:** GPLv3。

---

## 🛠 技術スタック
- **Core Language:** Rust (Latest Stable, Wasm & Native Core)
- **Graphics API:** `wgpu` (WebGPU / Vulkan / Metal / DX12 backend)
- **GUI Engine:** `iced` (Elm Architecture, `pane_grid` による VSCode-like ドッキングUI)
- **構造データI/O:** `proteindf-bridge`（外部crate。`ProteinDF_bridge`(Python/GPLv3) のRust 1:1移植。C/C++/Python バインディングも提供）
- **Serialization:** `serde`, `rmp-serde` (MessagePack), `serde_yaml_ng`
- **Concurrency:** `tokio` (Async I/O / Streaming), `rayon` (Parallel Parsing)
- **Scripting & AI:** `PyO3`（ネイティブPython）/ `Pyodide`（Wasm-Python）、Web Speech API + AIチャットアシスタント（Claude/Gemini/ローカルLLM（Ollama経由）を差し替え可能）
- **通信:** WebSocket（初期実装）→ 将来的に WebTransport(QUIC) へ移行可能なトランスポート抽象化

---

## 🔢 バージョニング

ProteinDF/QCLObot/ProteinDF_bridge 等の姉妹プロジェクトと同じ **CalVer方式（`年.月.リリース番号`）** を採用します。例: `2026.9.0` は「2026年9月の最初のリリース」を意味します。同一月内での追加リリースはリリース番号を `2026.9.1`, `2026.9.2` ... と増やします。SemVer（`MAJOR.MINOR.PATCH`）ではないため、破壊的変更の有無はリリースノートで確認してください。

各クレートのバージョンは、ルート `Cargo.toml` の `[workspace.package] version` から `version.workspace = true` で継承しています。

---

## 🖱 操作

同じ一覧が、アプリの **Help → Mouse & Keyboard...** にもあります。

**3D ビューポート（マウス）**
- 左ドラッグ: 中心のまわりを回す
- Shift + 左ドラッグ: 平行移動
- Ctrl（macOS は Cmd）+ 左ドラッグ: 拡大・縮小（下へドラッグで縮小）。マウスホイールでも拡大・縮小
- 原子を左クリック: その原子だけを選ぶ
- Shift + クリック: 原子を選択に追加
- Ctrl（macOS は Cmd）+ クリック: 原子を選択に対して切り替え（選ばれていれば外す）
- 原子をダブルクリック: その残基全体を選ぶ
- 何もないところをクリック: 選択を外す（Shift か Ctrl と一緒なら、選択は残る）
- 右クリック: コンテキストメニュー

**キーボード**
- Space: 再生 / 一時停止
- 左 / 右矢印: 1 フレーム戻る / 進む
- Esc: 選択を外す
- Ctrl+S（macOS は Cmd+S）: 開いているテキストファイルを保存

**ターミナル**
- 画面をクリック: ターミナルへ入力（枠が水色になる）。ほかの場所をクリックすると戻る
- Ctrl+C: 実行中のプログラムを中断
- Ctrl+Shift+C / V（macOS は Cmd+C / V）: コピー / 貼り付け。Shift+Insert でも貼り付け
- マウスホイール、Shift+PageUp / PageDown: 履歴をスクロール

---

## 📂 プロジェクト構成
```text
yui/
├── Cargo.toml              # Workspace構成（[workspace.package] version = "2026.10.10" 等を一元管理）
├── core/                   # [Shared] AppState定義、シリアライズ、proteindf-bridge連携
│   └── src/
├── frontend/               # [Wasm/Browser] iced + wgpu フロントエンド
│   └── src/
├── backend/                # [Server] Axum製 REST/WebSocket API サーバー
│   └── src/
└── renderer-native/        # [Native/CLI] ヘッドレスwgpuによる高解像度/ベクター/オフライン出力
    └── src/

# 外部依存（別プロジェクト・別リポジトリで管理）
proteindf-bridge/                 # Rust製構造I/O・モデリング前処理基盤 (C/C++/Pythonバインディング付き)
```

---

## 🏃 ビルド & 実行
 
すべての検査とテストは、手元で `scripts/check.sh --trunk` を実行して行います（trunk は一度だけ `cargo install trunk --locked` で入れてください）。GitHub Actions の CI と同じ検査です。CI は、pull request ごとと、`main` / `develop` への push で動きます。貢献の手順は [CONTRIBUTING.ja.md](CONTRIBUTING.ja.md) を見てください。

エディタのフォントなど、利用者ごとの設定は、OS の設定フォルダの `settings.yaml`（Linux では `~/.config/yui/`）にあります。アプリの **File → Open Settings File** で作成・編集できます。設定は起動時に読むので、編集したら YUI を再起動してください。詳しくは `architecture.md` 8.6節を参照してください。

```bash
# ワークスペース全体のテスト
cargo test

# スタンドアロンのデスクトップアプリとして実行（backend不要）
cargo run --bin yui-frontend

# ヘッドレスレンダラー（YAML状態ファイルの読み込み確認）
cargo run --bin yui-renderer-native -- path/to/state.yaml
```

### `backend` と `frontend` を連携させて実行する（コラボレーションモード）

`yui-backend` は REST/WebSocket APIサーバー専用で、`/` へのルートは実装されていません。そのため
`http://127.0.0.1:8080/` をブラウザで直接開くと、仕様どおり404になります。実際に画面を伴って動かすには
`frontend`（ネイティブ or Web）と組み合わせる必要があります。

```bash
# 1. APIサーバーを起動（http://127.0.0.1:8080 で待受）
cargo run --bin yui-backend

# 2a. ネイティブのデスクトップアプリから接続する場合
cargo run --bin yui-frontend -- --collab ws://127.0.0.1:8080/api/collab
```

### Webブラウザ向けビルド（Trunk使用）

```bash
# 初回のみ
rustup target add wasm32-unknown-unknown
cargo install trunk

cd frontend
trunk serve --port 8085 index.html
```

ブラウザで `http://127.0.0.1:8085/` を開いてください。ページは、自分を配信したサーバーの `/api/collab` に接続します。
`trunk serve` は `/api/` を `backend` に転送する（`frontend/Trunk.toml`）ので、`backend`（8080 番）を起動してあれば
接続され、なければスタンドアロンで動きます。別の backend には、URL に `?collab=ws://host:port` を付けて指定できます。
**`frontend/dist/index.html` を `file://` で直接開かないでください** — ES moduleと`.wasm`の読み込みが
CORSでブロックされ、画面が真っ白になります。

描画には**実GPUのハードウェアアクセラレーションを伴うWebGPU対応ブラウザ**が必要です。ソフトウェア/フォールバック
WebGPUアダプタしか使えない環境（VM、リモートデスクトップ、ヘッドレス環境など）では、無言で真っ白になる代わりに
診断用のエラーオーバーレイが表示されるようになっています。

#### Web 版の配信（リリースビルド）

```bash
scripts/build-web.sh                 # リリースビルド → target/web/dist と target/web/yui-web-<version>.zip
YUI_WEB_ROOT=target/web/dist cargo run --release -p yui-backend
```

`backend` が、画面と API を 1 つのプロセスで配信します。`http://127.0.0.1:8080/` を開いてください（`?collab=` も、
2 つ目のサーバーも要りません）。zip には同じファイル（`index.html`、`.js`、`.wasm`。約 14 MB、zip で 6 MB）が
入っていて、任意の静的ホスト（GitHub Pages など）に置けます（ルート以外に置くなら、`--public-url /sub/path/` を付けて
ビルド）。`dist/` には、各ファイルの隣に `.gz`（ビルド時に `brotli` が入っていれば `.br` も）もあり、`backend` は、ブラウザが
受け付ける方式のものを返します（`.wasm` は 13.7 MB、`.gz` で 6.3 MB。zip は、圧縮前のファイルのままです）。`backend` がなければ、ページはスタンドアロンです（File → Open でワークスペースを開く）。
実 GPU 上の WebGPU に対応したブラウザと、安全なコンテキスト（`https://` か `localhost`）が必要です。

**`backend` の秘密を守ってください。** すべての API が、起動ごとのランダムな秘密を求め
（設定フォルダの `backend.json` に書かれ、Web 版のアドレスと一緒に表示されます）、ほかの Web サイトからの要求は断り、
`YUI_BIND` で変えない限り `127.0.0.1` にバインドします（変えるには `YUI_BEHIND_TLS_PROXY=1` も要り、ターミナルと
ファイルの API は、`YUI_ENABLE_TERMINAL=1` / `YUI_ENABLE_FS=1` がない限り閉じたままです）。セッションの共有は
`scripts/yui-invite.py create viewer` で行います（期限つきで取り消せる Viewer / Editor の秘密で、ターミナルを開くことも、
状態を変えることもできません）。*Host* の秘密を渡した相手は、ターミナル（任意のコマンドの実行）を開き、ワークスペースの
ルートの下のファイルを読めます。`architecture.ja.md` 7.4節を参照してください。

### Windows でのビルド（ビルドと起動を確認）

ネイティブ版は MSVC のツールチェーンでビルドします。Windows で、ビルドと起動を確認しました。そのほかの機能（プラグイン、インストーラー、ターミナル）は、Windows ではまだ確認していません。問題があれば知らせてください。

1. [Rust](https://rustup.rs) を、既定の MSVC のツールチェーンで入れます。あわせて、Visual Studio Build Tools の「C++ によるデスクトップ開発」を入れます。
2. Git を入れます（`proteindf-bridge` の依存を GitHub から取得するため）。
3. PowerShell で、次を実行します。
   ```powershell
   git clone https://github.com/ProteinDF/YUI.git
   cd YUI
   git checkout develop
   cargo run --bin yui-frontend
   ```
4. GPU は Vulkan か DirectX 12 が使えれば足ります。新しいグラフィックスドライバーで足ります。
5. 利用者ごとの設定とプラグインは、`%APPDATA%\yui\`（`settings.yaml`、`plugins\`）にあります。
6. セットアップの `.exe` を作る場合は、`cargo install cargo-packager --locked` のあと、`cargo packager --release -f nsis` を実行します。結果は `target\packager\` に出ます。`scripts/build-installers.sh` は bash 用なので、PowerShell では `cargo packager` を直接実行します。

### インストーラー（ネイティブ）

```bash
scripts/build-installers.sh          # このマシンの OS 向け: Linux は .deb と .AppImage、macOS は .dmg、Windows は NSIS の setup .exe
```

[cargo-packager](https://github.com/crabnebula-dev/cargo-packager) で作ります（`cargo install cargo-packager --locked`。設定は
`frontend/Cargo.toml` の `[package.metadata.packager]`）。結果は `target/packager/` に出ます。インストーラーは、その OS の
上でしか作れません。3 つの OS 向けを作るには、GitHub のワークフロー **Release installers**
（`.github/workflows/release-installers.yml`）を使います。すべて作ると時間がかかるので、
手動実行だけです。Actions タブの "Run workflow"、または `gh workflow run release-installers.yml --ref <タグ>`。
インストーラーは、ワークフローの artifacts として出ます。

**Mac でのローカルのビルド。** cargo-packager を一度インストールしてから、Mac で dmg を作ります。
1. `cargo install cargo-packager --locked`（初回は時間がかかる）。`cargo packager` は cargo の標準のコマンドではない。この手順を飛ばすと「no such command: `packager`」と出る。
2. リポジトリの直下で、`develop` に切り替えて最新にする: `git checkout develop && git pull`。
3. `scripts/build-installers.sh dmg`（または `cargo packager --release -f dmg`）。`.dmg` は `target/packager/` に出る。
4. アイコンを確かめるには、`.dmg` を開いて YUI を Applications にドラッグし、Dock と Finder を見る。アイコンがなければ、`YUI.app` の `Contents/Resources/` に `icon.icns` があるかを確かめる。

パッケージには、`yui-frontend`（アプリ。メニューなどでは「YUI」）と、任意の `yui-backend` が入っています。

**インストール。** インストーラーは**署名も公証もしていない**ので、最初の起動で警告が出ます。
- *macOS:* `.dmg` を開き、YUI を Applications にドラッグし、アプリを右クリック →「開く」（または
  `xattr -dr com.apple.quarantine /Applications/YUI.app`）。
- *Windows:* SmartScreen が「Windows によって PC が保護されました」と出る →「詳細情報」→「実行」。
- *Linux:* `sudo apt install ./yui-frontend_<version>_amd64.deb`（Vulkan か OpenGL のドライバーが要る）、または
  `chmod +x yui-frontend_<version>_x86_64.AppImage && ./yui-frontend_<version>_x86_64.AppImage`。

### ローカルの `proteindf-bridge` で上書き開発する場合

本リポジトリは既定で GitHub 上の Git リリースタグ（`tag = "2026.10.20"`）から `proteindf-bridge` を取得します。ローカルマシン上で `proteindf-bridge` を同時に変更しながら開発したい場合は、Cargo の `[patch]` 機能を利用して差し替えることができます。

各自のユーザー単位設定（例: `~/.cargo/config.toml`）に以下を記述するか、`cargo` 実行時に `--config` オプションで一時的に指定してください:

```toml
[patch."https://github.com/ProteinDF/ProteinDF_bridge.git"]
proteindf-bridge = { path = "/absolute/path/to/ProteinDF_bridge/rust/crates/proteindf-bridge" }
```

> **注意:** リポジトリの可搬性を維持するため、本リポジトリ内の `.cargo/config.toml` や `Cargo.toml` に手元の絶対パスを直接書き込んでコミットしないでください（AGENTS.md の相対パス規約）。必ずリポジトリ外の設定ファイルまたは一時的なオプションを使用してください。

詳しい手順（コラボレーションのdelta同期、Log Viewer、Terminalなど）は [`TESTING.ja.md`](./TESTING.ja.md) を参照してください。
