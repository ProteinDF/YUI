# YUI — 開発ロードマップ

*[English version](ROADMAP.md)*

本ドキュメントは、次にこのプロジェクトを実装する担当者（人間の貢献者、またはAIコーディングエージェント）向けの引き継ぎブリーフである。「何を・なぜ作るか」の正本は `architecture.md` であり、本ドキュメントは、現時点でリポジトリに何が既にあるかを踏まえた上での「どの順序で・どのようなルールの下で作るか」を扱う。

---

## 1. 基本ルール（担当するエージェント/ツールによらず適用する）

- **`architecture.md` を正とする。** 実装上の詳細がそこに書かれていない場合は、独断で別設計を決めるのではなく `architecture.md`（および `architecture.ja.md`）への追記を提案すること。
- **`AGENTS.md` の運用ルールは、使用するAIツールによらず適用される。** 人間の開発者への応答は必ず日本語で行う、ファイルパスは常に相対パスを用いる（絶対パスや `file:///` 形式のリンクは使わない）、バージョニングはCalVer（`年.月.リリース番号`）に従う、利用者から見える変更は `CHANGELOG.md` / `CHANGELOG.ja.md`（`Unreleased`）に書く。貢献の手順は `CONTRIBUTING.md` にある。
- **新規DSLを作らない。** 6.2節で既に、描画・選択のための独自言語は作らないと決定済みであり、表現・選択はPython API（`Selector`/`Representation`）駆動とする。14.3節はAIチャットアシスタントのツール呼び出しにも同じ方針を適用している。
- **力場最適化・ドッキング・DSSP相当の二次構造推定・非共有結合相互作用検出を本リポジトリ内で自前実装しない。** これらは別プロジェクト `proteindf-bridge`（10章）の管轄である。`proteindf-bridge` がまだ存在しない間の進め方は下記2節を参照。
- **各フェーズ/タスクの完了条件:** `cargo check --workspace`、`cargo test --workspace`、`cargo fmt --all -- --check`、および clippy の `-D warnings`（`scripts/check.sh` による全件検査）が通ること。PR を作る前に手元で `scripts/check.sh --trunk` を実行して通り、その summary を PR に書くこと（GitHub Actions の CI も、PR ごとに同じ検査を行う）。設計判断を行った・変更した場合は同じ変更の中で `architecture.md`/`architecture.ja.md` を更新すること。
- **commitメッセージは英語とする**（どのツールが作成したcommitであっても）。
- **ブランチ運用はgit-flowとする。** `main` はリリース済み・タグ付けされた状態（CalVer、例: `2026.9.0`）のみを保持し、`release/*` または `hotfix/*` ブランチのマージによってのみ更新される — フィーチャー作業を `main` に直接pushしないこと。`develop` は統合ブランチであり、すべての `feature/*` ブランチ（例: `feature/phase-5b1-spatial-partitioning`）は `develop` から作成し、PRも `main` ではなく `develop` を対象とする。リリース準備の際は `develop` から `release/<CalVer>` を切り、そこで仕上げた上で `main`（タグ付け）と `develop` の両方にマージする。リリース後の緊急修正は `main` から切った `hotfix/*` を用い、`main`（新しいリリースとして）と `develop` の両方にマージする。`AGENTS.md`/`ROADMAP.md`/`ROADMAP.ja.md` は `develop` 上で更新し続ける生きたドキュメントであり、`main` 側の内容は直近のリリース時点のスナップショットを反映する。

---

## 2. 現状（2026-10-30時点）

| クレート | 状態 | 備考 |
| --- | --- | --- |
| `core` | 動作可能（フェーズ1, 5a, 5b-i, 6e, 6i-i, 6i-ii完了） | 状態管理（`state.rs`, `yaml.rs`, `packing.rs`, `trajectory.rs`, `collab.rs`, `role.rs`, `export.rs`, `interaction.rs`）。Rust struct ⇄ MsgPack（非圧縮およびzstd） ⇄ YAML の双方向ロスレス往復テスト済み。フェーズ6eで本物の `proteindf_bridge::AtomGroup` を `rmpv` 経由の手動serdeラッパーで統合。BVH空間分割構造（`spatial.rs`、`AtomBvh`）、構造化データ型 `Representation`（`representation.rs` にて `RepresentationKind::Interaction` および `interaction_kinds` をサポート）、二次構造サイドマップを実装済み。`proteindf-bridge` による実PDB/mmCIFファイルの読み込みを検証完了。 |
| `core-py` | 動作可能（フェーズ3, 6e完了） | `PyO3` によるネイティブPythonバインディング（`yui-py` cdylib/rlib）。`AppState`, `Selector`, `Representation`, 本物の `AtomGroup` をPythonへ公開。メソッドチェーン（`app.select(...).show(...).color_by(...)`）、型付きセレクタコンストラクタ（`yui.symbol`, `yui.residue_name` 等。裸の文字列は6.2節に従いPathWildcardのみで独自ミニ言語は無し）、ブール結合（`and_`, `or_`, `not_`）、セレクタ原子ID解決、YAML往復を統合テスト済み。 |
| `backend` | 動作可能（フェーズ4, 6a-i, 6a-ii, 6e, 6f-ii完了） | RESTエンドポイント（`/api/state`, `/api/structure`, `/api/fs/list`, `/api/fs/read`）、`pack_collab_packet`/`unpack_collab_packet` によるWebSocket差分同期（`/api/collab`）（7.3節）、`notify` クレートによるリアルタイムログWebSocket（`/api/logs`）（13.3節）、および `portable-pty` プロセス起動による全二重ターミナルWebSocket（`/api/terminal`）（13.2節）を実装。`RoleResolver`（Host/Editor/Viewer）による権限検証（7.1節）に対応し、ターミナルはHostのみに厳格に限定。LLMプロキシ（14章）はフェーズ6bで対応予定。 |
| `frontend` | 動作可能（フェーズ2, 4, 5a, 5b-i, 5b-ii, 5b-iii, 6a-i, 6a-ii, 6c-i, 6c-ii, 6d-i..v, 6e, 6f-i..v, 6i-ii完了） | `iced::widget::pane_grid` による4ペインレイアウト。球棒（ball-and-stick）描画、Catmull-Romリボン/カートゥーン表示（4.3節）、非共有結合相互作用（水素結合、塩橋、CH-π、ジスルフィド結合）の破線描画パイプライン（10.2節）、WGSL Compute Shader（`cs_cull`）によるGPU駆動視錐台カリング・atomicコンパクション・`draw_indexed_indirect` 間接描画（4.2節）、段階的LOD（近距離球棒/中距離インポスタークワッド）。Bottomペインのログビューワー・ターミナル（PythonコンソールとAIチャットタブは「Coming soon」プレースホルダ）。TrunkによるWebAssembly (WASM) ビルドパイプライン・ブラウザ内WebGL2/WebGPU描画・WebSocket通信層。メニューバー（File/View/Help）、レイキャスト原子ピッキング、右クリックコンテキストメニュー（8.2〜8.4節）。操作可能な `StructureTree`、`RepresentationInspector`（相互作用トグル対応）、`TextEditor`、プロジェクトディレクトリツリー。ローカルファイルおよびbackend `/api/fs/read` からの実構造ファイルロードに対応。Python プラグイン（フェーズ6k）: プラグインは、YUI が起動する別のプロセスとして動く（YUI が同梱する Python のランナー、または msgpack のプロトコルを話す任意のプログラム）。動かし続ける方式と、コマンドごとに起動し直す方式があり、View → Plugins... にプラグインの一覧がある。 |
| `renderer-native` | スケルトンのみ | 約30行。YAML状態ファイルの読み込み確認のみ。 |
| `proteindf-bridge` | `core` へ統合済み（フェーズ6e完了、フェーズ6gで可搬化完了） | 本物の `proteindf-bridge` を `AtomGroup`、`Bond::setup()`、PDB/mmCIFパーサ、DSSP二次構造推定に統合済み。upstreamリリースタグ `2026.10.20`（φ/ψ の計算、`AtomGroup::setup()` §3.14 および mmCIF 書き出しを含む）に固定。ターゲット別feature制御（native: `zstd`, wasm32: `ruzstd`）。残るupstream要件: Gaussian CUBEパーサ、`ProteinDF_pytools`統合（HDF5, Mulliken）。 |

**含意:** `proteindf-bridge` は `core` に統合され、可搬化された環境下で実構造のロードを支えている（フェーズ6g-i）。初回リリース `2026.10.0` により、基盤となる可視化、スクリプティング、コラボレーションバックエンド、WebAssemblyフロントエンドがパッケージ化された（フェーズ6g-iii）。続く第2回リリース `2026.10.1` では、非共有結合相互作用の可視化（フェーズ6i）、分子表面表示（フェーズ5c-i/ii）、マルチモデル表示切り替え、CI自動化が導入された。第3回リリース `2026.10.2` では、`.mol2` の読み込み、File メニューのファイル選択ダイアログ、PDB / mmCIF への構造の書き出しが加わり、`proteindf-bridge` の依存がリリースタグ `2026.10.1` に切り替わった。第4回リリース `2026.10.3` では、キャンバスの大きさを選べる PNG / SVG / アニメーションの書き出し、Gaussian CUBE の等値面、表現ごとのスタイル（Spacefill、Licorice、Line）、マルチモデルと軌跡の再生とカメラのアニメーション、原子の選択、2 つ目のビューポート、テーマと設定ファイル、見た目の自動確認（描画のギャラリー、UI テスト、スモークテスト）が加わり、第5回リリース `2026.10.4` では、分子表面が数倍速くなり、描画の設定（`render:`）、SSAO（既定はオフ）、CUBE の表示のワークスペースの状態への保存（ファイルごと）、SVG のリボンと分子表面の書き出し、書き出しの形に 3D ペインの縦横比を使うことが加わった。第6回リリース `2026.10.5` では、配信できるリリース版の Web ビルド（`scripts/build-web.sh`、`YUI_WEB_ROOT`）、署名なしのネイティブインストーラー（Linux の `.deb` / `.AppImage`、macOS の `.dmg`、Windows の NSIS。手動のワークフロー）、ブラウザ版のアニメーションの書き出し（PNG のフレームの zip）が加わった。第7回リリース `2026.10.6` は、ターミナルを使いやすくした（キーボードの直接入力、ペインの大きさへの追従、背景色・反転・下線、マウスの範囲選択とコピー・貼り付け、1000 行の履歴、`terminal.font_size`）。あわせて、ファイルを右クリックする「Open with」メニューと、テキストエディタのスクロールバーを足した。第8回リリース `2026.10.7` は、セキュリティのリリースである。`backend` が、ほかの Web サイトからの要求を断る（`Origin` / `Host` の検査。これまでは、利用者が見ているどの Web ページも、手元で動いている `backend` のターミナルを開けた）。すべての API が、固定のトークン 1 / 2 / 3 ではなく、起動ごとのランダムな秘密を求める。`YUI_BIND` と、ターミナル・ファイルの門で、TLS プロキシの後ろへの配備に備える（フェーズ6j-i、6j-ii）。第9回リリース `2026.10.8` は、セキュリティの共有の側を仕上げた。Host が、期限つきで取り消せる Editor / Viewer の招待を作り（**File → Share Session...** か `scripts/yui-invite.py`）、認証の失敗に回数制限が付き、`backend` が自身の動作ログを日ごとのファイルにも書く（フェーズ6j-iii）。第10回リリース `2026.10.9` は、磨きのリリースである。SSAO にぼかしを入れた（粒ではなく、なめらか）。ブラウザ版を圧縮して配信する（`.wasm` は 13.7 MB から、gzip で 6.3 MB）。招待で開いた接続は、招待を取り消すとすぐ閉じる。`backend` のエラーの文が理由を 2 回言っていたのを直した。第11回リリース `2026.10.10` は、見た目の磨きのリリースである。メニュー、タブ、ペインのタイトルがよりフラットで控えめになり、ボタンと書き出しダイアログの選択肢は強調色にそろった（設定ファイルの `ui:` の節で、タブの形、大きさ、角の丸み、色を変えられる）。Help には、マウス、キーボード、ターミナルの操作の一覧が加わった。その後の基盤機能として、組み込みインタラクティブPythonコンソール（フェーズ6h）、AIチャット統合（フェーズ6b）、GPU加速分子表面表示（フェーズ5c-iii）等へと進む。

---

## 3. フェーズ計画

各フェーズは、`architecture.md` 自体が採用している積み上げ順（基礎的な章を先に、応用・付加価値的な機能を後に）と同じ依存関係の順序で並べている。各フェーズは1つの巨大なcommitではなく、レビュー可能な単位の複数の変更に分けて進めること。

### フェーズ1 — `core` の堅牢化 + ローカルな `AtomGroup`/`Selector` 代替実装（完了）
- `core` の `AppState` を2.1節（Rust struct ⇄ MessagePack ⇄ YAML のロスレス相互変換）に適合させ、各変換方向のテストを追加する。
- 最小限のローカルな `AtomGroup`（鎖→残基→原子の木構造、2.2節）と、`Select_*` 述語群＋`And`/`Or`/`Not`（6.2.1節）を備えた `Selector` を、**当面は `core` 内に**、`proteindf-bridge` の一時的な代替であることを明記した上で追加する。PDB/mmCIF等のフォーマットパーサはここでは作り込まない（それは `proteindf-bridge` の管轄）。描画/UI作業のブロックを解消できる程度の、手組みまたは単純なテスト用フィクスチャで十分。
- `Representation`（6.2.2節）を `AppState` 内の構造化データ型として追加する。

### フェーズ2 — `frontend` MVP: 目に見える描画（完了）
- 8章のLeft/Center/Right/Bottomレイアウトに沿った `iced::widget::pane_grid` スケルトン（Center以外はプレースホルダで可）。
- フェーズ1の代替構造体を、ネットワーク接続なしでネイティブに描画する、ball-and-stick（4.1節）の `wgpu` ジオメトリインスタンシング。
- ゴール: 「小さな構造を読み込み、描画され、カメラを回転・ズームできる」というデモ可能なマイルストーン。

### フェーズ3 — スクリプティング基盤（完了）
- 6.1節に沿って `Selector`/`Representation` を公開する `PyO3` ネイティブバインディング（`yui.select(...).show(...).color_by(...)`）。
- これにより、後続のQCLObot連携（11章）とAIチャットアシスタントのツール呼び出し（14.3節）の両方が、新規言語ではなく同一のPython API上で定義できるようになり、道が開ける。

### フェーズ4 — `backend` の配線: 構造I/O + 差分同期 + ロール（完了）
- 構造データのアップロード/取得用RESTエンドポイント、`rmp-serde` を用いた `AppState` 変更の差分同期WebSocket（7.3節）。
- `CollabTransport` トレイト（7.2節）と `WebSocketTransport` 実装、Host/Editor/Viewerモデル（7.1節）に沿った `RoleToken` 検証。
- `frontend` がローカルファイルI/Oの代わりに（あるいはそれに加えて）このチャンネル経由で `backend` を利用するよう配線する。

### フェーズ5 — スケールアウト描画（完全な忠実度は `proteindf-bridge` 待ち）（5a、5b、5c-i、5c-ii、5c-iv は完了。5c-iii は未着手）

フェーズ5は範囲が広い（4.2〜4.5節）ため、それぞれ独立してレビュー可能な3つのサブフェーズに分割する:

- **フェーズ5a — リボン(カートゥーン)描画（4.3節）。** 自己完結: フェーズ1の代替実装 `AtomGroup`/`Residue` は既に `secondary_structure` を保持しているため、新規の上流データを必要としない。また `AppState.representations` が実際にビューポートの描画内容を左右する初めてのケースでもある（これまではビューポートは `representations` を無視し、常に全原子をball-and-stickで描画していた）。
- **フェーズ5b — GPU駆動カリング/空間分割/LOD/ストリーミング（4.2節）。** 約100万原子規模を想定した最適化であり、より大きな合成構造データが無いと約9原子の代替ペプチドでは効果を有意に検証できない。5aの後に行う。4.2節自体が4つの異なるサブシステムを束ねているため、さらに分割する:
  - **フェーズ5b-i — 大規模な合成構造フィクスチャ + `core`内の空間分割構造（BVH/Octree）。** 基礎となる部分: カリング・LOD判定・選択ヒット判定（4.2節）はいずれもこれに依存する。また、`frontend/src/viewport.rs`に既にあるO(n²)の総当たり結合検出（フェーズ2レビューで指摘済み）を、スケールする実装に即座に置き換えられる。この段階ではCompute Shaderには着手せず、純粋にCPU側のデータ構造とし、正当性と計算量改善の両方を示すベンチマーク/テストを備える。
  - **フェーズ5b-ii — Compute Shaderによる視錐台カリング + Indirect Draw（完了）。** 5b-iの`AtomBvh`および視錐台平面抽出の上に構築。`view_proj`行列からの6平面抽出、バウンディングスフィア判定を行うWGSL Compute Shader（`cs_cull`）、atomicカウンタによるインスタンスコンパクション、およびCPU読み戻しを介さない`draw_indexed_indirect`間接描画を実装。合成タンパク質格子フィクスチャ上で実動作とカリング効果を検証完了。
  - **フェーズ5b-iii — 段階的LOD（近距離=ball-and-stick、中距離=インポスタービルボード）（完了）。** 5b-iiのカリングの上に構築。2段階LODを実装: 近距離（$\le 65.0\text{ \AA}$）は球・結合ともにフルジオメトリメッシュ描画、中距離（$> 65.0\text{ \AA}$）はカメラ正対ビルボード単位クワッドによるインポスター球描画（円外破棄と法線再構成）とし、結合はプリミティブ抑制のため自動非表示。WGSL Compute Shader（`cs_cull`）内で距離二乗判定によりNear/Midの2系統バッファへ動的コンパクションを行い、2系統の `draw_indexed_indirect` で描画。第3段階（遠距離=表面のみ、4.2節）はフェーズ5c完了までの保留（ギャップ）として文書化。
  - **ストリーミング（4.2節、チャンク単位のロード/破棄）は当面無期限に保留**とし、番号付きサブフェーズとしては予定しない。これには`backend`側のページング/チャンク単位の構造配信対応が必要だが現状存在せず、パイプライン全体を通す実データもまだ無いため、実装の動機付けが弱い。`proteindf-bridge`が実際の大規模PDB/mmCIFファイルをパースできるようになる等、実際の大規模構造取り込み経路ができた時点で再検討する。
- **フェーズ5c — Marching Cubesによる分子表面/等値面生成（4.4/4.5節）。** 正しさを検証可能なCPU参照実装と段階的な描画統合のため、4段階に分割する:
  - **フェーズ5c-i — coreにおけるCPU参照実装（完了）。** 3次元ガウス密度場の格子生成と、共有辺の頂点インデックス統合・外向き法線計算・最近傍原子ID対応付けを備えたMarching Cubes等値面抽出の実装。解析的球フィクスチャ（表面積・体積誤差±5%以内）、閉じた多様体トポロジー、および1HLSで検証。
  - **フェーズ5c-ii — CPUメッシュによる `RepresentationKind::Surface` の実描画（完了）。** 生成された表面メッシュをビューポートの描画パイプラインおよび `RepresentationKind::Surface` に接続し、`ColorScheme`（鎖別、残基別、単色）による頂点カラーに対応する。
  - **フェーズ5c-iii — GPU（Compute Shader）への移植。** Marching Cubes等値面抽出をWebGPU Compute Shaderに移植し、5c-iのCPU版の結果を正解として同一トポロジー・幾何が得られることを検証する。
  - **フェーズ5c-iv — Gaussian CUBE入力（最初の版は完了）。** `proteindf-bridge` 2026.10.5 に CUBE の読み込みが入ったので、File →「Open CUBE (Isosurface)...」で、最初のデータセットの正と負の等値面を表示できる（`core/src/cube_iso.rs`、`frontend/src/cube_view.rs`）。Right ペインのカードで、等値レベルのスライダー、軌道の選択、色の見本が使える。構造を開いていなければ CUBE の原子を表示し、斜めの格子は再サンプリングし、ファイルのパスをワークスペースに保存する（`data_paths.cube_path`。ネイティブ版のみ）。まだ: 実際の ProteinDF の CUBE での確認、ワークスペースの相対パス、ブラウザ版での開き直し。

3つのサブフェーズいずれも、実際のタンパク質データに対しては `proteindf-bridge`（10章、10.1節）由来の二次構造ラベルと堅牢な大規模構造パースが必要。フェーズ1の代替実装に対しては合成/テストデータでまず実装し、`proteindf-bridge` が揃った時点で再検証する。

### フェーズ6 — ターミナル & ログビューワー、AIチャットアシスタント（6a は完了。6b は未着手）

これらはすでに機能している可視化＋スクリプティング基盤の上に載る付加的なUI機能である。アクセス制御・ツール呼び出しの設計がロールモデル（7.1節）とPython API（6章）の存在を前提としているため、フェーズ1〜4が機能する前に着手しないこと（この前提条件はフェーズ4完了時点で既に満たされている）。13章と14章は機能的に独立しているため、2つのサブフェーズに分割する:

- **フェーズ6a — ターミナル & ログビューワー（13章）。** いずれも13.2節に従い `backend` 側でホストし、`frontend` はコラボレーション（7.2節）と同じトランスポート抽象パターンで通信する。ロールモデル（7.1節）でアクセス制御し、ターミナルは既定でHostのみ、ログビューワーはEditorにも共有可とする。ターミナル（実PTY起動、VTEクレートによるANSI/VT解析、ゼロから作るカスタム`iced`セルグリッド描画ウィジェット）はログビューワー（7章の既存の差分同期/ストリーミングパターンをプレーンテキスト行に再利用できる）より格段に難易度が高いため、さらに分割する:
  - **フェーズ6a-i — ログビューワー（完了）。** `notify`ベースのファイル監視と未作成ファイル待機、`/api/logs` WebSocketエンドポイントによる追記専用行ストリーミング、Bottomペインの4タブ化（Log Viewer / Python Console / Terminal / AI Chat）、上限付きリングバッファ（5,000行）テキストビュー、自動スクロール（一時停止/再開トグル対応）、および部分文字列フィルタ。読み取り専用でEditorロールにも共有可。
  - **フェーズ6a-ii — ターミナル（完了）。** `backend` 側でホストする `portable-pty` によるシェル子プロセス起動と全二重ストリーミング（`/api/terminal` WebSocket）。ANSI/VT状態機械は `vt100` クレートを採用（C依存ゼロ、純粋インメモリ、WASM互換性から `alacritty_terminal` より選定）。等幅フォント、カーソル強調、クイック制御キーボタンを備えたカスタム `iced` ターミナルペインウィジェット。`RoleResolver` により厳格に Host ロール（role_token=1）のみに限定。
- **フェーズ6b — AIチャットアシスタント（14章）。** Claude/Gemini/Ollamaを差し替え可能にする `LlmProvider` 抽象、チャットパネル、フェーズ3のPython APIへのツール呼び出し、状態ダイジェストによるコンテキスト付与。6aの後に行う。着手前の留意点: 実際のエンドツーエンド検証には本物のAPIキー（Claude/Gemini）またはローカルOllama環境のいずれかが必要になる。HTTP応答をモックしたテストのみとするか、実際の認証情報を用意するかを事前に決めておくこと（これによって本サブフェーズの「完了」の定義が実質的に変わるため）。

### フェーズ6c — WASM/Webフロントエンド対応（完了）

`frontend`はこれまで全フェーズを通じてネイティブ版のみであり、1.1節はWASM互換性を目標として掲げているものの、実際にブラウザビルドを試みたフェーズは無い。既存のwgpu/`iced::widget::shader`ベースの描画（Compute Shaderカリング、ビルボードインポスター、カスタムのリボン/ターミナルパイプライン）がそもそもブラウザ上で動くのかどうかが最大の未知数であるため、通信部分に着手する前にまずそれを検証する:

- **フェーズ6c-i — wgpu/icedがブラウザで描画できることを示す最小WASMビルド（完了）。** `frontend/Cargo.toml` のネイティブ専用ネットワーククレート（`tokio-tungstenite`, `tokio` ランタイム, `reqwest`）を `cfg(not(target_arch = "wasm32"))` に分離し、`.cargo/config.toml` で `getrandom_backend="wasm_js"` を構成。`frontend/index.html`（Trunkエントリーポイント）を作成し、`trunk` によるビルドとブラウザ実行で既存の3D分子描画パイプライン（球棒モデル、Catmull-Romリボン/カートゥーン、視錐台カリング、LOD、カメラ軌道回転/ズーム）が完全に動作することを実証。通信層は `console_error_panic_hook` および `web-sys` console ログを備えたローカルスタンドアロンビューワーとしてスタブ化した。
- **フェーズ6c-ii — WASM互換の通信層（完了）。** `gloo-net` による `cfg(target_arch = "wasm32")` 向けブラウザWebSocketトランスポートを実装し、`tokio::sync::mpsc` チャネルと `wasm_bindgen_futures::spawn_local` を介して `iced` の `Send` ストリーム要件にブリッジした。ブラウザの `fetch` を使用するターゲット非依存の `reqwest` により初期状態取得を再有効化。URLクエリパラメータ（`?collab=` / `?backend=`）からの接続先抽出および `location.hostname` へのフォールバック（`resolve_wasm_collab_url`）を実装し、上位のアプリケーションロジックを分岐させることなく、ブラウザビルドを実際の `backend`（協調編集、ログ監視、双方向PTYターミナルストリーム）へ完全に再接続した。

実際にビルド済みWASMバンドルを配信・配布する部分は、引き続きフェーズ8（9章）の範囲とする。6c は機能することの証明に留める。

### フェーズ6d — メニューバー & コンテキストメニュー（8.2〜8.4節、新設）（完了）

既に機能している可視化＋スクリプティング基盤以外に前提条件は無い。現状Pythonコンソール経由か固定の既定値でしか到達できない操作に、発見しやすいUIを追加する。既存仕様の実装ではなく新規に設計したUI領域（8章はこれまで3つの簡潔な箇条書きしか無かった）であるため、「今すぐ作れるもの」と「先に別のウィジェットを作る必要があるもの」で分割する — LeftペインとRightペインは現状プレースホルダーのテキストであり、操作可能なウィジェットではない:

- **フェーズ6d-i — 上部メニューバー（File / View / Help、8.2節）（完了）。** 外部依存クレートを追加せず、ネイティブ版・WASM版の双方で動作するウィンドウ内 `iced` ウィジェットとして実装した。Fileメニュー: New（既定の空 `AppState` へのリセット）、Open（YAML状態ファイルのインポート）、Save / Save As（YAMLへのエクスポート）、Quit。Viewメニュー: Reset Camera（`camera_reset_token` によるカメラ視点既定値復元）、Left（Explorer）/ Right（Inspector）/ Bottom（Panels）各ペインの表示/非表示切替（`build_pane_configuration`）。Helpメニュー: About（CalVerバージョン、GPLv3ライセンス、説明を表示するモーダルダイアログ）。仕様に従い、変更履歴機構が存在しないEditメニュー（Undo/Redo）は明示的に除外。すべてのアクションは専用DSLを作らず既存の `AppState` / `CameraState` 操作パイプラインを通過する（8.4節）。
- **フェーズ6d-ii — 3Dビューポートの右クリックコンテキストメニュー（8.3節）（完了）。** `math::screen_to_world_ray` と `AtomBvh::query_ray`（原子半径マージン付きSlab法AABB枝刈り、および球・半直線の精密正交点判定）によるレイキャストピッキングを実装。`viewport::ViewportProgram` で右クリックイベントを検知してコンテキストターゲットをメインループへ発行するパイプラインを統合。最上位スタックレイヤー上に透明 `mouse_area` バックドロップ（外側クリックで閉じる）を備えたウィンドウ内コンテキストメニューカードウィジェット（`context_menu::view_context_menu`）を構築した。8.4節の原則を厳格に遵守し、全メニュー操作（Select Atom, Select Residue, Hide Atom, Show As, Color By）を `Selector::AtomIds`、`Selector::ResidueRange`、`Representation` の変更に直接マッピングし、ビューポート描画への即座のフィードバック（選択ハイライト、カスタム配色反映、非表示原子除外）を実装。ネイティブおよびWASMの双方でテスト検証を完了。
- **フェーズ6d-iii — ペイン/ビューモデルの汎用化（基盤フェーズ、6d-iv/6d-vに先行）（完了）。** 「どのコンテンツをどこにドッキングするか」を固定の `PaneKind` から切り離すため、実行時ビュー識別子 `ViewId`（`Viewport`, `StructureTree`, `RepresentationInspector`, `LogViewer`, `PythonConsole`, `Terminal`, `AiChat`）およびペインごとのビュー管理状態 `PaneViewState`（ビューリスト＋アクティブインデックス）を導入。複数ビューを持つ任意のペインで動的に表示可能な共通タブ切り替えUI（`view_tab_bar`）を実装し、従来の独立した `bottom_tab` を汎用機構へ完全統合。単一ビューのペインではタブバーを表示せず、現行のレイアウトと挙動を100%維持。`state.yaml` の永続化機構を拡張し、`SavedWorkspaceState` により `AppState` とペイン構成を同時にシリアライズ可能とするとともに、旧形式YAMLの読み込み時は既定の配置へ自動フォールバックする後方互換性を確保。単体テスト4件を追加し、ネイティブおよびWASMの双方で動作確認を完了。
- **フェーズ6d-iv — Leftペインを実際の構造/ファイルツリーにする、およびそのコンテキストメニュー（完了）。** `frontend/src/structure_tree.rs` に操作可能な `StructureTree` ウィジェットを新設し、`ViewId::StructureTree` として登録。`AtomGroup`（Model → Chain → Residue → Atom）の階層構造をインデントおよび展開/折りたたみトグルボタン付きで再帰的に描画。大規模構造（最大9,216原子規模の合成格子）でも軽快に動作するよう既定ですべて折りたたみ状態を維持。8.4節アクションパイプラインの原則を厳格に遵守し、左クリック選択（Model: `Selector::PathWildcard`, Chain/Residue: `Selector::ResidueRange`, Atom: `Selector::AtomIds`）を黄色ハイライト表現として `representations` に即座に反映。最上位スタックレイヤーに右クリックコンテキストメニュー（`view_tree_context_menu`）を配置し、Select, Hide, Show As, Color By の各操作を直接 `Selector`/`Representation` の更新へマッピング。未ロード時の空状態表示にも対応。単体テスト6件を追加し、ネイティブおよびWASMの双方で動作確認を完了。
- **フェーズ6d-v — Rightペインを実際の`Representation`一覧ウィジェットにする、およびそのコンテキストメニュー（完了）。** `frontend/src/representation_inspector.rs` に操作可能な `RepresentationInspector` ウィジェットを新設し、`ViewId::RepresentationInspector` として登録。`self.state.representations` を直接編集できるカード型リストとして描画し、3Dビューポートへの即座のフィードバックを提供。ツールバーの「+ Add」ボタンによる既定の表現追加（`Selector::All`, `BallAndStick`, `ColorScheme::Element`）、行ごとの可視性トグル（`👁`/`🚫`）、削除（`✕`）、および表示形式（`BallAndStick`, `Cartoon`, `Cpk`, `Licorice`, `Line`、未実装の `Surface` には "(not yet rendered)" を明記）と配色（`Element`, `Chain`, `Residue`, `SecondaryStructure`, `Uniform`）の切替ボタングループを装備。最上位スタックレイヤーに右クリックコンテキストメニュー（`view_rep_context_menu`）を配置し、可視性トグル、形式変更、配色変更、削除をサポート。単体テスト4件を追加し、ネイティブおよびWASMの双方で動作確認を完了。

5つのサブフェーズいずれも、そのアクションはスクリプティング層と同じ`Selector`/`Representation`のPython API相当の呼び出し経由で処理すること（8.4節）— メニュー/コンテキストメニュー専用のコマンド経路は作らない。これは新規DSL禁止ルール（6.2節）と整合する。

### フェーズ6e — ローカルな`AtomGroup`/`Selector`代替実装を本物の`proteindf-bridge`に置き換える（フェーズ5cとフェーズ7のブロッカー解消）（完了）
2026-09-19に調査: これは元々予定されていたことだった（`core/src/atom_group.rs`と`selector.rs`は「proteindf-bridgeが実装されるまでの一時的なローカル代替」と最初から明記されている）。実際の`proteindf-bridge`には既に、動作確認済みでドキュメント化された「protein schema」規約（`/model_N/chain_id/res_key/atom_key`というパスの深さによる規約。本リポジトリの型付き`Model → Chain → Residue → AtomRecord`と完全に対応する）と、`Bond::setup()`というVDW半径ベースの結合検出器が存在する — したがって結合生成は完全にproteindf-bridge側の責務であり、本リポジトリは利用するだけで検出ロジックを独自実装しない（10章）。変換用のアダプタ層ではなく型そのものの置き換えが正しい形だが、影響範囲は実際に大きい: `.models`/`.chains`/`.residues`/`.atoms`という型付きフィールドを直接読んでいるファイルが14個ある（`core/src/{selector,atom_group,packing,spatial,export}.rs`、`core-py/src/{state,atom_group}.rs`、`frontend/src/{ribbon,structure_tree,menu_bar,state_sync,viewport}.rs`、`backend/src/main.rs`、`renderer-native/src/main.rs`）ため、1つのPRにまとめず依存関係順にサブフェーズへ分割する。本物の`atom_group`データを含む`state.yaml`/MessagePackとの後方互換性は失われる — `#[serde(default)]`では型の完全な置き換えを吸収できないため、6e-viiで`AppState.version`に初めて実際の意味（「未対応/旧形式」への明確なエラー）を持たせる必要がある。

**ブランチ運用の例外（2026-09-19、6e-iiのレビュー中に決定）:** 基本ルール§1は、あらゆる
フェーズ/タスクの完了条件として`cargo check --workspace`が通ることを要求しているが、このサブフェーズ
分割は6e-iiから6e-vまでの間、他の未移行ファイルを意図的に壊れたままにする（14ファイルにわたる
影響範囲をレビュー可能な単位に分割すること自体がその目的）。各サブフェーズをそのまま`develop`に
マージすると、フェーズ6eの期間中`develop`自体が`cargo check --workspace`に失敗する状態になり、
他の並行作業者に対してこのルールを破ることになる。**対処: 6e-ii〜6e-vは個別に`develop`へは
マージしない。** 各サブフェーズは*直前の*サブフェーズのブランチから分岐し（スタック型PR連鎖:
`feature/phase-6e-iii-...`は`develop`ではなく`feature/phase-6e-ii-atomgroup-swap`から分岐する。
PRのbaseブランチも`develop`ではなく直前のサブフェーズのブランチとする）、その前提でレビュー・承認を
行う。6e-vが完了し`cargo check --workspace`/`cargo test --workspace`が再びグリーンになった時点で
初めて、スタック全体を古い順に`develop`へマージする（この時点では各マージは新たな破壊を持ち込まない
fast-forwardになる）。6e-iは対象外（既に`develop`へマージ済みで、単独ではワークスペースを
壊していなかったため）。

- **フェーズ6e-i (完了) — `proteindf-bridge`へのパス依存追加。wasm32ビルド確認のみ、消費側コードは無変更。** `core/Cargo.toml`が`../../ProteinDF_bridge/rust/crates/proteindf-bridge`に依存する。本リポジトリが自身の`zstd`を扱う方式と同じくターゲットゲートする（`[target.'cfg(not(target_arch = "wasm32"))'.dependencies]`は`zstd`、wasm32では`ruzstd`）— 下記のbridge側要求リストで、消費側が毎回`--no-default-features --features ruzstd`を指定しなくて済むよう上流側でこれを組み込んでもらうよう依頼する。`cargo check --workspace`と`cargo check --target wasm32-unknown-unknown -p yui-frontend`の両方が、他のファイルを一切変更せず通ることを完了条件とする。
- **フェーズ6e-ii (完了) — `core::atom_group`の公開型を本物の`proteindf_bridge::AtomGroup`に置き換える。** 本物の`AtomGroup`は外部クレートの型（orphanルールが適用される）なので、`create_test_peptide()`/`create_test_protein_lattice()`はinherentメソッドではなく拡張トレイトのメソッドになる — 約20箇所ある呼び出し元（いずれも`Chain`/`Residue`/`Model`を直接構築せずこれらのファクトリを呼ぶだけ）は`use`のインポート追加だけで済み、書き換えは不要。**このサブフェーズを仕様化する過程で見つかった訂正点:** ファクトリのシグネチャを完全に維持することは結局できない — `proteindf-bridge`の`calc_secondary_structure()`は（`Bond::setup()`が`add_bond`でツリーに書き戻すのとは違い）結果を別の`Vec<SecondaryStructure>`として返すだけで、`AtomGroup`ツリーには反映されない（`AtomGroup`にそれを持たせるフィールドが無いため）。そのためファクトリの戻り値は`(AtomGroup, SecondaryStructureMap)`になる — `SecondaryStructureMap`は`AtomGroup::path()`をキーとする`HashMap<String, SecondaryStructureKind>`の一時的なサイドマップで、`atom_group.rs`/`selector.rs`自体と同様「bridge側への要求（下記、`bonds`と同格の専用フィールド）が実現し次第削除する代替実装」と明記する。`AppState`にはこのマップを保持する新フィールドを追加してYAML/MessagePackに乗せる（`atom_group`自体がdelta同期対象でないのと同様、`AppStateDelta`には追加しない）。`proteindf_bridge::AtomGroup` が `Serialize`/`Deserialize` を導出しないため、`core::atom_group::AtomGroup` は bridge 側の型をラップする newtype（`Deref`/`DerefMut` 実装）として定義し、`with-serde` feature を有効化した `rmpv` を介して `get_raw_data()`/`from_dict_data()` に委譲する手動の `Serialize`/`Deserialize` を実装した（YAML および MessagePack のラウンドトリップテストで往復整合性を検証済み）。`core::atom_group`には、深さ規約の走査を一箇所に閉じ込める小さなヘルパー関数を追加する — 基盤となる型はスキーマレスで規約を強制する仕組みが何もないため、以降のサブフェーズは各自で深さの前提を再導出せず、必ずこのヘルパー経由にすること。
- **フェーズ6e-iii (完了) — `core/src/selector.rs`のマッチングエンジンを移行**し、6e-iiのヘルパーを使うようにする。公開`Selector`enumの形状（本リポジトリが独自に追加した`And`/`Or`/`Not`コンビネータを含む。proteindf-bridge側のトレイトベースセレクタにはこれらが無く、追加も依頼しない）はYAML/MessagePack/PyO3互換性のため変更しない — 内部の`matches()`/`select_atom_ids()`実装のみを変更する。`ResidueRange{start,end}`の`i32`範囲比較には、新しい`String`キーの残基IDをパースするための明示的かつテスト済みの方針が必要（`"6A"`のような挿入コード付きのケースを含む）。
- **フェーズ6e-iv (完了) — 残るレンダリング/UI消費箇所を移行**（`core::spatial`、`frontend::{ribbon,structure_tree,viewport,state_sync,menu_bar}`）。6e-iiのヘルパーを使うようにする。
- **フェーズ6e-v (完了) — `core-py`（`atom_group.rs`, `state.rs`）、`backend`、`renderer-native`を移行。**
- **フェーズ6e-vi (完了) — 本物の結合生成を配線する**: `to_flat_molecule()`内で`proteindf_bridge::Bond::setup()`（下記のbridge側スケーラビリティ要求が先に対応されていれば、その空間分割対応版）を呼び、`Molecule.bonds`に反映する。`frontend/src/viewport.rs`を、`Molecule.bonds`が非空ならそちらを優先し、空なら既存の`AtomBvh::query_pairs_within_range`幾何ヒューリスティックにフォールバックするよう切り替える。
- **フェーズ6e-vii (完了) — フォーマット破壊への対応と実データでの検証。** `AppState.version`に、暗号めいたserdeエラーの代わりに「未対応/旧形式の`atom_group`」への明確なエラー経路を持たせる。6e-iiの深さ規約の前提を、本リポジトリの合成フィクスチャだけでなく、`proteindf-bridge`の実際のパーサで読み込んだ最低1件の実PDB/mmCIFファイルに対して検証する（proteindf-bridge側にはこの規約を強制する仕組みが何もないため。下記のbridge側要求リストを参照）。`architecture.md`/`architecture.ja.md`の2章を、代替実装ではなく本物の`AtomGroup`型を説明する内容に更新する。

### フェーズ6f — VSCode風プロジェクトエクスプローラと汎用テキストエディタ（完了）
`architecture.md` 1.1節では、フェーズ6d-iiiでペインの表示内容をランタイムの`ViewId`へ一般化した際に、既に「将来的なビュー（スクリプトエディタ等）」への拡張が想定されていた。本フェーズでそれを実際に実装する: 選択したプロジェクトディレクトリをツリー表示し（既存のLeftペイン「Explorer / Playbook」タブ列を再利用）、選択したファイルをそのファイルに適したビューアで開く — 既定は新設の汎用テキストエディタだが、拡張子が既存の構造/状態ビューアと合致する場合はそちらを使い、複数候補がある場合はユーザーが選べるようにする。各PRをレビュー可能な粒度に保つため、依存順にサブフェーズへ分割する。

**着手前に見つかった設計上の訂正（2026-09-23）:** 当初の分割案は、単一ファイルのOpen/Saveと同様、`rfd::AsyncFileDialog`だけでディレクトリの「中身の列挙」もカバーできると誤って想定していた。実際にはwasm32上の`pick_folder()`が返す`FileHandle`は不透明な参照に過ぎず、`path()`自体が存在せず（rfd自身のドキュメントに"does not exist in WASM32"と明記）、中身を列挙する手段も提供されない。ブラウザ側で実際に列挙するにはFile System Access APIが必要だが、これはFirefox・Safariが全く実装しておらず（2026年9月時点でChromium系限定を確認済み）、クロスブラウザなツリーとしては行き止まりだった。**解決策: ブラウザのローカルファイルシステムではなく、既存の`backend`サーバー経由でディレクトリを閲覧する。** `backend`は元々ネイティブな`std::fs`に自由にアクセスできる上、ログストリーミング機能で`YUI_LOG_ROOT`という環境変数スコープ方式の前例が既にある。同じ考え方で新設する`YUI_WORKSPACE_ROOT`を土台に、そのルート配下を返すRESTエンドポイントを1つ用意すれば、ネイティブ・wasm32どちらのクライアントも（本アプリの他のREST/collab通信と同様に）単なるHTTPリクエストとして同一に扱え、ブラウザAPIの制約を恒久的な制約として受け入れる必要が無くなる。ネイティブビルドは、backend未接続のスタンドアロン利用のために`rfd::pick_folder()`＋ローカル`std::fs`によるフォールバックも維持する。また、「構造ファイルは既存の3Dビューポート/`AtomGroup`読み込みパイプラインへルーティング」という記述も、実は存在しないものを前提にしていたことが判明した — 実PDB/mmCIFファイルを`AtomGroup`に読み込むコードは`core/tests/real_pdb_validation.rs`（テスト専用）にしか存在せず、実行中のアプリからは一度も到達可能になったことがない。これをディスパッチフェーズの暗黙の前提とせず、独立した小さなサブフェーズとして切り出す。

- **フェーズ6f-i (完了) — 汎用テキストエディタペイン。** `iced::widget::text_editor`を土台とした新しい`ViewId::TextEditor`を追加し、`rfd::AsyncFileDialog`経由で任意のファイルを開閉・保存できるようにする。`rfd`を選ぶ理由は、ネイティブ（OSネイティブダイアログ）とwasm32（内部的にブラウザの`<input type=file>`を使用）の両方について、単一ファイルのOpen/Saveに限れば同一の非同期APIでサポートしており、実装を2系統に分けずに済むためである。ネイティブの`FileHandle`は実パスを保持する（ため「Save」はダイアログ無しで上書きできる）が、wasm32の`FileHandle`は保持しない（ブラウザのセキュリティモデル上の制約）ため、wasm32側の「Save」は事実上「Save As」相当の挙動に縮退する — これは想定内のプラットフォーム差であり、バグではない。エディタペインのOpen/Save操作はペイン内に閉じたものとし、意図的に既存のFileメニューのOpen/Save（8.2節、`state.yaml`ワークスペース状態専用）へは統合しない。
- **フェーズ6f-ii (完了) — backendのファイルシステムエンドポイント。** 新しい`GET /api/fs/list?path=<相対パス>`と`GET /api/fs/read?path=<相対パス>`のRESTエンドポイントを、`create_shared_state()`が既にログストリーミングを`YUI_LOG_ROOT`でスコープしているのと同じ要領で、`YUI_WORKSPACE_ROOT`環境変数（既定: CWD）配下にスコープする。すべての`path`はそのルートからの相対パスとして解決し、正規化した上でルートのプレフィックス配下に収まっていることを検証してから初めてディスクにアクセスする（外れる場合は拒否する）— これは単一のローカルユーザー専用のエンドポイントではなく、接続中のどのクライアントからも到達可能なサーバーエンドポイントであるため、パストラバーサル対策は必須である。情報漏洩リスクに見合ったレベルで`RoleResolver`によるゲートを掛けること（既存のHost/Editor/Viewerモデルを再利用し、少なくともEditor以上を要求する方向に倒す — Viewerに開放するかどうかは、読み取り専用だが機微なエンドポイントに対する既存のロール運用方針を改めて確認した上で判断する）。今回はフロントエンドUIは作らない。
- **フェーズ6f-iii (完了) — プロジェクトディレクトリツリー（ネイティブ・wasm32共通）。** Leftペインの「Explorer / Playbook」タブ列に（既存の`StructureTree`と並ぶ形で、その展開/折りたたみ行UIパターンを再利用した）新しいタブを追加する。backendに接続している場合は、その`YUI_WORKSPACE_ROOT`配下を`/api/fs/list`経由で閲覧する — フォルダ選択ダイアログは不要（そもそもルートがリモートのサーバー上にある可能性があるため意味を持たない）で、ネイティブ・wasm32いずれのクライアントも単なるHTTP呼び出しなので同一に動作する。backend未接続のスタンドアロン実行時は、`rfd::AsyncFileDialog::pick_folder()`＋ローカル`std::fs::read_dir`にフォールバックする（ネイティブのみ。backendもローカルファイルシステムも持たないスタンドアロンwasm32ではツリーは使えない）。いずれの経路でも展開時にその階層だけ読む遅延読み込み（open時点で全階層を再帰的に読まない）とし、合成フィクスチャの`StructureTree`では気にする必要がなかった大規模な実ディレクトリでのフリーズを避ける。個別エントリの権限エラーやシンボリックリンクの循環でクラッシュしないよう防御すること。ファイルクリック時に「ファイルが選択された」イベントを発行するところまでを実装し、何で開くかの判断（6f-vの責務）はまだ行わない。
- **フェーズ6f-iv (完了) — 実構造ファイルを`AppState`へ読み込む。** `proteindf_bridge`のPDB/mmCIFローダー（`Pdb::from_file()` → `get_atomgroup()` → `AtomGroup::setup()`、`core/tests/real_pdb_validation.rs`が既にテストで使っているのと同じパイプライン）を、ファイルのバイト列（ローカルパスからでも、6f-iiの`/api/fs/read`レスポンスからでも良い）を受け取って`AppState.atom_group`/`secondary_structure_map`を設定し`sync_molecule_from_atom_group()`を呼ぶ、アプリから到達可能な関数として配線する。現状このパスは実行中のUIから一度も到達可能になったことがなく、合成フィクスチャしかロードされたことがない。今回はUI入口は作らず、6f-vから呼び出される形にする。
- **フェーズ6f-v (完了) — 「開き方」ディスパッチ。** 6f-iiiで選択されたファイルを拡張子ごとにハンドラへ振り分ける: 構造ファイル（`.pdb`/`.cif`/`.mol2`等）は既定で6f-ivのローダーへ、それ以外は6f-iのテキストエディタへ。1つの拡張子に複数のハンドラが当てはまり得る場合（例: `.yaml`はワークスペース`state.yaml`のインポートとしても、単なるテキストとしても開ける）は、ユーザーが既定を上書きできる選択UIを用意する。

### フェーズ6g — ビルド環境の可搬化と初回リリース（完了）
- **フェーズ6g-i (完了) — proteindf-bridge 依存の可搬化。** 相対パス依存 `../../ProteinDF_bridge/rust/crates/proteindf-bridge` を、rev `997ad2478e8681ce2d55ff381feaa759b3395490` 固定のHTTPS Git依存へ変更し、ターゲット別featureゲート（native: `zstd`, wasm32: `ruzstd`）を維持する。実テスト構造（`1hls.pdb`, `1HLS.cif`）を `core/tests/data/` に配置し、出典READMEを整備する。外部の `~/.cargo/config.toml` による `[patch]` 上書きに対応する。
- **フェーズ6g-ii (完了) — ドキュメントおよびロードマップ同期。** `ROADMAP.md`/`ROADMAP.ja.md` の2章現状表および3章フェーズ計画を、完了済みフェーズと今後の優先度に合わせて最新化する。
- **フェーズ6g-iii (完了) — 初回リリース `release/2026.10.0`。** `release/2026.10.0` ブランチを作成し、バージョン番号の更新、チェックゲート全通過の確認を経て、git-flowに従いCalVerタグ付きで `main` へリリース・マージする。

### フェーズ6h — Python コンソール（未着手。フェーズ6k で埋め込みの Python を削除したので、方式を決め直す）
（※2026-10-03 ユーザー判断により、非共有結合相互作用の可視化（フェーズ6i）を優先するため後回しとした）
現在Bottomペインの「Python Console」タブは「Coming soon」プレースホルダ表示となっている。本フェーズでインタラクティブなPython評価環境を実装し、AIチャットのツール呼び出し（フェーズ6b）やインタラクティブなQCLObot解析（フェーズ7）の土台を整える:
- **フェーズ6h-i — ネイティブ版Pythonコンソール（埋め込みPyO3）。** ネイティブプロセス内で動作するインタラクティブなPyO3 REPLをBottomペインに実装し、`yui` モジュールおよびアクティブな `AppState` への直接操作を提供する。
- **フェーズ6h-ii — WASM版Pythonコンソール（Pyodide）。** Pyodideによるブラウザ内WebAssembly Python REPLを実装し、Web配信環境でもネイティブと同等のスクリプティング体験を提供する。

### フェーズ6i — 非共有結合相互作用の可視化（完了）
`proteindf-bridge` の `InteractionSet`（水素結合、CH-π、イオン対、ジスルフィド結合）を、3Dビューポート上に破線ジオメトリコネクタおよび選択可能な `Representation` として直接描画する。
- **フェーズ6i-i (完了) — 読み込み時の検出と保存・端点導出（core 側）。** 構造読み込み時にモデルごとに `detect_all` を呼び出して `AppState.interactions` に保存し、描画に必要な両端の3D座標（`resolve_interaction_endpoints`）および選択フィルタ用の原子ID集合（`resolve_interaction_atom_ids`）を解決する。1HLS実データ（PDB/mmCIF）を用いたジスルフィド結合・水素結合・CH-π・塩橋の統合テストおよび往復シリアライズテストを整備。
- **フェーズ6i-ii (完了) — frontend での破線描画と Representation 連携。** 3Dビューポートでの破線コネクタ描画パイプラインを実装し、`Representation` の `kind: Interaction` から表示・フィルタリングを切り替え可能にする。

### フェーズ6j — backend の認証とネットワークへの公開（完了）
`architecture.ja.md` 7.4節に設計した。**急ぐ発見:** `127.0.0.1` にバインドしていても、`backend` は、利用者が見ているどの Web ページからも操作できる（`/api/terminal?token=1` への、別のサイトからの WebSocket、緩い CORS）。Host のトークンが固定の `1` で、`Origin` も `Host` も調べていないからである。3 つとも終わった: 穴は閉じ、固定のトークンはなくなり、共有のセッションには、招待と回数制限がある。
- **フェーズ6j-i (完了) — Origin / Host の検査と、緩い CORS をやめる（小さい。プロトコルは変えない）。** あわせて、認証の失敗とターミナルを開いたことのログ。Web ページからの穴を閉じる。
- **フェーズ6j-ii (完了。Chrome で確認) — ランダムな秘密、渡し方、バインド。** 起動時のランダムな Host の秘密（`YUI_TOKEN` で上書き）、文字列を解決する `RoleResolver`（`TokenStore`）、接続に結びつけたロール、すべての `/api/` の認証、`Authorization: Bearer` / WebSocket のサブプロトコルでの秘密（チケットの API はなし。ヘッダーで、すでに URL から外せる）、`backend.json` / `--token` / `#token=` での受け渡し、TLS プロキシの宣言とターミナル・ファイルの門つきの `YUI_BIND`。
- **フェーズ6j-iii (完了) — 招待と乱用の制限。** 期限つきで取り消せる Editor / Viewer の秘密のための `POST/GET /api/invites` と `DELETE /api/invites/{id}`（開いている接続は、取り消しですぐ、期限切れは期限の時刻に閉じる）、`scripts/yui-invite.py`、接続元ごとの締め出し（60 秒の間に 10 回失敗すると、60 秒の 429）。アプリには、招待を作り、一覧し、取り消す **File → Share Session...** がある。

### フェーズ6k — Python プラグイン（完了）
`architecture.ja.md` 6.4節に設計した: プロジェクトの外の開発者が Python で機能を足す。YUI が起動する別のプロセスで動かす（標準入出力で msgpack のメッセージをやり取りする。ブラウザー版には当面プラグインはない）。ネイティブ版では完了（6k-i から 6k-vi）。まだ: `pip` / entry points、`numpy` などの外部パッケージ、Log Viewer での `plugin.log` の自動表示。
- **フェーズ6k-i — 組み込みの Python の基盤（完了。6k-v で置き換えた）。** 最初の版は、アプリの中で CPython を動かした（`frontend` の feature `python`、UI スレッドとチャネルでつながる Python のスレッド）。これは削除した。Python コンソール（フェーズ6h-i）は、別に決める必要がある（6.4節）。
- **フェーズ6k-ii — プラグインの読み込み（完了。失敗はステータスバーと `plugin.log` に出す。Log Viewer にはまだ出さない）。** `plugin.yaml` と `register(app)` を置く `<config>/yui/plugins/<name>/`、`api_version` の確認、`settings.yaml` の `plugins.enabled`（有効にするまで動かさない）、失敗のステータスバーと Log Viewer への表示。この段階の前に、`pip` / entry points にも対応するかを決める。
- **フェーズ6k-iii — メニューとコマンド（完了）。** 8.7節の受け口を使う `add_command`（識別子は `plugin.<name>.` の下）と `add_menu_item`。右クリックの対象をコマンドに渡す。
- **フェーズ6k-iv — プラグインの一覧と、プラグインを書く人のための文書（完了）。** アプリのプラグインの一覧（有効・無効、版、状態）、プラグインの書き方の手引き（英語と日本語）、公開 API の一覧、`api_version` の決まり、例のプラグイン。
- **フェーズ6k-v — プラグインを別のプロセスで動かす（`resident` は完了）。** メッセージのプロトコル（長さ付きの msgpack）、YUI が起動・終了する子プロセス、何も入れなくてよい Python のランナー（`msgpack` のパッケージがあればそれを使う）、`settings.yaml` の `plugins.python`、`plugin.yaml` の `mode` と `command`。`--features python` は要らなくなった。
- **フェーズ6k-vi — `oneshot` の方式（完了）。** コマンドを呼ぶたびに新しいプロセスを起動する。メニュー項目とコマンドは `plugin.yaml` に書く（`menu_items`、`commands`）。入力と出力は msgpack のファイル（`{input}`、`{output}`）。Python のパッケージは、ランナーで `register(app)` のまま書ける。1 回の失敗でプラグインは外さない。Python 以外の言語向けのプロトコルの手引き（`docs/plugins/plugin-protocol.ja.md`）と、例（`examples/protocol/raw-python`）。

### フェーズ6l — グラフと 3D 表示の連動（進行中）
計算結果のグラフで、グラフの点を選ぶと 3D 表示でも選ばれ、その逆もできるようにする。
- **フェーズ6l-i — 汎用の散布図と、点の選択（完了）。** 下のペインのグラフのタブ、`core` のグラフのデータ型、点のクリックで原子を選ぶ（3D 表示と同じ選択）。開発用のサンプル（View →「Plot: Atom Positions」）が使っている。本物のグラフは、6l-ii と 6l-iii で足す。
- **フェーズ6l-ii — `ProteinDF_pytools` のグラフ。** `ProteinDF_pytools` が書くグラフの JSON を読み、点を原子に結びつけ（原子の数と座標で確かめる）、両方向に選べるようにする。点が原子でないグラフ（軌道、エネルギー準位）は、あとで扱う。
- **フェーズ6l-iii — Ramachandran プロット。** φ/ψ のプロットから残基を選ぶ。

### フェーズ7 — QCLObot連携 & 正準MO可視化研究支援機能（未着手）
- 11章（`.QCLO.yaml` プレイブック、ステップ再生、ツリー/YAML本文/3Dビューの双方向リンク、`modeler.yml` のDAG表示）。
- 12章（MOブラウザ、軌道別寄与マッピング、QCLO収束ビュー、複数計算の比較表示）。
- `proteindf-bridge` の `ProteinDF_pytools` 統合（HDF5読み込み、Mulliken寄与計算、行列演算 — 10章）が利用可能であることが前提。姉妹プロジェクトへの依存度が最も深いフェーズ。

### フェーズ8 — 論文品質出力 & 配布（進行中）
- SSAO/パストレースによるオフラインレンダリング（4.7節）、SVGベクターエクスポート（4.6節）。ここまでに済んだもの: SSAO（オプション。`render.ssao_strength`、既定はオフ。4.7節のフェーズA）、File →「Export Image (SVG)」（原子、結合、相互作用の線、リボン、分子表面）と、File →「Export Image (PNG)」（ビュー全体。幅・なめらかさ・背景透明のダイアログつき。4.6節）。File →「Export Animation」（連番の PNG と、ffmpeg 経由の MP4。回転・首振り・モデルや軌跡のフレーム。デスクトップ版のみ）。Web 版の配備は、ローカルでの利用について対応済み（`scripts/build-web.sh`。backend が `YUI_WEB_ROOT` で配信する。backend の認証ができるまで、ネットワークへの公開は対象外）。ネイティブのインストーラーは、署名なしで対応済み（`scripts/build-installers.sh`、手動実行のワークフロー `release-installers.yml`: Linux は deb と AppImage、macOS は dmg、Windows は NSIS の exe。ここで作って起動を確かめたのは Linux だけ）。ブラウザ版のアニメーションの書き出しは、PNG のフレームの zip として対応済み（メモリに溜める経路と zip は、デスクトップの描画器で動かした。実際のブラウザでの経路は、動かしていない）。まだ: 署名と公証、Windows の msi（カレンダーバージョンが MSI のバージョンの規則に合わない）、rpm。
- ネイティブインストーラ、およびフェーズ6cで得られたWASM `frontend` ビルドの配信・配布（9章）。

---

### 推奨着手順
完了: `6g → 6i → 5c（5c-iii を除く）→ 6j → 6k → 6l-i`。次: `6l-ii` と `6l-iii`（グラフと 3D 表示の連動）、続いて Python コンソール（`6h`。方式を決め直してから）、AI チャット（`6b`）、`7`、`8`。`7` は `proteindf-bridge` 待ち。`5c-iii`（表面の抽出の GPU への移植）と、描画の間引きの第 3 段（遠距離は表面だけ）は、いつでもできる。

### proteindf-bridge 側への依頼事項
- **Gaussian CUBEパーサ（提供済み）:** `proteindf-bridge` 2026.10.5 にあり、フェーズ5c-ivが使っている。実際の ProteinDF の CUBE は、まだ試していない。
- **`ProteinDF_pytools` 統合:** フェーズ7に必要なHDF5読み込み、Mulliken電荷/軌道別寄与計算、行列演算のRust移植。
- **塩橋の官能基中心座標:** `IonPair` / `Interaction` の検出結果において、塩橋の各極（アニオン側・カチオン側官能基）の中心3D座標を出力に含める、または中心座標を返す公開APIの提供（現状はYUI側で残基全体の重心を暫定使用しているため）。
- **§3.14 を含むリリースタグ（対応済み）:** bridge のタグ `2026.10.1` に `AtomGroup::setup()`（コミット `2615f79` 以降）が含まれている。YUI の依存を Git の `rev` 固定からこのタグへ切り替える作業は、§4「未対応の機能」で扱う。
- **MOL2 の substructure（番号と名前）の保持:** `SimpleMol2` のパース時に ATOM 行の substructure（番号と名前）を `AtomGroup` に保持し、分子全体を1残基に集約するのではなく残基単位での表示・選択を可能にしてほしい。
- **複数フレームの軌跡の読み込み:** `proteindf-bridge`（タグ `2026.10.1`）の `Xyz` は、ファイルの先頭の 1 フレームしか読めない（YUI は、独自の索引 `core::xyz_trajectory` で複数フレームの XYZ を読んでいる）。複数フレームの GRO と、MD の軌跡（DCD、XTC、TRR、NetCDF）の読み込みもない。依頼: 複数フレームの XYZ と GRO のフレームの列挙 / ランダムアクセス、DCD / XTC（できれば NetCDF）の、ファイル全体を読まずに、番号で 1 フレームの座標を返す読み込み。これらができるまで、YUI では扱わない。

---

## 4. 技術的負債

今後のリファクタリングで解消すべき技術的負債として以下を認識している:
- **フロントエンドのモジュール肥大化（解消済み）:** `frontend/src/viewport.rs` を `frontend/src/viewport/` サブモジュール（`gpu_types`, `camera`, `mesh`, `scene`, `pipeline`, `primitive`, 外部 `.wgsl` シェーダー）へ、`main.rs` を `app/`（`mod.rs`, `message.rs`, `update.rs`）および `streams/`（`mod.rs`, `native.rs`, `wasm.rs`）へ責務分割。
- **マルチモデル構造の扱い（解消済み）:** `AppState.active_model` によるアクティブモデル選択とモデル単位のフィルタリングに対応。平坦化 `Molecule` キャッシュと非共有結合相互作用の絞り込みを維持しつつ、NMRアンサンブル（20モデルの `1HLS.cif` 等）の単一モデル表示を実現。
- **Undo/Redo の変更履歴設計:** `architecture.md` 8.2節で保留されているEditメニュー（Undo/Redo）のための `AppState` 変更履歴スタックの正式な設計と実装。

### 未対応の機能（ファイルの読み書きなど、2026-10-05時点）

`2026.10.1` の手動テストの準備中に判明した未対応の機能。いずれも、まだ番号付きのフェーズとしては予定していない。
- **`proteindf-bridge` の依存をリリースタグへ切り替える（対応済み）:** Git の `rev` 固定（`997ad24`）から、`AtomGroup::setup()`（§3.14）と mmCIF 書き出しを含むリリースタグ `tag = "2026.10.1"` に切り替え完了。下記の mmCIF の書き出しの前提となる。
- **構造ファイルの書き出し（PDB / mmCIF）（対応済み）:** `proteindf-bridge` の `Pdb` および `SimpleMmcif` を `core::structure_export::export_structure_to_string` へ接続し、File メニューに非同期保存ダイアログを伴う「Export Structure...」を追加完了。マルチモデル構造の全モデル書き出し、非整数部分電荷の適切なハンドリング、ネイティブおよび WASM 両環境での非同期保存に対応。
- **プラグインの仕組みの設計（言語、読み込み、権限、メニューの受け口の利用）（設計済み）:** `architecture.ja.md` 6.4節に設計した（プラグインは別のプロセスで動かす。Python は YUI が同梱するランナーで動かす。利用者が有効にするまで動かさない。8.7節の受け口を使う）。実装はフェーズ6k。
- **次回の起動で、開いていたテキストファイルを復元する:** 開いているファイルは利用者ごとのもので、どこにも保存しないため、アプリを終了すると閉じる。復元するなら、利用者ごとの設定（`architecture.md` 8.6節）か、セッションのファイルとして扱う。
- **ブラウザのタブを閉じるときの、未保存のテキストファイルの確認:** ネイティブ版は終了するときに未保存のファイルを確認するが、ブラウザ版では確認できない（タブを閉じる操作は横取りしない。`beforeunload` など）。
- **画像の書き出しのキャンバス（対応済み）:** SVG、PNG、アニメーションの書き出しで、形（ウィンドウ / 決まった縦横比 / 画素数の指定）と構図（画面のまま / 全体に合わせる）を選べる。`3D pane` の形は、最初の分子図が最後に描かれたときの縦横比を使う（まだ描かれていないときは、ウィンドウの縦横比）。
- **UI からの画像の書き出し（SVG）（対応済み）:** File →「Export Image (SVG)...」で、表示中の原子、結合、相互作用の線（破線）、注釈、リボンと分子表面（CUBE の等値面も）を、陰影をつけた多角形として書き出せる（`architecture.md` 4.6節。画家のアルゴリズムなので、互いに貫く三角形は、重なりが誤ることがある）。論文品質の出力全体（SSAO、パストレース）は、引き続きフェーズ8（4.7節）の範囲とする。
- **backend 経由でのファイルの書き込み:** backend のファイル API は読み取り専用である（`GET /api/fs/list`、`GET /api/fs/read`）。書き込みのエンドポイントを追加する場合は、読み込みと同じパストラバーサル対策と、ロールの判断（少なくとも Editor 以上）が必要になる。
- **分子表面の透明度を変える UI（対応済み）:** Representation 一覧の Surface と Interaction のカードに、Opacity のスライダーがある。

---

## 5. 明示的な非対象（ここでは作らないもの）

- 力場最適化、ドッキング、その他フルスペックの分子モデリングエンジン — 自前実装せず `ModelingProvider`（3章）経由で外部ツールと連携する。
- 独自の選択/描画/チャットコマンドDSL — 常に `Selector`/`Representation`/Python API（6.2節、14.3節）経由とする。
- PDB/mmCIF/MOL2等のフォーマットパーサ、DSSP相当の二次構造推定、非共有結合相互作用検出 — これらは本リポジトリではなく `proteindf-bridge`（10章）の管轄である。
