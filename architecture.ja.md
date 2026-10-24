# YUI — システム構成・設計書 (architecture.md)

*[English version](architecture.md)*

本ドキュメントは、統合可視化プラットフォーム「結 (YUI)」のシステムアーキテクチャ、モジュール境界、データフロー、およびスレッド設計を定義する。開発およびAIエージェントによるコード生成は、本設計書に厳密に従うこと。

---

## 1. 全体システムアーキテクチャ (High-Level Architecture)

「結 (YUI)」は、**データ指向 MVI (Model-View-Intent) アーキテクチャ**を採用し、状態（State）の一貫性と完全な再現性を担保する。`iced` の Elm Architecture (Model-View-Update) は、この設計思想と直接的に対応する。

```
┌───────────────────────────────────┐
│           Input Intents           │
│  (Mouse/UI, Python, Voice via LLM) │
└─────────────────┬─────────────────┘
                   │ AppState Delta (iced::Message)
                   ▼
┌───────────────────────────────────┐
│          YUI Core (Rust)          │
│           [AppState]              │◄──── [ proteindf-bridge (外部crate) ]
└─────┬───────────────────────▲─────┘      構造I/O・モデリング下請け
      │                       │
┌─────┼───────────────────────┼───────────────────┬───────────────────┐
▼     ▼                       ▼                   ▼                   ▼
┌─────────────────┐ ┌─────────────────┐ ┌─────────────────┐ ┌─────────────────┐
│  wgpu Renderer  │ │  File I/O (OS)  │ │ WebSocket        │ │   PyO3/Pyodide  │
│ (Render/Compute)│ │ (YAML/MsgPack)  │ │ (→WebTransport可)│ │ (Python Engine) │
└────────┬────────┘ └────────┬────────┘ └────────┬────────┘ └────────┬────────┘
         │                   │                   │                   │
         ▼                   ▼                   ▼                   ▼
 [Screen / Headless]  [Local/Cluster Disk]  [Cluster/Other Users] [User Command / API]
```

### 1.1 モジュール境界
1. **`core` (Shared Library):**
   - プラットフォームに依存しない純粋なデータ構造（`AppState`）、およびシリアライズ/デシリアライズ（MessagePack, YAML）を担当。
   - 構造データそのものの読み書き・変換・力場非依存の構造操作は自前実装せず、外部クレート `proteindf-bridge`（後述 10章）に委譲する。
2. **`frontend` (Wasm / Native App - クライアント側):**
   - `iced`（Elm Architecture）によるVSCode風のユーザーインターフェース。パネルレイアウトは `iced::widget::pane_grid` を用いた可変分割・ドラッグ可能な構成とする。
   - デスクトップ版のほか、WebAssembly (WASM) 向けビルド（`wasm32-unknown-unknown`）にも対応。ブラウザ上の WebGL2/WebGPU を介したリアルタイム3D描画とUI制御を実行。
   - **WASM ビルドパイプライン & Trunk:** Web エントリーポイントとして `frontend/index.html` を配置し、[Trunk](https://trunkrs.dev/) を用いてビルド・バンドル。暗号論的乱数生成器には `.cargo/config.toml` にて `getrandom_backend="wasm_js"` を構成。ブラウザ DevTools でのパニック通知には `console_error_panic_hook`、ライフサイクルログには `web-sys`（console）を採用。
   - **ネットワーク層の分離とWASM通信層（フェーズ6c-i / 6c-ii）:** ネイティブ専用のネットワーククレート（`tokio-tungstenite`、`tokio` ランタイム）は `[target.'cfg(not(target_arch = "wasm32"))'.dependencies]` に分離。WASM コンパイル時（`cfg(target_arch = "wasm32")`）は、ネットワークストリーム群（WebSocketコラボ、ログ監視、PTYターミナル）を `gloo-net` WebSocket で実装し、`tokio::sync::mpsc` と `wasm_bindgen_futures::spawn_local` によるブリッジを通じて `iced` の `Send` ストリーム要件を満たす。REST API リクエストはブラウザの `window.fetch` をバックエンドとするターゲット非依存の `reqwest` を使用する。
   - Web実行時は、URLクエリパラメータ（`?collab=` または `?backend=`）から接続先を指定可能とし、未指定時は、ページを配信したサーバー自身の `/api/collab`（`ws://{location.host}/api/collab`。https のページは `wss://`。ホストが分からない `file://` などのときだけ `ws://127.0.0.1:8080/api/collab`）に自動フォールバックする。大容量データパースやローカルファイル I/O を直接行わず、Web サーバーから HTTP fetch (REST API) や WebSocket 差分同期を通じてオンデマンドでデータを受信する。
3. **`backend` (HTTP / WebSocket Server - サーバー側):**
   - 大容量 PDB/XYZ ファイル I/O や並列軌跡デコード、等値面 Marching Cubes のバックエンド計算を代行する Rust サーバー。
   - クライアントに対して CORS 対応の REST API（JSON/MessagePack）と、低遅延差分同期用のコネクションを提供する（4.2節・7章参照）。
4. **`renderer-native` (Native CLI App):**
   - ヘッドレス `wgpu` による、描画ウィンドウを持たないバッチレンダリング。高解像度PNG・SVG（ベクター）に加え、論文品質のオフラインレンダリング（4.7節）を担当。
5. **`core-py` (Pythonバインディング — ネイティブ):**
   - `PyO3` を用いて `core` のデータ構造（`AppState`, `Selector`, `Representation`, `AtomGroup`）をPythonへ公開する `cdylib`/`rlib` クレート。
   - `core` をプラットフォーム非依存かつWebAssembly（`wasm32-unknown-unknown`）完全互換に保つため、Python C-APIへの依存を `core-py` に完全に隔離する。ネイティブ環境のスクリプティング（`import yui`）を提供し、14.3節のAIチャットツール呼び出しや11章のQCLObot連携の基盤となる。

---

## 2. データ構造 & 状態管理 (Single Source of Truth)

### 2.1 状態管理の三態
アプリケーションの状態 `AppState` は、以下の3つのフォーマット間でロスレスに相互変換可能とする。
1つの `AppState`（カメラ、`SceneSettings`、構造データ、Representation、アノテーション、軌跡、レーザー
ポインター等、画面に表示されているものを再現するために必要な全て）をこの3形式で相互変換可能に表現した
まとまり全体を、**「YUI Workspace Document」**と呼ぶ。フォーマット全体を指す場合はこの用語を使うこと
（そのうちシーン全体のレンダリング設定サブレイヤー（背景色等）のみを指す `SceneSettings`（2.3節）とは区別する）。

1. **Rust Struct (Memory):** スレッド安全（`Send + Sync`）であり、ポインタコピーを避けるため最小限のデータで構成。
2. **MessagePack (Intermediate / Network / Disk):** 高速・軽量バイナリ。チェックポイント保存・ネットワーク転送（差分同期）に使用。
   - **ヘッダー設計:** `[Magic: "YUI\0" (4B)] + [Version (1B)] + [Compression Flag (1B)] + [Payload]`
   - `proteindf-bridge` のネイティブ往復フォーマット（`.brd`, msgpackベース）と互換性のあるエンコーディング規約を採用し、ProteinDF/QCLObotエコシステムとの相互運用性を確保する。
   - **シリアライズ形式（マップ形式）:** `AppState` の MessagePack ペイロードは、デフォルトの配列（タプル）形式ではなくフィールド名付きのマップ形式（`rmp_serde::to_vec_named`）で書き出される。これは `proteindf_bridge::Interaction` が `None` のフィールド（`distance`, `angle` 等）を `skip_serializing_if = "Option::is_none"` で省略するため、配列形式では位置ずれが生じて破損するためである。デシリアライズ（`unpack_app_state`）は新形式のマップ形式だけでなく、旧形式の配列形式（`interactions` が存在しなかった 2026.10.0 時点の12要素配列を含む）も自動認識して読み込み可能であり、欠落した `interactions` は `#[serde(default)]` により空の `InteractionSet` として安全に補填される。なお、協調差分パケット（`pack_collab_packet`）は `AppStateDelta` に相互作用データが含まれないため、従来どおり `rmp_serde::to_vec`（配列形式）のまま転送され問題ない。
3. **YAML (Interchange / Human):** ユーザーへのエクスポート/インポート用。相対パス解決ルールは既存踏襲。

### 2.2 構造データモデルと `proteindf-bridge` の関係
`AppState` 内の分子構造は、`proteindf_bridge::AtomGroup` をラップする透過的な newtype（`Deref`/`DerefMut` 実装）である `core::atom_group::AtomGroup` によって保持される。MessagePack および YAML へのシリアライズ／デシリアライズは、`proteindf-bridge` の辞書交換形式（`get_raw_data()` / `from_dict_data(&rmpv::Value)`）に委譲する手動実装により担保されている。

プロテインスキーマの規約に従い、階層木構造は以下の4階層で構成される:
**`Model` → `Chain` → `Residue` → `Atom`**（例: `"/model_1/A/5/CA"`）。
各階層の走査は、ハードコードされた深さを仮定するのではなく、`core::atom_group` が提供する正規の深さ走査ヘルパー（`iter_models`, `iter_chains`, `iter_residues`, `iter_atoms`, `iter_flat_atoms`, `parse_residue_key`）を経由して一元的に行う規約とする。

二次構造ラベル（Helix/Sheet/Loop）は、残基パスをキーとする一時的なサイドマップ `SecondaryStructureMap`（`HashMap<String, SecondaryStructureKind>`）によって追跡される。これは `proteindf_bridge::AtomGroup` 側に二次構造専用フィールドが新設されるまでの一時的な代替措置である（`RUST_PORT_SPEC.md` 9章参照）。

結合トポロジーおよび結合次数は、`proteindf_bridge::AtomGroup::setup(&mut self)`（フェーズ6e-vi）によって解決される。これはファイル由来の結合情報（PDB CONECTレコード等）を尊重した上で、CCD（Chemical Component Dictionary）テンプレートによる正確な結合次数（カルボニル C=O 二重結合や芳香環等）を適用し、未結合ペアを共有結合半径ヒューリスティックで補完する。`AppState` の `molecule: Molecule` は、`sync_molecule_from_atom_group()` により `atom_group` から単方向に派生するレンダリング用の平坦化キャッシュである。

マルチモデル構造（NMRアンサンブルや複数モデルとして表現された軌道フレーム、例えば20モデルを含む `1HLS.cif` など）は、`AppState.active_model: Option<String>` を通じて表示フィルタリングが行われる。構造読み込み時（`load_structure_into_state`）に複数モデルが存在する場合（`models.len() > 1`）、先頭のモデルキー（例: `"model_1"`）が自動的に `active_model` に設定され、1モデル以下の構造では `None` となる。`active_model` が `Some(key)` のときはそのモデルに属する原子球・結合棒・リボン・非共有結合相互作用線・ピッキング判定のみが描画され、`None` のときは全モデルが同時に描画される。重要な点として、`Molecule` や `Atom` のデータ形式自体は一切変更せず、平坦化された `Molecule` キャッシュは全モデルの原子を保持し続ける。毎フレームの描画処理で全原子を走査するオーバーヘッド（O(N)）を避けるため、原子とモデルの対応関係は構造変更時に `core::atom_group::calculate_model_atom_ranges` により連続するID範囲のリスト（`ModelAtomRange`）として1回だけ事前計算して保持され、アクティブ原子収集時に O(モデル数) で高速に所属判定される。なお、表示モデルの切り替えはクライアントローカルの視覚的関心事であるため、協調編集の差分同期（`AppStateDelta`）の対象からは除外される。ただし、backend からの接続時における初期状態（`AppState`）取得では、構造データとともに `active_model` も受け取ってローカル状態へ反映される。

`proteindf_bridge::format::SimpleMol2` 経由で読み込まれた MOL2 構造は、分子ノード直下に原子が並ぶ平坦な `AtomGroup` を返す。これを YUI の標準 4 階層（`Model → Chain → Residue → Atom`）へ適合させるため、以下の規則で正規化を行う:
- **Model:** キー `"model_1"`。
- **Chain:** キー `"A"`。
- **Residue:** キー `"1"`。残基名は MOL2 の分子名の先頭3文字を大文字にしたもの（例: `"ethanol"` ならば `"ETH"`）、空または空白の場合は `"UNL"`（Unknown Ligand）。
- **Atom:** 残基グループ直下に MOL2 の原子 ID 文字列（`"1"`, `"2"` 等）をキーとして保持。
- **結合:** ファイル内に `@<TRIPOS>BOND` 節が存在する場合、残基グループ直下に明示的な結合レコードとして保持し、`AtomGroup::setup()` による上書き・追加推定は行わない。BOND 節が存在しない場合のみ `AtomGroup::setup()` を呼び出して共有結合半径ヒューリスティックによる結合推定を行う。二次構造は計算しない（空マップのまま維持）。


### 2.3 `SceneSettings` — シーン全体のグローバルパラメータ
特定の選択や `Representation`（6.2節）に紐づかない、レンダリングされる3Dシーン全体に対するパラメータは、
`AppState` にばらばらのトップレベルフィールドとして持たせるのではなく、`CameraState`/`DataPaths`/
`TrajectoryState` と同じ粒度で専用のサブ構造体 `AppState.scene: SceneSettings` に集約する。これにより
YAML/MessagePack（2.1節）で自然に表現できる構造を保ったまま、新規DSL禁止の方針（6.2節）にも合致する
（パーサではなく、serdeフレンドリーな構造化されたRust構造体である）。現時点では
`background_color: Color4` のみを保持する（既定値は `index.html` のダークテーマ `#121316` に合わせ、
`DEFAULT_BACKGROUND_COLOR` として一箇所で定義）。フォグ・環境光・グリッド/軸表示等は今後この構造体に
追加される想定。`AppStateDelta.scene: Option<SceneSettings>` により、`camera: Option<CameraState>` と
同じ「サブ構造体ごと置き換え」パターンで往復する。本フィールドには `#[serde(default)]` を付与しており、
本サブ構造体導入以前のYAML/MessagePackペイロード（`scene`キーを持たない）を読み込んでもエラーにならず
`SceneSettings::default()` にフォールバックする。

### 2.4 YUI Workspace Document — フィールドリファレンス

**`AppState` のトップレベルフィールド**（`core/src/state.rs`）:

| フィールド | 型 | 用途 |
|---|---|---|
| `version` | `u32` | 状態フォーマットバージョン（`ATOM_GROUP_FORMAT_VERSION = 2`）。インポート時の厳密なデシリアライズ前に検証され（`core::packing::unpack_app_state` / `core::yaml::import_app_state_from_yaml[_str]`）、不一致時は未対応・旧形式であることを示す明確なエラーを出力する。 |
| `camera` | `CameraState` | `eye`/`target`/`up`/`fov` — **1つ目の** 3Dビューポートのカメラ姿勢（8.1節）。そのビューポートでドラッグ・スクロールすると更新され、ワークスペースに保存される。協調編集の差分や backend の初期状態で変わったときは、画面に反映される。2つ目のビューポートのカメラは `AppState` に含まれない。 |
| `scene` | `SceneSettings` | シーン全体のグローバルなレンダリングパラメータ（2.3節）。 |
| `atom_group` | `Option<AtomGroup>` | 構造データのSSOT（2.2節）: `proteindf_bridge::AtomGroup` をラップした newtype（`Model → Chain → Residue → Atom` 木）。空/新規ワークスペースでは`None`。 |
| `secondary_structure_map` | `SecondaryStructureMap` | 残基パスをキーとして二次構造ラベルを追跡する一時的なサイドマップ（`HashMap<String, SecondaryStructureKind>`、2.2節）。 |
| `interactions` | `InteractionSet` | `proteindf_bridge::InteractionSet` による非共有結合相互作用（水素結合、塩橋、CH-π）およびジスルフィド結合のリスト（10.2節）。構造読み込み時に一度だけ検出され、描画側での再計算は行わない。`#[serde(default)]` により旧形式互換、`AppStateDelta` には含めない。 |
| `molecule` | `Molecule` | `{ atoms: Vec<Atom>, bonds: Vec<Bond> }` — `atom_group`から`sync_molecule_from_atom_group()`により単方向に派生する、GPUインスタンシング向けのフラット化キャッシュ。直接編集しても`atom_group`には逆流しない。 |
| `representations` | `Vec<Representation>` | `{ id, selection: Selector, kind: RepresentationKind, color: ColorScheme, opacity, visible, interaction_kinds: Vec<InteractionKind>, hides_atoms: bool }` のリスト（6.2.2節）。選択が重複する場合は後の要素が勝つ（PyMOL方式）。 |
| `named_selections` | `BTreeMap<String, Selector>` | 名前付き選択グループ（PyMOLの `select groupname, ...` 相当）。表示スタイルと1対1に紐づく `Representation.id` とは異なり、複数箇所から再利用可能な名前→Selectorのマッピング。 |
| `annotations` | `Vec<Annotation>` | 3Dテキスト/マーカーアノテーション（`id`, `text`, `position`, `color`, `visible`, 任意の`target_atom_id`）。 |
| `cube_view` | `CubeView` | `{ dataset（0）、isolevel（0 はそのデータセットの既定値）、opacity（0.6）、positive_color、negative_color }` — CUBE のどの軌道（データセット）を、どの等値レベルで見るか、と、等値面の見た目。CUBE のファイルごとに決め、ワークスペースに保存する（ファイルのパスは `data_paths.cube_path`）。YAML: `cube_view: { dataset: 1, isolevel: 0.3, opacity: 0.6, positive_color: 0, negative_color: 1 }`。別の CUBE を開く、CUBE を閉じると既定に戻る。ワークスペースが同じファイルを開き直したときは、そのまま使う（最後のデータセットを超える `dataset` は丸め、負または有限でない `isolevel` は 0 にする）。色は、フロントエンドの色の見本（`cube_view::COLOR_PRESETS`）の番号である。8.1節のカメラと同じく、利用者の設定ではなく状態である。 |
| `data_paths` | `DataPaths` | `{ pdb_path, cube_path }` — オプションの外部ファイル参照。インポート時、YAMLファイル自身のディレクトリからの相対パスとして解決される。 |
| `trajectory` | `TrajectoryState` | `{ file_path, total_frames, current_frame_index, play_speed_fps, is_playing }` — 軌跡再生の状態。 |
| `laser_pointer` | `LaserPointer` | `{ active, position, user_id, color }` — 共同作業中のライブポインター。`AppStateDelta`で同期される。 |
| `active_model` | `Option<String>` | マルチモデル構造（NMRアンサンブル等）における表示対象モデルキー。`None` の場合は全モデル描画（単一モデル時の既定も含む）、`Some(model_key)`（例: `"model_1"`）の場合は原子球・結合棒・リボン・ピッキング・相互作用をそのモデルのみに絞り込んで描画する。`#[serde(default)]` により旧形式互換、クライアントローカルの表示設定のため `AppStateDelta` には含めない。 |

**`AppStateDelta`**（協調差分同期、7.3節）は、同じサブ構造体（`camera`、`scene`、`representations`、
`annotations`、`laser_pointer`、`named_selections`）を`Option<...>`で保持する — 値が存在する場合は「このサブ構造体全体を
置き換える」ことを意味し、サブ構造体内部でのフィールド単位マージは行わない。

**ディスク上の`state.yaml`ファイルは、素の YUI Workspace Document ではない。** `frontend`のFile > Open/Save
（8.2節）は、`SavedWorkspaceState { #[serde(flatten)] app_state: AppState, pane_views: PaneLayout }`（構成は、以前と同じ `pane_views` のマップとして保存する）
（`frontend/src/pane_view.rs`）という形でこれをラップする。そのため上表のフィールドはYAMLのトップレベルに
展開され、そこに各領域のタブレイアウト（8.2/8.3節）を保持する`pane_views`キーが同居する形になる —
このキーはUIのレイアウト情報であり、Workspace Document自身のデータモデルには含まれない。
`import_workspace_from_yaml_str`は旧形式（`pane_views`を持たない素の`AppState`）へのフォールバック
パースも行う。一方、Workspace Documentを直接やり取りするMessagePackの経路（協調差分ストリーム、
`backend`の`/api/state` RESTエンドポイント、`core-py`のPython向けpack/unpack）には`pane_views`は
一切含まれない — このキーはネイティブ/Webフロントエンド自身の保存ファイル形式に特有のものである。

**enumフィールドのYAMLタグ構文。** ペイロードを持たないユニットバリアントは小文字snake_caseの裸文字列
としてシリアライズされる（例: `selection: all`、`color: element`）。データを持つバリアントは
`variant_name:`キーのマップではなく、先頭に`!variant_name`というYAMLタグを付けた形でシリアライズされる
— 例えば`ColorScheme::Uniform([1.0, 0.85, 0.1, 1.0])`は次のようになる:
```yaml
color: !uniform
- 1.0
- 0.85
- 0.1
- 1.0
```
`Selector::ResidueRange { chain: Some("A".into()), start: 1, end: 10 }`は次のようになる:
```yaml
selection: !residue_range
  chain: A
  start: 1
  end: 10
```
Workspace DocumentのYAMLを手編集する場合、データを持つ`Selector`/`ColorScheme`バリアントにはこの
`!タグ`形式を使う必要がある — 単純な`variant_name: {...}`というマップは別の（かつこれらの型では
サポートされない）形である。

**解決済みの制約 — `Selector::Not`のYAMLシリアライズ。** 以前は`Selector::Not(Box<Selector>)`が`Vec`を経由せず
別の`Selector`値を直接ラップするnewtypeバリアントであったため、`serde_yaml_ng`が`"serializing nested enums in YAML is not supported yet"`
エラーを発生させていた。これは`Selector::Not`をnewtypeバリアントから名前付きフィールドを持つstructバリアント（`Not { inner: Box<Selector> }`）
に変更することで解決され、任意のネスト深度で`!not { inner: ... }`としてYAMLを損失なく往復できるようになった。

**MessagePackシリアライズ形式 — マップ形式と旧配列形式の後方互換。**
`AppState` を MessagePack にパッキングする際（`core/src/packing.rs` の `pack_app_state`）、データは無名の配列形式ではなくフィールド名付きのマップ形式（`rmp_serde::to_vec_named`）で書き出される。これは `proteindf_bridge::Interaction` がオプショナルなフィールド（`distance`, `angle`, `donor_acceptor_role`）で `#[serde(skip_serializing_if = "Option::is_none")]` を使用しており、配列形式では要素位置の対応が崩れるためである。`unpack_app_state` は `peek_state_format_version` においてトップレベルのコンテナ形式（Map または Array）を自動判別し、マップ形式に加えて旧来の配列形式（`interactions` が存在せず `#[serde(default)]` により空セットとして復元される 2026.10.0 時点のデータを含む）も正常にデシリアライズできる。なお、協調差分同期パケット（`pack_collab_packet`）は `AppStateDelta` に `interactions` が含まれず最小バイト長が重視されるため、従来どおり `rmp_serde::to_vec`（配列形式）のまま運用される。配列形式の旧データにおいて要素の位置ずれによる値の取り違えやデータ破損を防ぐため、`AppState` に新しいフィールドを追加する際は必ず構造体の末尾に配置し、`#[serde(default)]` を付与する必要がある。

**最小構成の例**（2原子のペプチド断片、デフォルトのRepresentation1件、他は全て`Default::default()`）:
```yaml
version: 1
camera:
  eye:
  - 0.0
  - 0.0
  - 100.0
  target:
  - 0.0
  - 0.0
  - 0.0
  up:
  - 0.0
  - 1.0
  - 0.0
  fov: 45.0
scene:
  background_color:
  - 0.0706
  - 0.0745
  - 0.0863
  - 1.0
atom_group:
  name: mini
  models:
  - id: model_1
    chains:
    - id: A
      residues:
      - id: 1
        name: GLY
        insertion_code: null
        secondary_structure: null
        atoms:
        - id: 1
          name: N
          element: N
          position:
          - 0.0
          - 0.0
          - 0.0
          formal_charge: null
          bfactor: null
        - id: 2
          name: CA
          element: C
          position:
          - 1.5
          - 0.0
          - 0.0
          formal_charge: null
          bfactor: null
molecule:
  atoms:
  - id: 1
    element: N
    position:
    - 0.0
    - 0.0
    - 0.0
    radius: 1.55
    color:
    - 0.2
    - 0.2
    - 0.9
    - 1.0
    residue_name: GLY
    residue_index: 1
    chain_id: A
  - id: 2
    element: C
    position:
    - 1.5
    - 0.0
    - 0.0
    radius: 1.7
    color:
    - 0.5
    - 0.5
    - 0.5
    - 1.0
    residue_name: GLY
    residue_index: 1
    chain_id: A
  bonds: []
representations:
- id: rep_1
  selection: all
  kind: ball_and_stick
  color: element
  opacity: 1.0
  visible: true
named_selections: {}
annotations: []
data_paths:
  pdb_path: null
  cube_path: null
trajectory:
  file_path: ''
  total_frames: 0
  current_frame_index: 0
  play_speed_fps: 0
  is_playing: false
laser_pointer:
  active: false
  position:
  - 0.0
  - 0.0
  - 0.0
  user_id: ''
  color:
  - 0.0
  - 0.0
  - 0.0
  - 0.0
```
（`export_app_state_to_yaml_str`で生成。アプリから保存された`state.yaml`では、上記に加えて前述の
`pane_views`キーが並んで出力される。）

---

## 3. モデリング機能とツール連携

「モデリング」は自前でフル実装せず、**pluggable な `ModelingProvider` 抽象**を介して外部ツールと連携する設計とする。インストールの手間を最小化しつつ、将来的な内製拡張の余地も残す。

### 3.1 `ModelingProvider` 抽象
```rust
trait ModelingProvider {
    fn minimize(&self, ag: &AtomGroup, opts: &MinimizeOptions) -> Result<AtomGroup>;
    fn add_hydrogens(&self, ag: &AtomGroup) -> Result<AtomGroup>;
    // ...
}
```
実装バックエンドは設定ファイル（YAML）で切り替え可能とする:

1. **ローカル実行ファイル:** 実行パス＋引数テンプレートを設定（例: OpenBabel `obminimize`）。
2. **HTTP API:** エンドポイントURLを設定（Docker化したOpenBabel/RDKit等をローカル/リモートで起動）。
3. **内製（`proteindf-bridge` 委譲）:** 水素付加・イオン対付与・SSBond付与・末端キャッピング（ACE/NME）など、`proteindf-bridge` の `Modeling`/`IonPair`/`SSBond`/`Neutralize` 相当の構造前処理はここで完結させる。本格的な力場最適化・ドッキングは対象外とし、上記1/2に委譲する。

インストール負荷の軽減策（OS別プリビルドバイナリ同梱、Dockerコンテナ配布等）は、配布方式（9章）確定後に別途検討する。

---

## 4. レンダリング・パイプライン (WebGPU)

大規模複合体を低スペックPCやモバイルでも快適に描画するため、以下の技術を採用する。**目標スケールは最大約100万原子**（既存の1AON：約58,000原子から大幅に引き上げ）。

### 4.1 ジオメトリ・インスタンシング
原子（球）、スティック（円柱）はマスターメッシュをGPUメモリ上に保持し、インスタンスデータを `wgpu::Buffer` 経由で一括転送、1パスで並列描画する。

### 4.2 GPU駆動レンダリング（100万原子対応）
単純インスタンシングのみでは100万原子規模でフレームレートが破綻するため、以下を追加する。
- **Indirect Draw + Compute Culling:** Compute Shaderで視錐台・オクルージョンカリングを行い、描画対象インスタンス数をGPU側で事前に絞り込む。
  - *視錐台平面の抽出:* Gribb-Hartmann法により、`view_proj` 行列から6枚の正規化クリッピング平面（Left, Right, Bottom, Top, Near, Far）を直接抽出する。wgpuのNDC（正規化デバイス座標系）深度空間 `[0, 1]` に適合（Near: $z' \ge 0$, Far: $z' \le w'$）。
  - *包括球カリングとコンパクション:* WGSL Compute Shader（`cs_cull`）内で、各インスタンス行列 $M$ から包括バウンディングスフィアの中心（$M_{3.\text{xyz}}$）と保守的な包括半径（$\sqrt{\|M_{0.\text{xyz}}\|^2 + \|M_{1.\text{xyz}}\|^2 + \|M_{2.\text{xyz}}\|^2}$）を動的に算出し、原子球・結合円柱の双方で欠落（false negative）のない包括判定を行う。
  - *GPUコンパクションと間接描画:* 6平面すべてで可視と判定されたインスタンスのみを、`atomicAdd(&indirect_args.instance_count, 1u)` によりコンパクション済み頂点バッファへ隙間なく書き込み、生成された間接描画引数バッファを `wgpu::RenderPass::draw_indexed_indirect` に直接渡すことで、CPU読み戻しによる同期ストールを起こさずGPU単体でカリング・描画を完結させる。
- **空間分割:** 原子座標集合に対するBVH（Bounding Volume Hierarchy, AABB木）を導入し、O(n²)の総当たり処理を排除する。
  - *データ構造の選定理由:* 正則Octreeではなく、最長軸中央値分割によるAABB（Axis-Aligned Bounding Box）ベースのBVHを採用。生体分子は細長い繊維状やドメイン間隙など非一様・非等方的な空間分布を持つため、空間を立方体等分割するOctreeでは空ノードの急増や探索木の偏りが生じやすいのに対し、適応的なBVHは疎なクラスターに密着した境界箱を構築でき枝刈り効率が高い。さらに各ノードのAABB構造は、フェーズ5b-iiで導入するカメラ視錐台カリングやGPUバッファへの平坦化転送とも整合性が高い。
  - *近傍ペア探索:* 共有結合の自動検出や接触ペア列挙には、2つのノード間のAABB最短距離が `max_dist` を超える枝を刈り取る再帰的ツリー走査（`query_pairs_within_range`）を用い、平均計算量 O(n log n) を実現する。
- **LOD:** 近距離=ball-and-stick、中距離=インポスター（ビルボードスプライト）、遠距離/ズームアウト時=表面のみ、という段階的LODを実装する。
  - *2段階LODの実装仕様（フェーズ5b-iii）:*
    - *近距離（$\le 65.0\text{ \AA}$）:* 原子球・結合円柱ともにフルジオメトリマスターメッシュで描画。
    - *中距離（$> 65.0\text{ \AA}$）:* 原子球はカメラ正対ビルボード単位クワッドによるインポスター描画に切り替え。フラグメントシェーダーで円外ピクセルを discard（$u^2 + v^2 > 1.0$）し、手前向き法線成分（$N = u R + v U + \sqrt{1 - u^2 - v^2}(-F)$）を再構成して同一のディレクショナルライティングを適用。スティック（結合）はプリミティブ過多を抑制するため中距離では自動的に非表示（カリング）とする。フラグメントシェーダーは、クワッド平面ではなく球の表面の深度（`frag_depth`）を書くので、スティックや他の原子と球が正しく交差する。
    - *GPU2系統コンパクション & 間接描画:* WGSL Compute Shader（`cs_cull`）にカメラ座標 `camera_pos` と距離二乗判定（$d^2 \le \text{lod\_threshold\_sq}$）を追加。視錐台カリングを通過したインスタンスを近距離用・中距離用の各インスタンスバッファにアトミックカウンタで振り分けコンパクションし、CPU読み戻しなしで2系統の `draw_indexed_indirect` により一括描画。
    - *第3段階（遠距離=表面のみ）:* 分子表面描画機能（フェーズ5c）に依存するため、フェーズ5c完了までの保留（ギャップ）として文書化。
- **ストリーミング:** 100万原子をフラット配列で常時オンメモリに保持せず、チャンク単位でのロード/破棄を前提とする。

### 4.3 リボン（カートゥーン）表示
`proteindf-bridge`（または代替 `AtomGroup`）が計算した二次構造ラベル付きバックボーン座標列を、Catmull-Rom等のスプラインで補間し、リボン/チューブメッシュとしてGPUに送る。
- **バックボーン抽出と選択解決:** `Representation.selection` に一致するCα原子を持つ残基を鎖（Chain）ごとに収集する。
- **スプライン補間:** 鎖ごとに連続するCα座標列に対し、Catmull-Romスプラインによる残基間細分化（8〜12ステップ）と、Parallel Transport（平行移動）フレームによるねじれ防止を行ってサンプリング点列と接線・法線ベクトルを算出する。
- **断面プロファイル（形状）の切り替え:**
  - *Helix（αヘリックス）:* 幅広の楕円断面（例: 幅 1.3 Å、厚み 0.3 Å）。
  - *Sheet（βシート）:* 平たいリボン断面（例: 幅 1.2 Å、厚み 0.15 Å）。
  - *Loop（ランダムコイル/ループ）:* 細い円筒チューブ断面（例: 半径 0.25 Å）。
- **描画とフォールバック規則:** `AppState.representations` が空の場合は後方互換性のため全原子の球棒（ball-and-stick）表示を行う。`Cartoon` 表現が指定されている場合は、生成されたリボンメッシュを頂点カラー（`ColorScheme::SecondaryStructure`、`Chain`、`Uniform` 等に対応）とともに同一の深度テスト付きレンダーパスで一括描画する。
- **カメラを構造に合わせる:** 構造を開いたとき、ワークスペースを開いたとき、File → New、backend から初期状態が届いたとき、View → Reset Camera で、カメラを表示中の構造に合わせる。マルチモデルの構造で表示中のモデルを切り替えたときは合わせない（視点が飛ばないようにするため）。合わせ方（`viewport::camera::fit_camera_to_points`）: 表示中のモデル（`active_model`。全モデル表示なら全原子）の原子を対象とし、注視点はそのバウンディングボックスの中心とする。カメラは +z 側（−z 方向を見る、up は +y）に置き、注視点を中心とする外接球（原子の球の分として 2 Å を足し、1.1 倍の余白を掛けたもの）が、分子図の縦と横の画角のうち狭い方に収まる距離にする（縦長・横長のどちらのペインでも収まる）。原子がないときは既定のカメラにする。合わせる要求はトークンで伝える（`Yui::request_camera_fit` が `camera_fit_token` を進める）。分子図の縦横比を知っているのは分子図のウィジェットだけなので、ウィジェットが新しいトークンを見たときにカメラを計算する。大きな構造（例: backend のデモの 1AON、約58,000原子）が切れないよう、ズームの距離の範囲を 2〜5000 Å（以前は 2〜500 Å）とし、投影の far を `max(1000 Å, カメラの距離 × 4)`（以前は 1000 Å の固定。near は 0.1 Å のまま）とした。視錐台カリングの平面は同じ投影から作るので、これに従う。合わせた結果は協調編集では送らない。カメラの置き場所と、合わせる要求を各分子図へ届ける方法は 8.1 節（「2つ目の分子図と視点の連動」）に書く。分子図が作り直されても（例: 中央のタブをテキストファイルに切り替えて戻したとき）視点は保たれ、合わせ直さない。ワークスペース（または backend の初期状態）を読み込んだときは、保存されていたカメラが既定のカメラでなければそのまま使い、既定のカメラなら（古い版は動かした視点を保存しなかった）これまでどおり構造に合わせる。
- **起動時の状態と既定の表示設定:** フロントエンドは何も読み込まずに起動する。File → New と同じ空の状態（`menu_bar::create_empty_state()`: 構造も表示設定もない）である。何も読み込まれていない間は、中央の分子図に案内「No structure loaded. Open a file from the Project tab or File > Open.」を表示する。`atom_group` があるか、平坦化キャッシュ `molecule` に原子があれば、構造が読み込まれているとみなす（backend のデモの状態は `molecule` だけを持つことがある）。`core::structure_loader::load_structure_into_state` で構造を読み込んだ後に `representations` が空であれば、`representation::default_representations()` の既定の2つの表示設定を追加する: `cartoon_default`（`Cartoon`、`ColorScheme::SecondaryStructure`）と `ball_and_stick_default`（`BallAndStick`、`ColorScheme::Element`）。既にある表示設定（利用者が作ったもの、ワークスペースから読み込んだもの）は変更しない。読み込みに失敗したときは状態を変更しない。この規則を `core` に置くのは、読み込みの経路（Project タブのローカルのファイル、backend 経由、将来の Python API などの呼び出し元）によらず同じ動作にするためである。backend から取得する初期状態（`state_sync::merge_initial_state`）では、backend の表示設定をそのまま使う。

### 4.4 分子表面表示
SES/SAS相当の分子表面は、3次元ガウス密度場を構築し、Marching Cubes パイプライン（4.5節）によって等値面メッシュを抽出する。
- **密度の定式化と出典:** ガウス関数の和としての密度場の関数形は、VMD の QuickSurf アルゴリズムの定式化を採用する（Krone et al. 2012、VMD User's Guide, QuickSurf）：
  $$\rho(\mathbf{r}) = \sum_{i=1}^N \exp\left( -\frac{\|\mathbf{r} - \mathbf{r}_i\|^2}{2 \alpha_i^2} \right)$$
  - *出典:* Michael Krone, John E. Stone, Thomas Ertl, Klaus Schulten, *"Fast Visualization of Gaussian Density Surfaces for Molecular Dynamics and Particle System Trajectories"*, EuroVis 2012 - Short Papers, pp. 67–71, 2012. DOI: 10.2312/PE/EuroVisShort/EuroVisShort2012/067-071; VMD User's Guide, QuickSurf (https://www.ks.uiuc.edu/Research/vmd/vmd-1.9.3/ug/node73.html).
- **パラメータの導出と既定値（YUI 独自の選択）:** ガウス関数の幅 $\alpha_i$ の決定法は QuickSurf 論文のものではなく、YUI 独自のパラメータ決定法である。van der Waals 半径 $r_i$ と溶媒プローブ半径 $r_{\text{probe}}$ を持つ各原子 $i$ に対し、想定される表面半径を $R_i = r_i + r_{\text{probe}}$ とする。孤立した単一原子の等値面が距離 $R_i$ において閾値 $C$ と一致するように、$\alpha_i = \frac{R_i}{\sqrt{2 \ln(1/C)}}$ と解析的に決定する。
  - *格子間隔 (grid spacing):* 0.5 Å（既定値、変更可能）。
  - *プローブ半径 (probe radius):* 0.0 Å（既定値、PyMOL の surface や VMD の QuickSurf に近い vdW 包絡面の外観に準拠。溶媒接触表面 SAS / 溶媒排除表面 SES の計算時には 1.4 Å などのカスタム値を指定可能）。
  - *等値面レベル ($C$):* 0.5（既定値、このとき $\alpha_i = R_i / \sqrt{2 \ln 2} \approx R_i / 1.17741$）。
- **速さ（`core::surface`）:** ガウス関数は軸ごとの積に分けられる（$e^{-(dx^2+dy^2+dz^2)/2\alpha^2} = e_x e_y e_z$）ので、原子ごとに 3 つの小さな 1 次元の表を作り、内側のループは掛け算だけにした（格子点ごとの `exp` がない）。ネイティブ版では、密度の格子を z 方向のスラブに分け、`rayon` で並列に埋める（各格子点には、これまでと同じ順で原子が足されるので、結果は直列と同じ）。WASM 版は 1 スレッドで、スラブ 1 つを埋める。Marching Cubes では、勾配を、頂点ができる辺の両端でだけ計算し、辺と頂点の対応表には、軽い（暗号用でない）ハッシュを使う。頂点の最近傍の原子の対応付けも、ネイティブ版では並列にした。速くする前と、メッシュは同じ（頂点、並び、インデックスが同じ。指紋で確認した）。実測（リリースビルド、タンパク質並みの密度の擬似乱数の原子の雲、5 万原子）: この機械で約 2.6 秒 → 0.3 秒。`cargo test --release -p yui-core --test surface_test -- --ignored --nocapture` で、時間の報告が出る。
- **計算量と領域制限:** 寄与が無視できるカットオフ距離 $d_{\text{cutoff}, i} = \alpha_i \sqrt{2 \ln(1/\epsilon)} \approx 3.255 \alpha_i$（$\epsilon = 0.005$）の範囲内の格子点にのみ密度を加算し、$O(N \cdot \text{grid})$ の総当たりを回避する。格子の定義域は全原子座標の AABB に $\max(d_{\text{cutoff}}) + 2 \times \text{spacing}$ の余白を加えた領域とする。
- **頂点と原子の対応付けおよび配色:** 生成された各メッシュ頂点は、`AtomBvh::query_nearest` を用いて最寄りの原子 ID と対応付けられる。頂点カラーは `ColorScheme`（`Uniform`、`Element`、`Chain`、`Residue`、`SecondaryStructure`）に従って解決され、原子球棒やリボン描画と共通のカラーパレット関数を再利用して重複定義を避ける。
- **表面原子の抽出とマルチモデル対応:** `Representation.selection` による絞り込みに加え、マルチモデル（NMR 構造やトラジェクトリ）では `AppState.active_model` と `model_atom_ranges` を用いて現在のアクティブモデルに属する原子のみを抽出して表面を生成する。
- **透明度と描画パス:**
  - 分子表面のポケットや内部空洞を観察できるよう、リボンと同様に両面描画（`cull_mode: None`）を採用する。
  - `opacity >= 1.0` の場合、深度書き込みを有効にした不透明パスで描画する。
  - `opacity < 1.0` の場合、不透明なオブジェクト（球棒、リボン、相互作用線など）をすべて描画した後に、深度書き込みを無効化したアルファブレンド（`BlendState::ALPHA_BLENDING`）で描画する。個々の三角形の奥から手前へのソートは行わないため、複雑な凹凸形状で視角により重なり順の見た目の乱れが生じるが、既知の制約として許容する。
- **メッシュ生成のライフサイクルと UI 非同期生成:**
  - 描画ごとのメッシュ再生成は行わず、構造（`atom_group`）、表示モデル、または `RepresentationKind::Surface` の選択・可視性・パラメータが変更されたときのみ、`extract_surface_geometry_signature` によるシグネチャ比較で差分検知して非同期再生成をトリガーする。色や透明度のみの変更時は既存のメッシュを再利用し、頂点カラーバッファのみを同期的に再計算・更新する。Representation 一覧には、Surface と Interaction のカードにだけ Opacity のスライダー（0〜100 %。アクション `ChangeOpacity`、反映は `representation_inspector::set_opacity`）を出す。`opacity` を使う表示形式がこの 2 つだけだからである。ドラッグしてもジオメトリのシグネチャは変わらないので、メッシュは作り直されない。
  - ネイティブ環境では、UI スレッドを停止させないよう iced の非同期 `Task`（`tokio::task::spawn_blocking`）でバックグラウンド生成を行い、世代番号（`SurfaceGenerationTracker`）を付与して照合することで、高速な連続変更時に古い生成結果が新しい状態を上書きすることを防止する。シングルスレッド環境の WASM では、状態更新時に同期的に生成を行う。

### 4.5 Compute Shader による等値面（Isosurface）生成
スカラー場（分子表面のガウス密度、および ProteinDF の電子密度/分子軌道 .cube データ）からの等値面抽出は、以下の2段階で設計する（CUBE 入力の最初の版: `proteindf_bridge::format::Cube` がファイルを読み、`yui_core::cube_iso::cube_to_grid` が 0 番目のデータセットを Å 単位の `ScalarGrid` にし（Bohr の値は `BOHR_TO_ANGSTROM` で換算。x, y, z 軸に平行で間隔が正の格子はそのまま使い、斜めの格子や軸が負の向きの格子は、元の格子を覆う軸に平行な格子に三線形補間で再サンプリングする。間隔は最も短い格子ベクトルの長さで、800 万点を超えるなら粗くし、元の格子の外は 0。`resample_oblique`）、`extract_signed_isosurfaces` が、同じ Marching Cubes を `+level` で、符号を反転した格子で `-level` として実行する。既定のレベルは、負の値がないデータ（密度）で 0.05、正負があるデータ（軌道）で 0.02 で、データの最大値がそれより小さいときは最大値の半分。Right ペインの上のカードに、ファイル名、軌道の CUBE では軌道の選択（軌道ごとに 1 つのデータセット。`CubeSet::orbital_number`）、最大値の 1/10000 から最大値までの対数のスライダーで等値レベルを出す。計算（格子の変換、抽出）はバックグラウンドで行い、計算中の変更は 1 つずつ積まない。終わったとき、表示したいデータセットやレベルが違えば、もう一度だけ計算する。`frontend/src/cube_view.rs` が、正の面を青、負の面を赤、不透明度 0.6 にして、分子表面の隣で半透明のパスで描く。CUBE の面は構造とは独立。CUBE を開いたときに構造を開いていなければ、CUBE の原子を構造として読み込む（`structure_loader::load_cube_atoms_into_state`。結合は距離から、二次構造なし）。開いている構造は置き換えない。カードには、正の面と負の面の色の見本もある（`COLOR_PRESETS`。色を替えても面は計算し直さない）。ネイティブ版では、ファイルの絶対パスを `AppState.data_paths.cube_path` に持ち、ワークスペースの YAML から開き直せる（File → New とワークスペースを開くときは、いま開いている CUBE を閉じ、新しい状態が指す CUBE を開く。パスは保存した機械の文字列のままで、相対パスにはしない）。ブラウザ版はファイルのパスがないので、開き直さない）：
- **CPU 参照実装 (`core::surface`, フェーズ5c-i):** 正しさをテストで厳密に検証可能な正解グラウンドトゥルースとして、まず `core` に先行実装（球の表面積・体積の誤差 $\pm 5\%$ 以内、閉じた2様体判定、連結成分の検証）。任意のスカラー場を受け取れる汎用 `ScalarGrid`、共有辺の頂点インデックス統合、中心差分勾配による外向き法線計算を備える。WebGPU Compute Shader が利用できない環境でのフォールバックとしても維持する。
  - *Marching Cubes 表の出典:* William E. Lorensen and Harvey E. Cline, *"Marching Cubes: A High Resolution 3D Surface Construction Algorithm"*, ACM SIGGRAPH 1987 (DOI: 10.1145/37401.37422) に基づく Paul Bourke (1994), *"Polygonising a scalar field"* の標準 256 エントリ辺・三角形ルックアップテーブルを採用。
- **GPU Compute Shader (`renderer`, フェーズ5c-iii):** WebGPU の Compute Shader（ワークグループサイズ `[8, 8, 4]`）へ移植し、同一のトポロジーと幾何が得られることを CPU 参照実装の結果を正解として検証する。

### 4.6 高解像度 SVG ベクターエクスポート
File →「Export Image (SVG)...」で、今のビューを書き出す。`frontend/src/svg_export.rs` が、画面と同じ `style_layers`（表示中の表現、その配色、表示中のモデル）で描くものを決め、原子（球）、結合（スティック。`styled_stick_segments`）、注釈の文字を `yui_core::render_svg`（`core/src/export.rs`）に渡す。`render_svg` は SVG を文字列で返し、保存は呼び出し側がファイルダイアログで行うので、同じコードがブラウザ（ダウンロード）でも動く。行列は描画と同じもの（`viewport::camera::view_proj_matrix`）を使う。画像は幅 1600 px で、高さはウィンドウの縦横比に合わせる（3D ペイン自身の大きさはアプリが持っていないので、画面と構図が少しずれることがある）。
- **投影:** 点は `x = (clip_x / w * 0.5 + 0.5) * width`、`y = (1 - (clip_y / w * 0.5 + 0.5)) * height` になる。`w <= 0` の点はカメラの背面なので描かない（結合は、どちらかの端がそうなら描かない）。
- **半径:** 奥行き `w` にある長さ `r`（Å）は、`r * f / w * height / 2` ピクセル。`f = 1 / tan(fov / 2)` で、`view_proj` の `y` の行（xyz 成分）のノルムに等しい。結合の線の太さは、スティックの半径のぶんの 2 倍。これは球の中心を通る面での半径で、広い視野の端にある球の本当の輪郭は、少し大きくなる。
- **画家のアルゴリズム:** 原子と結合をまとめて、`w` が大きい（奥の）ものから順に並べて描く（結合は両端の平均）。注釈の文字は最後に描く。
- **相互作用の線:** 非共有結合相互作用の線（水素結合、塩橋、CH-π、ジスルフィド）は、画面と同じ絞り込み、色、透明度で（`interaction_render::build_interaction_lines_from_resolved`）、破線（`stroke-dasharray`、端は平ら）として書き出す。太さは 3D の線と同じ 0.08 Å。原子や結合といっしょに、奥から順に並べる。破線の長さは線の太さに比例する（3 倍と 2 倍。最小は 4 px と 3 px）。破線の模様は SVG の定数で、画面のシェーダーの模様とは違う。破線は 1 本の `<line>` なので、手前の原子との前後は、画家のアルゴリズムの順だけで決まる。
- **メッシュ（リボン、分子表面、CUBE の等値面）:** 画面が描く三角形のメッシュを、`<polygon>` として書き出す（`SvgMesh`: 頂点ごとの位置・法線・RGBA の色。`svg_export::build_scene` が、カートゥーンを `ribbon::build_all_cartoons` から、表面を画面と同じ `SurfaceBatches` から取るので、同じ表現、配色、不透明度に従う。ブラウザ版の再生中は、画面と同じく表面を入れない）。三角形は 1 色で、頂点の色の平均に、画面の光、`0.35 + 0.65 * max(dot(n, l), 0)`（`l` は画面の固定の世界座標の光。面の法線は頂点の法線の平均で、`fs_main` と同じく、背面でも裏返さない）を掛ける。不透明な三角形は、原子・結合といっしょに（`w` の平均で）奥から並べ、隣との間に細い隙間が見えないよう、同じ色の細い線（0.6 px）を引く。不透明な分子表面は閉じているので、背面（法線がカメラと反対を向く。`SvgScene::eye`）を省き、ファイルがおよそ半分になる。リボンと半透明の表面は、両面を残す。`alpha < 1` の三角形は、画面が半透明のパスを描くのと同じく、不透明な形のあとに、奥から順に、線なしで描く。三角形の山に画家のアルゴリズムを使うことの限界: 互いに貫く三角形や大きな三角形は、重なりが誤ることがある（リボンの中の原子、表面を貫くリボン）。隣り合う半透明の三角形の縁に、薄い継ぎ目が見えることがある。気になるときは、PNG の書き出しを使う。ファイルの大きさはメッシュに比例する（1HLS: カートゥーン 0.8 MB、表面 2.2 MB、半透明の表面とカートゥーンで 4.4 MB）。`MAX_SVG_TRIANGLES`（50 万。背面を省いたあとの数）を超えると、表面を隠す、`render: surface_grid_spacing`、PNG を勧めるエラーになる。選択の強調は書き出さない。
- **キャンバスの大きさと構図（SVG、PNG、アニメーション）:** 3 つの書き出しのダイアログは、`canvas::CanvasOptions` を共有する。*形*: **3D pane**（最初の分子図が画面に最後に描かれたときの縦横比。`ViewportProgram::draw` が `bounds` の大きさを `ViewportSizes` に残し、`Yui::export_canvas` が読む。まだ何も描かれていないときは、ウィンドウの縦横比）、1:1、4:3、3:2、16:9、または Custom（幅と高さを入力。一辺 16〜16384 px。そのときは、ダイアログの幅の候補は使わない）。*構図*: **As on screen** は、画面のカメラと縦の視野角のまま（縦長のキャンバスでは左右が切れることがあり、横長では左右が広く見える）、**Fit all**（`canvas::fit_keep_direction`。`fit_camera_to_points` を使う）は、カメラの向き、up、視野角を保ったまま、注視点を表示中の構造の中心にし、距離を、外接球（原子の余白つき）がキャンバスの短いほうの辺に収まるように合わせる。`Yui::export_canvas` が大きさとカメラを返し、SVG（`render_view_svg(.., camera, size)`）、PNG（`offscreen_primitive_with`）、アニメーション（Spin / Rock の基準のカメラ。大きさは偶数にする）が使う。SVG のダイアログ（新しい）には、幅（800〜3200 px）の選択もある。アダプターのテクスチャの限界を超える PNG は、メッセージを出して失敗する（なめらかさは、先に自動で下がる）。
- **PNG（ラスター）の書き出し:** File →「Export Image (PNG)...」は、ビューそのものを描くので、画面にあるもの（球、スティック、カートゥーン、分子表面と CUBE の等値面、相互作用の線、背景）がすべて入る。選択の強調は入れない。`Yui::offscreen_primitive` が画面と同じ `ViewportPrimitive`（`ViewportProgram::draw`、アプリのカメラ）を作り、`viewport::offscreen` が、専用の wgpu のインスタンスとデバイス（どのアダプターでもよく、ソフトウェアのものでもよい）で、`Rgba8UnormSrgb` のテクスチャに描き、読み戻し（行を 256 バイトの倍数にそろえて取り、詰める）、`png_export` がファイルダイアログのあとに保存する。メニューの項目は、まず小さなダイアログ（`png_export::view_png_dialog`、設定は `PngOptions`。次の回のために覚えておく）を開く。幅は 1000 / 2000 / 3000 / 4000 px（高さはウィンドウの縦横比から決める。SVG と同じく、3D ペイン自身の大きさはアプリが持っていない）、なめらかさは Off / 2x / 3x、背景は Scene color / Transparent。なめらかさは、その倍率で大きく描いて平均して縮める（アンチエイリアス）。平均は線形の光で取り（`downsample_box`）、倍率は、描く大きさがアダプターのテクスチャの限界と 6400 万画素に収まるまで、1 ずつ下げる（`fit_supersample`）。背景が透明のときは、背景の描画を飛ばし（`ViewportPrimitive::draw_background = false`）、描画先を透明に塗りつぶす。GPU のアルファブレンドの結果は、色に透明度が掛かった形になるので、`downsample_box` が、平均して透明度で割り、通常の透明度つきの PNG にする（なめらかさ Off のときも行う）。GPU の処理と読み戻しの待ちは、ネイティブ版では `spawn_blocking` で行い、ブラウザ版では await する（読み戻しはブラウザが進める）。画面なしの描画テスト（`viewport/headless.rs`、TESTING 7章）も、同じ `offscreen` を使う。
- **アニメーションの書き出し（デスクトップ版はフォルダ、ブラウザ版は zip）:** File →「Export Animation...」は、連番の PNG（`frame_0001.png`, ...）をフォルダに書き出し、`ffmpeg` が PATH にあれば、`movie.mp4`（H.264、yuv420p）まで作る（なければ、そのまま使えるコマンドをステータスバーに出す）。ダイアログ（`movie_export::view_movie_dialog`、`MovieOptions`）で選ぶのは、動き（**Spin**: 画面の縦軸のまわりに視点を 1 周させる。`camera_for_frame` と `orbit_camera`。最後の次が最初に戻るので、ループできる。**Rock**: 8.11節の首振りの 1 周期。**Frames**: マルチモデル構造のすべてのモデル、または XYZ の軌跡のすべてのフレームを、先頭から末尾まで 1 回）、フレームレート（15 / 24 / 30）、Spin と Rock の長さ（4 / 8 / 12 秒）、幅（960 / 1280 / 1920 px。エンコーダーのために幅と高さは偶数にする）、なめらかさ（Off / 2x）、MP4 を作るか。フレームは、`app/movie_ops.rs` の状態機械が 1 枚ずつ進める。UI のスレッドで、視点を決め（または `show_frame` でフレームを表示し）、画面と同じ `ViewportPrimitive` を作り（`offscreen_primitive_with`）、描画、縮小、ファイルへの書き出しは、共有の `OffscreenRenderer`（GPU のデバイスとパイプラインは、1 回の書き出しで 1 度だけ作る）で `spawn_blocking` で行い、終わったら次の 1 枚を始める。ペインの上のバーに進み具合と取り消しのボタンを出す。取り消しても、書いたフレームは残し、Frames では表示中のモデルを元に戻す。分子表面は、どの動きでも入る。Spin と Rock では今のものを使い、Frames では、フレームごとに、そのフレームの原子で、描く直前に、描画と同じスレッドの中で作り直す（`SurfaceWork`: `show_frame` のあとに `Yui::surface_build_requests` が作る生成の依頼と、書き出しごとに 1 度作る、頂点の色のための所有する `SurfaceColorContext`。原子の ID、元素、鎖、残基、二次構造はフレームが変わっても変わらない）。1 フレームごとに分子表面を 1 回作るので（1HLS の 782 原子で、リリースビルドで分子表面 1 つにつき約 43 ms。以前に測った値）、分子表面があるときの Frames の書き出しは、その分遅い。相互作用の線と CUBE の面は、これまでどおり。選択の強調と透明な背景は使わない。進行中は、ウィンドウを操作しないこと。**ブラウザ版:** フォルダも `ffmpeg` もないので、同じダイアログ（MP4 の行はなく、ボタンは「Start」）が、PNG をすべてメモリに溜め（`MovieJob::to_memory`、`frames`、`bytes_kept`）、最後のフレームのあと、`zip_store::build_zip`（圧縮しない「保存のみ」の zip、CRC-32 つき、依存なし。ファイルは最大 65,535 個、全体で 4 GiB 未満）で、1 つの `yui-animation-<version>.zip` にまとめ、保存のダイアログ（`movie_export::save_zip`。PNG の書き出しと同じ方法）でダウンロードさせる（そのあと、ステータスバーに、展開したフレームから動画を作る `ffmpeg` のコマンドを出す）。溜める PNG は 400 MB まで（`WEB_MAX_FRAME_BYTES`。zip にするときに、同じ大きさがもう一度要る）で、超えそうになると、幅を小さくする、フレームを減らす、デスクトップ版を使う、という案内を出して止まる。ほかは、デスクトップ版と共通である。描画器（`OffscreenRenderer` は、デスクトップ専用ではなくなった。ブラウザでは、ブロックするスレッドではなく `await` で動かす。GPU の読み戻しはブラウザが進めるため）、フレームの計画、Frames の分子表面（同期的に作るので、実行中はページが忙しい）、スーパーサンプリング、3D ペインの縦横比、SSAO。コードは両方のターゲットで同じ: `render_frame_png` が PNG のバイト列を返し、デスクトップ版の `render_frame_to_file` がそれをファイルに書く。ブラウザでの経路そのもの（実際のブラウザの WebGPU、ダウンロード）は、開発者は動かしていない。メモリに溜める経路は、デスクトップの描画器で動かし、zip を Python の `zipfile` と `unzip -t` で確かめた。

### 4.7 論文品質オフラインレンダリング（段階導入）
- **フェーズA（優先。SSAO はオプションとして対応済み。既定はオフ）:** ラスタライズ + SSAO（Screen Space Ambient Occlusion）。安価でリアルタイム/オフライン双方に転用可能。`shaders/ssao.wgsl` は、3D のパスのあとの、別の全画面の描画パス（`primitive.rs` の手順 8）として動き、設定ファイルの `render.ssao_strength` が 0 より大きいときだけ動く（既定は 0 なので、有効にしない限り何も変わらない）。3D のパスが書いたばかりの深度テクスチャ（`Depth32Float`）を、フィルターなしの float の `texture_2d<f32>` として `textureLoad` で読む（`texture_depth_2d` への `textureLoad` は、GL バックエンド、つまり WebGL2 と Mesa のソフトウェア描画では動かないため）。色は読まない。出力は、今の色に*掛ける*係数（ブレンドは `src * 0, dst * src`。アルファはそのまま）なので、暗くするだけで、3D ペインの中（ペインのビューポートとシザー）だけに効き、背景（深度 1.0）には効かない。ピクセルごとに、深度から視点座標系の位置を求め（`near`、`far`、視野角、ペインの大きさを `SsaoUniforms` で渡す）、差の小さい隣のピクセルから面の法線を求め、黄金角の螺旋上の 24 個のサンプル（ピクセルごとに、interleaved gradient noise で回す）を、`render.ssao_radius` Å（そのピクセルの奥行きでのピクセル数にし、2〜96 px）の範囲で調べる。サンプルが接平面より上（`smoothstep(0.1, 0.5, dot(n, v) / |v|)`）にあり、半径の内側（半径の端で 0 に落ちる）なら遮蔽とみなし、係数は `1 - strength * 3 * 遮蔽の割合` である。PNG の書き出しとアニメーションのフレームも、同じコードで描く（画面外の描画も、同じ `prepare` / `render` を通る）ので、スーパーサンプリングしても、ピクセルでの半径は整合する。SVG の書き出しには、SSAO はない。1 段目は、係数を専用の `R8Unorm` テクスチャ（深度テクスチャと同じ大きさ。ペインの外は 1 でクリア）に描き、2 段目（`fs_blur`）が、そのぼかしたものを色に掛ける。ぼかしは 5 × 5 のバイラテラル（ガウス重み、シグマ 1.5 ピクセル）で、線形の深度の差が `ssao_radius` の約 4 分の 1 より小さい隣だけを混ぜる（輪郭をまたいで遮蔽がにじまない）。背景のピクセルは混ぜない。ぼかしが、ピクセルごとのノイズをならす。限界: とても強いと、薄いむらが残ることがある（ぼかしの半径は 2 ピクセルで固定）。半透明の表面と選択のグローは、深度を書かないので、遮蔽を作らず、受けもしない。遮蔽の鋭さは、深度バッファの精度までである（`far` が非常に大きいと粗くなる）。
- **フェーズB（低優先度）:** パス/レイトレーシングによるソフトシャドウ・AO（Ambient Occlusion=環境光遮蔽）を追加し、ChimeraXのray機能やBlender Cyclesに匹敵する品質を、論文用オフラインエクスポート時のオプションとして提供する。

### 4.8 WebAssembly / ブラウザ描画ターゲット（フェーズ6c-i）
- **バックエンド自動選択:** `wgpu` により、モダンブラウザ（Chrome, Firefox, Safari 等）の WebGPU が最優先で自動選択され、未対応環境では WebGL2 へフォールバックする。
- **Canvas ライフサイクルと HiDPI 整合:** `iced::application` が HTML DOM の `<canvas>` へ自動アタッチする。3D ビューポートパイプラインは `Depth32Float` 深度アタッチメントのサイズをサーフェスの物理ピクセル（`viewport.physical_width/height`）に厳密に一致させ、`pane_grid` のペイン境界に応じた `set_scissor_rect` および `set_viewport` を動的適用することで、ブラウザのリサイズや HiDPI 環境下でのアタッチメント不整合パニックを防止する。
- **シェーダーの可搬性:** WGSL シェーダーは `naga` を介して稼働中のブラウザバックエンド（WebGPU または WebGL2 GLSL ES）へ自動クロスコンパイルされる。球棒インスタンシングおよびリボン/カートゥーン Catmull-Rom メッシュの双方がネイティブ環境と同一のコードパスで動作する。

### 4.9 複数の分子図（共有するパイプラインと、分子図ごとの資源）
`iced` は同じ型の `shader::Pipeline` を 1 つしか作らないので、2つの 3D 分子図は 1 つの `ViewportPipeline`（`frontend/src/viewport/pipeline.rs`）を共有する。そのため、`prepare` が書き込むものは分子図ごとに分けておかないと、描画の前に互いのバッファを上書きし合ってしまう。資源は次のように分ける。
- **共有するもの（1 回だけ作る）:** シェーダーモジュール、すべてのレンダーパイプラインとカリングのコンピュートパイプライン、バインドグループのレイアウト、球・円柱・インポスターのクワッドの共通メッシュ、深度テクスチャ。深度テクスチャは、どのレンダーパスも描く前に 1.0 でクリアし、パスは順番に実行されるので、共有できる。
- **分子図ごとのもの（`ViewportResources`。`ViewportPipeline::viewports` に `ViewportId::{Primary, Secondary}` をキーにして持つ）:** カメラのユニフォームバッファとバインドグループ、背景のユニフォームバッファとバインドグループ、球・スティックのソース／カリング後／間接描画のバッファとカリング用ユニフォーム・バインドグループ、リボン・分子表面（不透明・半透明）・相互作用のバッファ。
`ViewportProgram` と `ViewportPrimitive` が `ViewportId` を持ち、`prepare` と `render` は自分の識別子の資源だけを使う。資源は、識別子が初めて `prepare` されたときに作り、その識別子が `prepare` されなかったフレームの終わりに解放する（`shader::Pipeline::trim`）。そのため、分割をオフにすると2つ目の分子図のバッファは解放される。この管理（`ResourceLedger`）は、単体テストできるよう、GPU のオブジェクトから分けてある。

---

### 4.10 選択の強調（ハロー / グロー）
選ばれている原子（8.9節）は、表示形式に関係なく（カートゥーンだけを表示しているときも）強調する。強調は、分子と同じレンダーパスの、分子表面のあとに、選ばれている原子 1 つにつき 1 つのインスタンス（`ViewportPrimitive::selection_instances`: 原子の位置、ボール・スティックの球と同じ半径（`radius × 0.35`）、UI のパレットの強調色（`UiPalette::selection`、8.8節））で描く。インスタンスバッファは分子図ごとに持つ（`ViewportResources::selection_instance_buffer`）ので、2つの分子図は同じ選択をそれぞれ強調する。シェーダーは `frontend/src/viewport/shaders/selection.wgsl`。パイプラインは球のインポスターのクワッドとカメラのバインドグループを使い回し、シーンの深度で深度テスト（`LessEqual`）して深度は書かないので、手前の原子に隠される。ビルボードは原子の半径の半分だけ奥へずらし、同じ平面に描かれる中距離のインポスター球を上書きしないようにしている。
- **ハロー:** 原子のまわりの輪。ビルボードの半幅は半径の 1.5 倍で、輪は半径の 1.0〜1.3 倍の範囲（縁はなめらか、アルファブレンド）。原子の球の輪郭のように見える。
- **グロー:** 原子のまわりのにじむ光。ビルボードの半幅は半径の 3.0 倍で、強さは中心から縁へ `exp(-3.5 r²)` で弱くなる（縁で 0 になるよう値をずらしてある）。これに 0.85 を掛ける。乗算済みアルファのブレンド（`One`、`OneMinusSrcAlpha`）で描くので、暗い背景では光って見え、明るい背景でも色が付いて見える。隣り合う選択された原子の光は重なって、ひとつの光になる。
- **画面空間のグローとの違い:** マスクを描いてぼかして合成するのではなく、原子ごとのスプライトである。画面外のテクスチャが要らない（大きさの変更への追従も、分子図ごとの管理も、WebGPU 向けの追加も要らない）。一方、光は正確な輪郭をぼかしたものではなく原子ごとの固定の形で、大きさは画面ではなく原子の半径に比例する。マスクとぼかしのグローに置き換える場合も、選択の状態と設定は変えずに済む。
- **切り替え:** View → Selection Highlight: Glow（既定）/ Halo / Off（8.9節）。Off では、インスタンスを送らない。

## 5. マルチスレッド & 非同期ストリーミング設計

【I/Oスレッド (Tokio)】　　　　　【CPU計算スレッド (Rayon)】　　【GPU（VRAM）】

- ディスクからの大容量データロード、およびネットワークを介した計算データの受信は、メイン（UI）スレッドから完全に分離された非同期タスク（Tokio）として実行する。
- PDBファイルのパースや、フレーム補間（アニメーションの中間フレーム座標計算）、チャンク単位ロード・LOD判定は、`Rayon` によるワークスティーリング型スレッドプールでマルチコアに均等分散して並列演算する。
- 連続する軌跡データ（Trajectory）を再生する際は、GPUメモリ上に3スロットのインスタンスバッファ（トリプルバッファリング）を確保し、パース済みの次フレームデータを非表示中のバッファスロットへ非同期アップロードすることで、描画をブロッキングしない滑らかな再生を実現する。
- **軌跡の再生（複数フレームの XYZ。実装済み）。** `core::xyz_trajectory`、`frontend/src/trajectory.rs`、`app/trajectory_ops.rs`。
  - *形式とパーサ。* 複数フレームの XYZ: 1 フレーム = 原子数の行、コメントの行（`step=N` をステップとして読む）、原子ごとの `元素 x y z` の行。全フレームの原子数は同じ。パーサは YUI の独自のもの: `proteindf-bridge`（タグ `2026.10.1`）の `Xyz` は、ファイルの先頭の 1 フレームしか読めず、GRO の複数フレーム、DCD、XTC、TRR、NetCDF の読み込みもない。これらは、bridge 側への依頼事項として ROADMAP に記録し、ここでは扱わない。
  - *読み込み。* ファイルを最初に 1 回走査して、各フレームのバイトの範囲の索引を作る（`index_xyz`。原子の行は解析しない。原子数が変わる、途中で終わっているフレームがあるのは、エラー）。再生中は、必要なフレームだけを読んで解析する（`XyzTrajectory::read_frame`）: ネイティブ版は、ファイルから seek して読み（`from_file`。メモリはフレーム数に比例して増えない）、ブラウザ版は、メモリに持っているバイト列から読む（`from_bytes`）。先読みも、上のトリプルバッファリングも実装していない（フレームは、表示するときに読む）。
  - *開き方。* File →「Open Trajectory (XYZ)...」（構造が必要）、または Project タブで `.xyz` を開く（「Trajectory (XYZ)」が既定、もう一つは「Text Editor」）。backend 上のファイルは、既存の、保護された `/api/fs/read`（パスの検証、ロール）で取得する。backend に新しい API は足していない（`GET /api/trajectory/frame/{index}` は、固定の `demo_trajectory.xyz` を読む、パスを受け取らないデモ）。軌跡の原子数は、表示中の構造と同じで、並びも同じでなければならない（元素は比べない）。違えば、エラーをステータスバーに出す。File →「Close Trajectory」で、軌跡を開く前の構造の座標に戻す。構造を入れ替える（構造やワークスペースを開く、File → New）と、軌跡は閉じる。
  - *再生。* 軌跡は、再生の基盤（8.10節）のフレームの出どころになる: マルチモデルの再生と同じ操作のバー、コマンド、キー。ステータスバーには `step N (i/総数)` を出す。利用者ごとのもので、`AppState.trajectory` は使わず、協調編集では共有せず、ワークスペースにも保存しない（保存されるのは、表示中のフレームの座標）。
  - *フレームを変えたときに更新するもの。* 座標を、構造の木（`AtomGroup`。`atom_group::set_flat_positions`。カートゥーンと相互作用の端点が使う）と、平坦化した `Molecule` のキャッシュ（球、スティック、ピッキング、分子表面）の両方で差し替える。非共有結合相互作用の端点は、毎フレーム、木から解決し直す（相互作用の検出そのものは、構造を読み込んだときの 1 回だけ）。球棒・ピッキング・結合のフォールバックの構造（BVH）は、描くときに `Molecule` から作るので、追従する。原子の ID、結合、二次構造、選択は変わらない。分子表面は、再生中は描かず、再生が止まったとき、表示中のフレームで作り直す（8.10節と同じ）。構造の書き出しは、構造の木をそのまま書く、つまり、表示中のフレームの座標になる。
- WASMクライアント環境では、ブラウザのローカルファイルI/O制限を回避するため、`wasm_bindgen_futures::spawn_local` によるHTTP fetchベースのオンデマンドストリーミングを行う。

---

## 6. スクリプティング & 拡張性

### 6.1 Python連携
学習コストを抑えるため、独自DSLではなく**本物のPython**をサポートする。

- **ネイティブ版:** `PyO3` で Rust の `AppState` 操作APIを Python バインディングとして公開する（`import yui; yui.select("A").color("blue")` 等、PyMOL/ChimeraX類似のAPI）。
- **Web版:** `Pyodide`（Wasm化されたCPython）を使用。ランタイムサイズが大きい（数十MB）ため、初回ロードの非同期化・キャッシュを行う。
- ネイティブ/Web間でAPI互換性を保つため、`core` に薄いPython向けファサードモジュールを用意し、両ランタイムから共通に呼び出せるようにする。

### 6.2 選択(Selection) & 表現(Representation) モデル — 描画DSLは新設しない

学習コストを下げる方針（6.1節）と同じ理由により、描画指定のための独自DSLは新設しない。代わりに以下の2層構造とする。

1. **選択層（`Selector`）:** `proteindf-bridge` の `Select_*` 述語群（`Select_Symbol`, `Select_Name`, `Select_Path_wildcard`, `Select_PathRegex`, `Select_Range`, `Select_Atom`, `Select_AtomGroup`）をRustに移植した上で、既存には無い **`And`/`Or`/`Not` ブール合成コンビネータを新規追加**する。この `Selector` が Python API・YAML設定・`proteindf-bridge` 自身で共有される唯一の選択言語となる。
2. **表現層（`Representation`）:** `{ selection: Selector, kind: BallAndStick|Cartoon|Surface|Licorice|Cpk|Line|Interaction, color: ColorScheme, opacity: f32, visible: bool, interaction_kinds: Vec<InteractionKind>, hides_atoms: bool }` という構造化データとして `AppState` に保持する（YAMLではこのままフィールドとしてシリアライズ、Pythonからはメソッドチェーン `yui.select(...).show(...).color_by(...)` で組み立てる）。両者は同一の内部モデルを指すだけであり、パーサを複数メンテする必要はない。`Representation` はリストとして保持し、重なりは後勝ち（PyMOL方式）で解決する。`hides_atoms: true` のカード（ツリー・ビューポートの「Hide」）は、自身は何も描かず、表示中のあいだ、選んだ原子をすべての原子の表現から外し、CA がそれに含まれる残基のカートゥーンも隠す（リボンはそこで区切るので、隠した部分をまたいでつながらない）。
3. **REPL一行ショートハンド:** `show cartoon, A/*` のようなPyMOL風一行構文は、Python呼び出しへの薄いシンタックスシュガーとして将来追加可能だが、**スコープ外（バックログ）** とし、優先実装はしない。

**原子の表現の描き方（`frontend/src/viewport/scene.rs` の `AtomStyle`）:** `BallAndStick`、`Cpk`（Spacefill）、`Licorice`、`Line` は、どれも球と円柱で原子を描き、原子ごとに次のように描き分ける。Ball & Stick は、ファンデルワールス半径の 0.35 倍の球と、灰色の 0.12 Å のスティック。Spacefill は、ファンデルワールス半径そのままの球で、スティックなし。Licorice は、0.2 Å のスティックで、関節がなめらかになるよう、各原子に同じ半径の球を置く。Line は、0.04 Å のスティックで、球なし。Ball & Stick の結合は灰色の円柱、Licorice と Line の結合は中点で 2 つに分けて原子ごとの色、Spacefill はスティックなし。表示中の表現は、それぞれ自分の選択と配色で独立に描き（`style_layers`。VMD と同じ）、重ねる。Spacefill と Ball & Stick の両方に入る原子は両方で描かれ、Ball & Stick は大きな球の中に隠れる。カードの目のアイコンは、そのカードだけに効く。すべての表現で原子を隠すのは別の種類のカード（`hides_atoms: true`）で、何も描かず、表示中のあいだ、選んだ原子をすべての表現から外す（ツリーとビューポートのメニューの Hide が作る。目のアイコンで、隠すのをやめたり、また隠したりできる）。クリックと選択の強調も同じ半径を使い、球のない Line の原子は 0.15 Å の範囲で拾える。Line は GPU の線のプリミティブではなく細い円柱で描くので、太さは Å 単位で、拡大しても 1 ピクセルのままにはならない。

**パスのワイルドカード/正規表現マッチング（暫定仕様）:** `proteindf-bridge` 本物の `Select_Path_wildcard`/`Select_PathRegex`（10章）が揃うまでの間、ローカルな暫定実装の `Selector` は次のマッチング規則に従うこと。これにより、11.1節のQCLObot連携がこれに依存し始めた後で挙動を作り直す必要がなくなる。パスは `/` 区切りのセグメント列とし、先頭・末尾のスラッシュは無視する（すなわち `/A/6` と `/A/6/` は同じ2セグメントのパス `["A", "6"]` を表す）。パターン中の `*` セグメントは、その位置の任意のセグメント1つに一致する（1階層のみのglob。この暫定実装では再帰的な `**` は扱わない）。パターンは、対象の `AtomGroup` ノードのパスセグメントとパターンのセグメントが位置ごとに一致するときにマッチする。中間ノード（例: 残基）にマッチした場合は、そのノードを根とする部分木に含まれる全原子に解決される。したがって末尾スラッシュの有無に意味の違いはない。これは意図的な簡略化であり、`proteindf-bridge` が揃った時点で本来の挙動と突き合わせて調整する。

**初期実装における `ColorScheme` の範囲:** `ColorScheme` は当面 `Element | Chain | Residue | SecondaryStructure | Uniform` のみとする。スカラー値駆動の配色（原子ごとのMulliken軌道寄与、B-factor、RMSF等）は `ColorScheme` のバリアントには**しない**。これらは12.2節で定義する `ScalarOverlay` 機構により基本色の上に重畳するものであり、12章を実装する段階で初めて追加する。

### 6.3 音声入力
Web Speech APIでキャプチャしたテキストは、独立したパイプラインで処理するのではなく、14章で定義するAIチャットアシスタントへチャットメッセージとして投入する。音声とアシスタントのツール呼び出しループ（上記Python APIへの変換）の関係は14.6節を参照。

### 6.4 Python プラグイン（設計。未実装）
プラグインは、プロジェクトの外の開発者が、Rust のコードを変えたりビルドし直したりせずに、Python で YUI に機能を足すための仕組みである。コンソール（6.1節）と同じ Python API と、同じ `Selector` / `Representation` のモデル（6.2節）を使う。プラグイン専用の言語はない。

- **動かす場所: 別のプロセスではなく、アプリの中。** ネイティブ版の `frontend` に `PyO3` で CPython を組み込む。Cargo の feature `python` で切り替える。ブラウザー版には、当面プラグインはない（6.1節と同じく、後で Pyodide を使うかもしれない）。Python は専用のスレッドで動き、UI スレッドとはチャネルでつながる。プラグインの処理が遅くても、止まっても、例外を出しても、アプリは固まらず、止まらない。プラグインはアプリの状態を直接書き換えない。変更はメッセージとして UI スレッドに送られ、メニューと同じ関数で実行される（8.4節）。状態の読み取りは、UI スレッドが用意する読み取り専用の形（スナップショット、または問い合わせ。どちらにするかは作るときに決める）で行う。Python コンソール（フェーズ6h-i）も、同じ仕組みを使う。
- **公開する API と互換性。** プラグインは公開のモジュール（`yui.plugin`）を使う。`yui` のそれ以外は内部のもので、変わることがある。公開する API には版 `api_version`（整数）があり、今あるプラグインが動かなくなる変更のときだけ上げる。足すだけなら上げない。プラグインは、書いたときの `api_version` を宣言する。アプリが対応していない版のプラグインは読み込まず、理由を表示する。
- **プラグインの形。** 利用者の設定フォルダー（8.6節）の下に、1 つのプラグインにつき 1 つのフォルダーを置く: `<config>/yui/plugins/<name>/`。中に `plugin.yaml`（`name`、`version`、`api_version`、`description`、`author`。`homepage` は任意）と、`register(app)` を定義する Python のパッケージ（`__init__.py`）を置く。`name` はフォルダーの名前で、プラグインの名前空間に使う。
- **有効にするまで動かさない。** プラグインは他人が書いたコードなので、見つけただけでは動かさない。起動時に読み込むのは、利用者が並べたものだけである: `settings.yaml`（8.6節）の `plugins.enabled: [<name>, ...]`、またはアプリのプラグインの一覧（同じキーに書く）。一覧には、プラグインの名前、版、作者、状態（有効、無効、失敗とその理由）を出す。
- **失敗。** `plugin.yaml` の誤り、対応していない `api_version`、`register` やコマンドの中の例外で、アプリは止まらない。そのプラグインを失敗の扱いにし（コマンドとメニューの項目を外す）、プラグインの名前と理由をステータスバーと Log Viewer に出す。
- **インタープリターの起動と終了（実装の注記。2026-10-24）。** 起動は `Py_InitializeFromConfig` で行い、失敗は `Err` として返す。`Py_InitializeEx` は使わない。初期化に失敗すると、プロセスごと終了させるからである（`Fatal Python error`）。起動はプロセスに 1 回だけ行う（2 回目は、動いている Python を壊す）。アプリを終えるとき、ランタイムは Python のスレッドに終了を頼み、待つのは最大 2 秒である。それでも呼び出しが続いていれば、スレッドを切り離してアプリを終える。実行中のプラグインの呼び出しは打ち切られ、Python の後片付けもしない。そのため、終わらないコードのプラグインがあっても、アプリの終了は止まらない。
- **メニューとコマンド（8.7節）。** `app.add_command(id, func)` でコマンドを登録する。`id` は `plugin.<name>.` で始まらなければならず、プラグインは組み込みのコマンドや他のプラグインのコマンドを置き換えられない。`app.add_menu_item(location, order, label, command, args=None)` は、`MenuRegistry::contribute` で、8.7節の場所のどれかに項目を足す（知らない場所は断る）。項目が押されると、`CommandRegistry` が呼び出しを Python のスレッドに送り、そこで `func(ctx, args)` を実行する。`ctx` には、右クリックの対象と、`MenuContext` のうち公開する部分を入れる。項目の有効・チェックの状態を Python で決められるようにするか（メニューを描くときに、UI が Python を待つことになる）は、作るときに決める。まずは決まった項目だけを扱う。
- **信頼と権限。** サンドボックスはない。プラグインは、利用者が自分で実行する Python のスクリプトと同じ権限で動く。このことを、プラグインを有効にする場所と、プラグインの文書に書く。共有のセッションでは、プラグインの変更は、利用者自身の編集と同じ経路と役割の確認（`verify_role_action`、7.1節）を通る。Viewer のプラグインは状態を変えられない。プラグインに `backend` の秘密は渡さない（7.4節）。
- **インストーラーと `libpython`（2026-10-22 決定）。** CPython を組み込むと、バイナリはビルドしたときのマイナー版（`libpython3.X`）に結びつく。当面、インストーラーは `python` の feature **なし**で作り、これまでと同じに動く。プラグインは、`--features python` を付けてソースからビルドしたときに使える（ビルドする計算機にその `libpython` が要る）。プラグインをインストーラーの利用者に届ける段になったら、インストーラーに**専用の CPython を同梱する**（python-build-standalone のような、場所を移せるビルド）。利用者は何も入れなくてよく、プラグインの作者は 1 つの Python の版だけを相手にすればよい。利用者のシステムの Python は使わない。版が合わないとアプリ自体が起動せず、プラグインを使わない利用者まで困るからである。後で同梱するときに作り直さなくてよいように、最初から次のように作る: Python の初期化に失敗したら、プラグインだけを無効にして（理由を表示して）アプリは起動する。外部のパッケージを探す場所は、環境ではなく YUI が決める。
- **まだ決めていないこと。** フォルダーに加えて、`pip` でインストールして entry points で見つける方式も使えるようにするか。組み込みの Python が `numpy` などの外部のパッケージをどこから探すか。プラグインの一覧を置くメニューの場所。

---

## 7. リアルタイム・コラボレーション

### 7.1 権限（Role）モデル
- **Host (Professor):** `AppState` の書き換え権限を100%保持。視点強制同期（Follow Me）トリガー権を持つ。
- **Editor (Student):** アノテーション追加、特定残基の操作権を持つ。
- **Viewer:** 読み取り専用。

**トークンの解決:** `RoleResolver`（`core/src/role.rs`）が、意味のない秘密の文字列を `UserRole` に解決する。実装は `TokenStore`（ランダムな秘密、定数時間の比較、16 文字より短い秘密は登録しない）で、以前の固定の番号 1 / 2 / 3（`SimpleRoleResolver`）はなくなった。秘密は、起動時に `backend` が発行する（7.4節）。collab のパケットのヘッダーの `role_token` は、もう見ない（7.3節）。ロールは、接続を受け付けたときに、接続に結びつく。

### 7.2 トランスポート抽象化
将来のプロトコル移行を見据え、通信層を trait で抽象化する。

```rust
pub trait CollabTransport {
    type Error: std::fmt::Debug;
    fn send(&mut self, packet: CollabPacket) -> Result<(), Self::Error>;
    fn recv(&mut self) -> Result<Option<CollabPacket>, Self::Error>;
}
```

- **クレート配置:** `core` のプラットフォーム非依存性およびWebAssembly（`wasm32-unknown-unknown`）互換性を維持するため、`core` には純粋なデータ構造 `CollabPacket` と `CollabTransport` トレイトのみをネットワーク依存なしで定義する。具象トランスポート実装（`tokio-tungstenite`, `axum` WebSocket等）は `backend` および `frontend` 側に配置する。
- **初期実装:** `WebSocketTransport`。全ブラウザ（Safari含む）で安定動作し、Rustエコシステム（ネイティブでは`tokio-tungstenite`/`axum`、WASMでは`gloo-net`）も成熟している。WASMでは `gloo-net::websocket::futures::WebSocket` がブラウザDOMオブジェクトを内包し `!Send` であるため、`frontend` 側ではソケットを `wasm_bindgen_futures::spawn_local` 内で駆動し、`tokio::sync::mpsc` チャネル経由でメッセージをブリッジすることで、`iced` の `Stream + Send` 購読要件を満たす。
- **将来実装:** `WebTransportTransport`（QUIC/HTTP3）。非信頼データグラムによりHead-of-Line Blockingを回避でき、レーザーポインター等の高頻度・低遅延同期に有利。ブラウザ対応状況（Safari/Firefox）とRustサーバー実装（`wtransport`等）の成熟度を見て移行判断する。
- `CollabPacket`（ヘッダー: SenderID/RoleToken/Timestamp + `AppStateDelta` ペイロード）はトランスポート非依存の共通フォーマットとし、実装差し替え時にアプリ層のコードは変更不要とする。

### 7.3 差分（Delta）メッセージ同期
`AppState` 全体ではなく変更差分のみをMessagePackでシリアライズして送信する。サーバー側は `RoleToken` を検証し、権限のない書き込み要求を破棄する。

---

### 7.4 backend の認証とネットワークへの公開（手順 1 は実装済み。残りは設計）
7.1節の暫定の仕組みは、`backend` を複数の人が使う前、または利用者自身のプログラム以外から届く前に、置き換える必要がある。この節が、その設計である。作業の順番は、`ROADMAP.ja.md` のフェーズ6j。

**今の問題（設計の途中で見つかった。`127.0.0.1` にしかバインドしていなくても、成り立つ）:**
- *利用者が見ているどの Web ページも、そのコンピュータでコマンドを実行できる。* ブラウザは、WebSocket に同一オリジンのルールを適用しない。`backend` は `Origin` ヘッダーを調べない。Host のトークンは固定の `1` で、`/api/terminal` はそれでシェルを起動する。さらに `CorsLayer::permissive()` のため、どのオリジンも REST の応答（`/api/fs/read`、状態）を読める。ページは、`ws://127.0.0.1:8080/api/terminal?token=1` を開くだけでよい。
- 同じコンピュータのほかの利用者（共有のワークステーション、クラスタのログインノード）も、ポートに届き、同じトークンを使える。
- DNS リバインディング: ページが制御する名前を `127.0.0.1` に解決させられる。`backend` は `Host` ヘッダーを見ていない。
- まだ届かないが、公開の前に潰す必要があるもの: ネットワーク上の盗聴と総当たり。

**設計:**
1. *Origin と Host の検査（最初、小さい）。* WebSocket の接続と、単なる資産ではないすべての REST の要求は、`Origin` がない（ネイティブのアプリとコマンドラインのツールは付けない）か、`backend` 自身のオリジンか、許可リスト（`YUI_ALLOWED_ORIGINS`、既定は空）にあるときだけ受け付ける。`Host` ヘッダーは、`127.0.0.1:<ポート>`、`localhost:<ポート>`、または設定した名前でなければならない。`CorsLayer::permissive()` はやめる（Web 版は同じオリジンから配信する。14.3節）。これだけで、トークンがどうであれ、Web ページからの穴は閉じる。
2. *1 / 2 / 3 ではなく、ランダムな秘密（実装済み、フェーズ6j-ii）。* 起動時に、`backend` が OS の暗号論的乱数から 256 ビットの秘密（16 進で 64 文字）を作り、Host のロールにする。メモリにだけ置き、定数時間で比較する（`TokenStore`）。`YUI_TOKEN` で与えることもできる（16 文字以上、空白とコンマなし）。`RoleResolver` は、`u64` ではなく、意味のない文字列を解決する。ロールは、接続を受け付けたときに*接続*に結びつけるので（`auth::guard` が、要求に `Auth` を入れる）、collab のヘッダーのパケットごとの `role_token` は、もう見ない（互換のため、フィールドは形式に残す）。同梱のフォントを除く、すべての `/api/` は、秘密を求める（なければ 401、知らない秘密なら 403）。状態を読むのは、どのロールにも開いているが、書く（`POST /api/state`、`/api/structure`）のと、ターミナルは、Host のロールが要る。
3. *漏らさない渡し方（実装済み、フェーズ6j-ii。最初の設計の、1 回限りのチケットに代わる）。* 秘密は、URL（履歴、ログ、`Referer`）に入れない。REST は `Authorization: Bearer <秘密>`。WebSocket は、デスクトップのアプリでは同じヘッダーで、ブラウザは WebSocket にヘッダーを付けられないので、サブプロトコル `yui.token.<秘密>` で渡す（ブラウザが求めるので、`backend` が、それを選び返す）。チケットの API は、検討してやめた。同じ秘密を、もう 1 つの要求に移すだけで、ヘッダーとサブプロトコルで、すでに URL から外せているため。クライアントが秘密を知る方法: `backend` が、起動時に `<設定フォルダ>/yui/backend.json`（`{url, token, pid}`、ファイルのモードは 0600）を書き、同じコンピュータのデスクトップのアプリが読む（遠くの `backend` には、`--token <秘密>` か `YUI_TOKEN` で与える。こちらが優先）。ブラウザ版は、`backend` が Web 版を配信するときに、`http://127.0.0.1:8080/#token=...` を表示する（URL のフラグメントは、サーバーに送られず、ログにも残らない）。ページが、秘密を `sessionStorage` に移し、アドレスバーから消す（`frontend/src/backend_auth.rs`）。古い `backend.json`（`backend` を再起動した後）は、認証に失敗するだけ。
4. *共有セッションの招待（実装済み、フェーズ6j-iii）。* Host は、`POST /api/invites {"role": "editor"|"viewer", "ttl_secs": 60〜604800（既定 3600）}`（Host のみ。応答に、秘密が入る。表示されるのは、このときだけ。`id` と `expires_at` も付く）で、Editor / Viewer の秘密を作り、`GET /api/invites`（秘密は含まない）で一覧し、`DELETE /api/invites/{id}` で取り消す。同時に有効なのは 50 個まで。メモリにだけ置く（再起動で、すべて終わる）。使い方は Host の秘密と同じ（`Authorization: Bearer` / サブプロトコル）で、ロールは招待のロール。招待で、招待を作ること、状態を書き換えること、`/api/terminal` を開くことは、できず、期限切れ・取り消しの招待は断る。招待で開いた接続（`/api/collab`、`/api/logs`）は、招待が取り消されたらすぐ閉じ（取り消しは、開いている接続に知らせる）、期限切れなら期限の時刻に閉じる（その時刻まで眠る。定期的な確認はしない）。コマンドラインでは、`scripts/yui-invite.py create viewer --ttl 3600 | list | revoke <id>`（`backend.json` を読み、トークンと、`#token=` つきのブラウザのアドレスと、デスクトップのコマンドを表示する。Python のほかに、依存はない）。アプリの中では、**File → Share Session...**（`frontend/src/share_dialog.rs`）で、ロール（Viewer / Editor）と有効な長さ（1 時間 / 1 日 / 7 日）を選んで招待を作り、ブラウザのアドレスとトークン（このときだけ）を「Copy link」/「Copy token」つきで表示し、有効な招待を backend から一覧して取り消せる。Host でない秘密のときは、backend の 403 が表示される。`InviteInfo` には `expires_in_secs`（応答を作った時点の残りの時間）もあり、クライアントは時計を要らない。
5. *バインド（実装済み、フェーズ6j-ii）。* 既定は `127.0.0.1:8080` のまま。`YUI_BIND`（例: `0.0.0.0:8080`）で変えられるが、loopback 以外のアドレスは、TLS を終端するリバースプロキシの後ろにあると `YUI_BEHIND_TLS_PROXY=1` で宣言しない限り、起動時に拒否する（`backend` 自身は、平文の HTTP）。また、loopback 以外では、`YUI_ENABLE_TERMINAL=1` / `YUI_ENABLE_FS=1` を設定しない限り、`/api/terminal` と `/api/fs/*` は 403 を返し、セッションを共有してもシェルが出てしまわないようにする。TLS 自体は、リバースプロキシの仕事。
6. *乱用の制限と記録（実装済み。記録は 6j-i、制限は 6j-iii）。* 認証の失敗（知らない秘密。秘密がない要求は、数えない。秘密を設定していないクライアントは、何度も接続し直すため）は、接続元のアドレスごとに数える（`ConnectInfo`）。60 秒の間に 10 回失敗すると、そのアドレスを 60 秒締め出し、その間は、正しい秘密でも、すべての要求に `Retry-After` つきの `429 Too Many Requests` を返す。締め出しの開始は 1 回だけログに書き、招待の発行と取り消しは、ID でログに書く（秘密は、決して書かない）。リバースプロキシの後ろでは、どのクライアントもプロキシのアドレスなので、制限は共有になる（秘密は 256 ビットなので、これは、推測の対策ではなく、騒ぎの対策）。
- **今回の対象外:** ユーザーアカウント、OIDC / SSO、ロールの中の利用者ごとの権限、共同作業のデータのエンドツーエンドの暗号化。
- **状況:** 手順 1（`backend/src/origin_guard.rs`。`/api/` の前に置いたミドルウェア）と、認証の失敗とターミナルのセッションのログは、実装した（フェーズ6j-i）。`YUI_ALLOWED_ORIGINS` / `YUI_ALLOWED_HOSTS` は、コンマ区切りの一覧。`trunk serve`（開発用）で配信したページは、プロキシ経由で、別のオリジンから `backend` に届く。403 になるときは、`YUI_ALLOWED_ORIGINS=http://127.0.0.1:8085`（`trunk serve` のポート）を設定する。検査は、`Origin` の名前とポートを `Host` ヘッダーと比べ、スキームは見ないので、`Host` を保つ TLS 終端のプロキシは、そのまま使える。
- **作業の順番:** (a) 手順 1 と、6 のログ — 完了（フェーズ6j-i）、(b) 手順 2、3、5 — 完了（フェーズ6j-ii）、(c) 手順 4（招待）と、6 の回数制限 — 完了（フェーズ6j-iii）。実際のブラウザ（Chrome 155、ヘッドレス、DevTools プロトコルで操作）で確かめた: `backend` が配信した Web 版が、`#token=` のフラグメントを受け取り、`sessionStorage` に移し、アドレスバーから消す。REST の取得と collab の WebSocket（サブプロトコル）が受け付けられる。同じオリジンからは、fetch にヘッダーが要り（なければ 401、あれば 200）、ターミナルの WebSocket がサブプロトコルを選び返されて開く。別のオリジン（`http://127.0.0.1:9999`）からは、正しい秘密があっても断られる（fetch は CORS で失敗、WebSocket は 1006 で閉じ、backend は `OriginNotAllowed` を記録する）。ヘッドレスの Chrome には、ソフトウェアの WebGPU アダプタしかなく、アプリは、設計どおり、それを断る（診断のオーバーレイ）ので、ページの描画は、そこではできなかった。まだない: 招待が終わった瞬間（5 秒以内ではなく）に接続を閉じること。

## 8. ユーザーインターフェース (VSCode-like UI)

### 8.1 レイアウト
`iced::widget::pane_grid` により、Left・Center・Right・Bottomの4つの物理ドック領域をユーザーがドラッグでリサイズ・分割できる。`pane_grid`自体が、ペインの中身が何であっても汎用的に動作する分割・リサイズ・ドラッグの仕組みを既に提供している。

各領域にどのコンテンツをドッキングするかは、領域自体から切り離されている（フェーズ6d-iii）: 固定の`PaneKind`を各領域に割り当てるのではなく、各ペインは実行時に決まる`ViewId`のタブリストを持ち、登録済みの任意のビュー（3Dビューポート、構造/ファイルツリー、Representationインスペクタ、Pythonコンソール、ターミナル、ログビューワー、AIチャット、および将来的なスクリプトエディタ等）を、Bottomペインで最初に実証されたタブ切り替えパターンに従って、タブバー経由でどの領域にもドッキングできる。`state.yaml`（8.2節 File Open/Save）は各領域で開いている/アクティブなビューを永続化する。既定のレイアウトは次のとおり: Left=構造ツリー/名前付き選択/プロジェクトツリーのタブ、Center=3Dビューポートのタブと、開いているテキストファイルごとのタブ、Right=Representationインスペクタ、Bottom=ログビューワー/Pythonコンソール/ターミナル/AIチャットのタブ（13・14章参照）。テキストファイルは、Bottomペインではなく、分子図と同じ中央の領域で編集する。

**エディタのグループ（データの持ち方）。** ペインの構成は `PaneLayout`（`frontend/src/pane_view.rs`）で持つ。固定のドック（`PaneKind::Left` / `Right` / `Bottom`）と、エディタのグループ（`PaneKind::Group(GroupId)`）の、タブの並び。それに、中央の領域でグループをどう並べるかを表す木 `GroupNode`（グループ 1 つ、または分割）。`pane_grid` の構成の中央の部分は、この木から作る（`menu_bar::build_pane_configuration`）。ビューをどこに置くかは、1 つの規則 `pane_view::home_group` で決める: 3D ビューポートは最初のグループ、2 つ目のビューポートは 2 つ目のグループ（分割中だけ）、ドキュメントは最後のグループ。保存の YAML は、以前と同じキーと順の `pane_views` のマップ（`left`、`center`、`right`、`bottom`、`center_right`）のままで、読み込むときにグループから木を作り直す。配置の正本は木である。グループは、左右にも上下にも、何度でも分割でき（`GroupNode::split_leaf`）、タブのなくなったグループは、タブのドラッグを終えたときと、ファイルを閉じたときに取り除く（`GroupNode::remove_leaf`、`PaneLayout::prune_empty_groups`。最後の 1 つは残す）。新しいグループには、使っていない次の番号を付ける（YAML では `group_N`。最初の 2 つは、今までどおり `center` と `center_right`）。木は、ワークスペースの YAML に、`pane_views` と並べて `editor_layout` として保存する（`SavedGroupNode`。グループが 2 つ以上のときだけ書く）。利用者がドラッグした大きさも保存する（保存の前に、`Yui::sync_editor_ratios` がグリッドから写す）。

```yaml
editor_layout:
  split: stacked        # side_by_side（左右）か stacked（上下）
  ratio: 0.3            # 最初の側（左・上）の割合
  a: { group: center }  # グループは pane_views のキーで書く
  b: { group: group_2 }
```

読み込むとき（`PaneLayout::set_editor_tree`）、タブがあるのに木にないグループは右端に足し、タブのない木の葉は、ほかの空のグループと一緒に取り除く。グループでないキー、同じグループの重複、0〜1 の外の割合がある木は使わない。`editor_layout` のない YAML（以前の版、またはグループが 1 つ）は、グループを番号の順に左右に並べる。`home_group` は、ビューを*加える*ときの置き場所である（新しいドキュメント、分割、分子図のない配置を読み込んだとき）。利用者がドラッグでほかへ移したタブは、その場所に残す。

**エディタのグループの間で、タブを移す。** タブをドラッグしている間（8.8節のパレットのタブバーを参照）、*ほかのエディタのグループ*のタブの上に入ると、ドラッグ中のタブをその位置へ移し（`PaneLayout::move_view_between`。移した先ではそのタブを選び、元のグループでは選んでいたタブを保つ）、ドラッグは移した先のグループで続く。グループの本体（タブの帯の下）に落としたときは、場所で決まる。中央なら、そのグループの末尾に加える。端（各辺の外側 4 分の 1）なら、そのグループをその向きに分割し、新しいグループへタブを移す（`PaneLayout::split_with_view`。グループの唯一のタブでは、自分のグループを分割しない）。エディタのタブのドラッグ中だけ、各グループの本体を 5 つの透明な `mouse_area`（`Yui::with_tab_drop_layer`）で覆い、入る・出るで `Yui::tab_drop_target`（グループと `DropZone`）を決める。落とすと配置が変わる場所なら、タブが入る範囲（グループ全体、またはその側の半分）をアクセント色で示す（`ui_style::tab_drop_style`）。変えるのは `TabDragEnded` のとき（`Yui::end_tab_drag`）。本体は、いつも同じ 3 層の `stack` にする（ドラッグしていないときは空の `Space`）。こうしないと、ドラッグを始めたときに、分子図やエディタのウィジェットの状態が作り直される。木が変わったら、`pane_grid` を作り直す（`Yui::begin_layout_change` / `end_layout_change`）。このとき、利用者がドラッグした大きさは失わない。今の分割の比率をグリッドから読み（`menu_bar::grid_split_ratios`）、木に書き、新しい構成の分割のうち、同じペインを分けるものに当てはめる（`menu_bar::with_split_ratios`。ドックを含む分割では、エディタのグループをまとめて 1 つとして扱うので、エディタを分割しても、ドックの境界の大きさは保つ。`menu_bar::split_key`）。ドック（左・右・下）とエディタのグループの間でも、どちらの向きにも同じように移せる。どのビューも、どのペインにも置ける（`move_view_between` は、どの 2 つのペインでも受け付ける）。ドックの本体には、落とす場所は中央だけを置く（ドックは分割しない）。ドックのタブをエディタのグループの端に落とすと、そのグループを分割する。どのペインも、タブが 1 つでもタブの帯を出す（どのタブもドラッグできるように）。タブがなくなったドックは残し、案内「Drag a tab here.」を出す（空になったら消すのは、エディタのグループだけ）。設定 `ui.hide_empty_docks: true`（8.6節。起動時に `Yui.hide_empty_docks` に読む）のときは、配置の変更を終えたとき（タブのドラッグ、ファイルを閉じたとき。`Yui::end_layout_change`）に、タブのなくなった表示中のドックを隠し、View メニューの印を外す。View で表示し直すと、案内の出た空のドックが現れる。ほかの境界の大きさは保つ。ドックに置いたドキュメントも、選んでいるドキュメントとして扱い（`active_document` は、グループを調べたあとにドックを調べる）、View →「Split Editor Right」では動かさない（動かすのは、エディタのグループにあるドキュメントだけ）。ビューの状態（ターミナルの接続、ログ、ツリー）はアプリが持つので、移しても保たれる。スクロールの位置など、ウィジェットの手元の状態は初めに戻ることがある。ドックの見出しと View メニューの名前は、今もそのドックの既定の中身を表す。ドラッグ中にタブの帯で移して空になったグループは、ドラッグを終えるまで案内「No file open」（見出し「Editor」）を出し、終えたときに取り除く。分子図のタブを移すと、ウィジェットが作り直されて、その状態は初めに戻るが、視点は保たれる。視点はアプリが持ち（`Yui::camera_of`）、新しいウィジェットはアプリの視点を採り入れる（`synced` が `None` のため）。構造に合わせ直すのは、アプリが求めたとき（`fit_pending`）だけである。分割をオフにすると、2 つ目の分子図を取り除き、ドキュメントを移した後に、右のグループに残ったタブ（たとえば 3D Viewport）を最初のグループの末尾へ移す。

**開いたテキストファイルをタブにする。** 開いているテキストファイル 1 つ分を `Document`（`frontend/src/text_editor_pane.rs`: `DocumentId`、内容、パス、ファイル名、未保存の印）とし、アプリの `DocumentStore` が持つ。ファイルごとに `ViewId::Document(id)` のタブが 1 つ、3D ビューポートと同じタブの列に並ぶ（例: `3D Viewport | a.yaml* | b.txt ×`。VSCode と同じ形で、エディタの中に 2 段目のタブは作らない）。タブにはファイル名を表示し、未保存の間は `*` を付け、`×` ボタンを置く。3D ビューポートのタブには `×` がなく、閉じられない。タブの列は、いつも表示する（「エディタのグループの間で、タブを移す」を参照）。新しいタブは、中央のペイン（分割中は右側のペイン）の末尾に追加して選ぶ。すでに開いているパスのファイルを開いたときは、新しく作らず、そのタブを選ぶ（編集中の内容は読み込み直さない。ブラウザ版のようにパスのないファイルは、同名でも別のファイルとして扱う）。ファイルは、Project タブ（「Text Editor」）、File →「Open Text File...」（ダイアログ）、File →「New Text File」（`Untitled-1`、`Untitled-2`、...。番号は再利用しない）で開く。各ドキュメントのツールバーに Save と Save As があり、Ctrl+S（macOS は Cmd+S）で選んでいるタブを保存する。エディタにフォーカスがあるときは、エディタのキー割り当てとして処理し（macOS の Cmd+S が文字として入力されないようにするため）、フォーカスがないときは、他のウィジェットがすでに処理したイベントを無視するアプリ全体のキーボード購読で受ける。内容を変える操作だけが未保存の印を付ける（カーソルの移動や選択では付けない）。結果とエラー（開いた、保存した、失敗した）は、ステータスバー（8.1節）に表示する。未保存のタブを閉じようとすると確認（Save / Don't Save / Cancel）を出す。Save は保存に成功したときだけ閉じる（保存のダイアログのキャンセルや書き込みエラーでは、タブを閉じない）、Don't Save は変更を捨てて閉じる、Cancel は閉じない。閉じたあとは、隣のタブを選ぶ。終了するとき（File → Quit、またはウィンドウの閉じるボタン。ウィンドウは自動では閉じず（`exit_on_close_request(false)`）、`window::close_requests()` を `Message::QuitRequested` に変えて処理する）に未保存のファイルがあれば、その名前（先頭の 8 件。それ以上は「…and N more」）を並べた確認を出す。*Save All*（1 件なら *Save*）は、開いた順に保存する（パスのないファイルは名前を付けて保存のダイアログが開く）。すべて保存できたときだけ終了する。保存のダイアログをキャンセルしたり、保存に失敗したりしたら、終了を取りやめ、ファイルは開いたまま未保存で残る。*Don't Save* は終了する。*Cancel* は終了しない。未保存のファイルがなければ、すぐに終了する。ブラウザ版には Quit がなく、タブを閉じる操作は横取りしない。開いているファイルは利用者ごとのもので、ワークスペースの YAML には保存せず、協調編集でも共有せず、次の起動で復元もしない。

**テキストエディタを分子図の右側に並べる。** View →「Split Editor Right」は、エディタのグループが 2 つ以上あるときに印が付く（`pane_view::is_editor_split`）。グループが 1 つのときは、そのグループを右に分割する。新しいグループ（番号 1 なら `PaneKind::CENTER_RIGHT`）に、2つ目の 3D ビューポートと、すべてのドキュメントのタブを並べる（選んでいたドキュメントは選んだまま）。印が付いているときは、すべてのグループを最初のグループにまとめる。2つ目のビューポートのタブを取り除き、ほかのグループのタブを末尾に足し、次にドキュメントのタブを足す（`set_editor_split`）。開いているファイルがない分割では、2つ目の 3D ビューポートを表示する（下記）。2つ目のビューポートがあるかどうかは別に調べる（`pane_view::has_second_viewport`。そのカメラと、構造に合わせる処理に使う）。タブを端へドラッグして作った分割には、2つ目のビューポートはない。ペインの表示・非表示、`pane_views`（File → New、ワークスペースを開いたとき）が変わるたびに、そこから `pane_grid` の配置を作り直す（`menu_bar::build_pane_configuration(show_left, show_right, show_bottom, editor_split)`、`Yui::rebuild_pane_grid`）。このときは、ペインの大きさは既定に戻る。エディタのグループが変わったとき（分割のメニュー、タブを落としたとき、ファイルを閉じたとき）は、大きさを保つ（「エディタのグループの間で、タブを移す」を参照）。ドキュメントの内容は、タブの置き場所とは独立した `DocumentStore` にあるので、切り替えで失われない。1つ目の 3D ビューポートは、最初は `Center` にある。右側には、2つ目の 3D ビューポートも先頭のタブとして並ぶ（下記）。

**2つ目の分子図と視点の連動。** 分割しているとき、右側の先頭のタブは `ViewId::Viewport2`（「3D Viewport 2」）で、その後ろにドキュメントのタブが並ぶ。`set_editor_split(true)` が加え（選んでいたドキュメントは選んだまま。ドキュメントがなければ2つ目の分子図が表示される）、オフにすると消える。配置を読み込むときは、`normalize_pane_views` が、分割していなければ取り除き、どちらの分子図も重複していれば後ろのものを取り除き（利用者がほかのグループへ移した分子図は、その場所に残す）、どこにもないときは、空の `center_right` のグループにだけ足す（2つ目の分子図より前の版で分割して保存したもの）。そのあと、タブのなくなったグループを取り除く。1つ目と同じく閉じられない。2つの分子図は、同じ構造・表現・表示中のモデルを描き、違うのはカメラだけである（表示内容は描画のたびに分子図ごとに作り直す。分子図が2つになると、このコストも2倍になる）。
- **カメラの置き場所。** 1つ目のカメラは `AppState.camera`、2つ目は `Yui.secondary_camera`。後者はこの画面だけのもので、保存せず、協調編集にも送らない。2つ目を最初に表示するとき、1つ目のカメラをコピーして始める（それまでは `None`。分割していないときに構造を開く・File → New・Reset Camera を行うと `None` に戻し、次に表示するときにまたコピーする）。分子図のウィジェット（`viewport::ViewportProgram`）はカメラをアプリから受け取って描き、変更（ドラッグ、ホイール、構造に合わせた結果）を `Message::ViewportCameraChanged { id, camera, fitted }` で知らせる。アプリが表示を作り直すまでに、複数のマウスイベントが届くことがあるので、ウィジェットは状態に手元のコピーと、アプリと最後にやりとりしたカメラ（`ViewportState::synced`）を持つ。アプリ側のカメラが `synced` と違えば、外から変わった（協調編集の差分、backend の初期状態、読み込んだワークスペース、下記の連動）ので採り入れ、同じならば手元のコピーを使う（`viewport::effective_camera`）。
- **合わせる要求。** `Yui::request_camera_fit` が `camera_fit_token` を進め、合わせが必要な分子図を `Yui.fit_pending` に記録する（2つ目は分割しているときだけ）。合わせが待たれている分子図は、自分の縦横比を知っているので、自分で合わせたカメラを計算してすぐそれで描き、トークンごとに 1 回だけ `fitted: true` で知らせる。アプリは受け取ると、その分子図の待ちを終える。つまり合わせは分子図ごとに行い、連動は合わせを伝えない。待ちの記録はアプリにあるので、いま表示されていない（タブを選んでいない）分子図は、次に表示したときに合わせ、合わせ済みの分子図は作り直されても視点を保つ。
- **連動。** View →「Link Viewport Cameras」（既定はオフ）。オンにすると1つ目のカメラを2つ目へコピーし、オンの間は、どちらの分子図でドラッグ・スクロールしても、もう一方へカメラを写す。
- **読み込み。** ワークスペースを開いたとき、または backend の初期状態を受け取ったときは `Yui::apply_loaded_camera` を行う。既定のカメラなら構造に合わせ、利用者が動かして保存されたカメラならそのまま使う。
- **協調編集へは送らない。** ドラッグによるカメラの変更は差分として送らない（接続があふれるため）。`camera` を設定する差分が届いたときは、1つ目の分子図に反映する。

**保存と互換性。** 分割は `pane_views` の一部なので、ワークスペースの YAML に保存され（`center_right` のキー。開いているファイルがなければ空のリスト）、開くと復元される。`center_right` のない YAML は分割なしで開く。ドキュメントのタブは、保存の前（`pane_view::pane_views_for_saving`）と読み込みの後（`pane_view::normalize_pane_views`）に取り除く。読み込みの後の処理は、`2026.10.2` 以前に保存したワークスペースにある、1 つだけの `text_editor` のタブも取り除く（`ViewId::LegacyTextEditor` は、そのようなファイルを読めるようにするためだけに残している。当時の版で分割していたワークスペースは、2つ目の分子図のある分割として開く）。配置を差し替えたとき（File → New、ワークスペースを開いたとき）は、`Yui::set_pane_views` が、開いているファイルのタブを、新しい配置が分割なら右側に、並べ直し、選んでいたタブを選び直す。ワークスペースを開いても、利用者のファイルは閉じない。

**エディタのフォント。** エディタとターミナルは、同じ等幅フォントの指定（`frontend/src/fonts.rs`: ファミリー名、太字・標準、大きさ 14 px）を使う。後で利用者ごとの設定ファイルから変えられるようにするためである。JetBrains Mono（SIL OFL 1.1。Regular 270 KB、Bold 274 KB。`frontend/assets/JetBrainsMono-*.ttf` と `JetBrainsMono-OFL.txt`）を同梱し、起動時に登録する。OS のフォントが使えないブラウザ版では、同梱が必須である。日本語の字形は持っていないため、日本語は同梱の Noto Sans JP（等幅ではない）で表示される（ターミナルでは日本語の桁は揃わない）。

あるペインのタブを別のペインへドラッグして移動する操作、およびドラッグによる実行時の新規分割作成は、後続フェーズまで明示的に対象外とする — これはこの基盤の上への純粋な追加機能であり、この基盤の上に構築されたビューを作り直す必要はない。

**ステータスバー。** ウィンドウの下端、`pane_grid` の下に、全幅で1行のバーを置く（`frontend/src/status_bar.rs`、高さ `STATUS_BAR_HEIGHT` = 24 px）。左側には、利用者の操作の結果の最新のメッセージを表示する。メッセージは `StatusMessage { text, level }` で表し、種類（`Info` / `Warning` / `Error`）はメッセージを書く箇所で明示的に指定する（文字列から推測しない）。警告は黄色、エラーは赤で表示する。メッセージは自動では消さず、次のメッセージで置き換わるまで残す。右側には、左から順に次を表示する: 分子表面のメッシュの生成中は `Generating surface...`（最新の世代の結果を反映したとき、または分子表面がなくなったときに消える）、読み込んでいる構造の原子数と結合数（構造がないときは表示しない）、表示中のモデルとその位置（`model_2 (2/20)`、全モデル表示では `All models (20)`。マルチモデルの構造のときだけ表示する）、協調編集の接続状態（`Local` / `Connecting...` / `Online` / `Offline: <エラー>`。ウィンドウのタイトルと同じ規則で決める）。表示する内容は純粋関数 `build_status_bar_fields` で求める。バーの色は、UI のパレット（テーマ）を導入するまでは同じモジュールの定数とする。テキストエディタのペインは、ツールバーに自身のステータス表示を持つ。

### 8.2 メニューバー（フェーズ6d-i）
**File**・**View**・**Help** をカバーするメニューバーを設ける。
- **File:** New（既定の空`AppState`にリセット）、Open（YAML状態ファイルのインポート、2.1節）、Save / Save As（YAMLへのエクスポート）、New Text File / Open Text File...（テキストファイルをタブで開く、8.1節）、Export Structure...（読み込み済み構造をPDBまたはmmCIF形式で書き出し。非同期保存ダイアログで拡張子に応じて形式を自動判定、構造未ロード時は項目を無効化、非同期`Task`で実行されUIをブロックしない、WASM環境ではブラウザのダウンロード保存で動作）、Quit。**ファイルI/Oに関する注記:** Open / Save / Save As / Export Structure は、ネイティブとWASMの両環境で非同期ファイル選択ダイアログ（`frontend/src/workspace_file_io.rs` 内の `rfd::AsyncFileDialog`）を使用する。ネイティブ版では Open および Save As で選択したファイルパスを記憶し、Save は直前のパスへダイアログなしで直接上書き保存する（パスが未記憶の場合は Save As にフォールバック）。WASM版ではブラウザのセキュリティ制約によりローカルパスを直接扱えないため、Save は Save As（ブラウザの保存・ダウンロードダイアログ）として動作し、Open/Save/ExportのいずれもUIをブロックすることなく非同期に実行される。
- **View:** Reset Camera（表示中のすべての分子図のカメラを、表示中の構造に合わせる。何も読み込まれていなければ既定のカメラに戻す。4.3節）、Left/Right/Bottomペインの表示/非表示切替、Split Editor Right（8.1節。2つ目の3Dビューポートも出る）、Link Viewport Cameras（8.1節）。
- **Help:** About（`Cargo.toml`のCalVerバージョンとGPLv3ライセンスを表示）。
- **実装方針:** `frontend`はネイティブ版とWASM/ブラウザ版の両方を対象とし（6章c）、ブラウザにはOSレベルのメニューバーが存在しないため、メニューバーはネイティブOSメニューではなくウィンドウ内`iced`ウィジェットとして描画し、両ターゲットで同一実装が動作するようにする。バージョン不一致等のリスクを持つ外部依存クレートを避け、`frontend/src/menu_bar.rs` において `iced` 標準のプリミティブ（`row`, `column`, `container`, `button`, `mouse_area`, `stack`）により自作した。ドロップダウンはメインビュー上のスタックレイヤーとして描画され、メニュー外クリックを拾う透明な `mouse_area` バックドロップで自動的に閉じられる。ペインの動的な表示/非表示は `build_pane_configuration(show_left, show_right, show_bottom)` で管理され、カメラリセットは単調増加トークン `camera_reset_token` の更新によってトリガーされる。
- **スタイル（右クリックのメニュー（8.3節）と共通）:** すべてのメニューの見た目は `frontend/src/ui_style.rs` から取る。色はメニューのコードに直接書かず、色の役割ごとの値をまとめた `UiPalette`（カードの背景・枠線・影、文字、控えめな文字、押せない文字、項目のホバー・押下の背景、強調色、区切り線、メニューバーの背景、注意の色、モーダルの暗幕）から取る。暗いテーマの値（`UiPalette::dark()`）だけがあり、テーマの切り替えは後続のタスクで行う。共通のスタイル関数は、`menu_card_style`（カードの背景・枠線・影）、`item_button_style`（通常は透明、ホバーで少し明るい背景、押せないときは暗い文字。塗りの強調色は使わない）、`danger_button_style`、`accent_button_style`（OK）、`menu_bar_button_style`、`menu_bar_style`、`menu_separator`（文字の破線ではなく高さ 1 px の線）。メニューバーの左端のアプリ名は表示しない（アプリ名はウィンドウのタイトルと About ダイアログに残す）。開いているメニューは、括弧ではなくボタンの背景の色で示す。3 つのボタンは幅を固定（`MENU_BUTTON_WIDTH`）しているので、ドロップダウンの左端はボタンの順番から計算でき（`dropdown_left`）、それぞれのボタンの真下に出る。About ダイアログは、背景を暗く（暗幕）して、カードのスタイルで表示する。
- **保留（明示的なギャップ）:** Editメニュー（Undo/Redo）は初期のメニューバーには含めない — `AppState`の変更履歴の設計がまだ存在しないため。動かないUndo/Redoメニュー項目をプレースホルダーとして追加しないこと。この履歴機構が設計された時点でEditメニューを追加する。

### 8.3 コンテキストメニュー / 右クリック（フェーズ6d-ii〜6d-iv）
右クリックのコンテキストメニューは、対象ペインが既に実際の操作可能なウィジェットを持っているか（単なるプレースホルダーのテキストではないか）に応じて段階的に追加する:
- **3Dビューポート / Centerペイン（フェーズ6d-ii）:** 右クリック時に、現在ロードされている構造に対してレイキャストによるピッキングを行う。スクリーン座標から `screen_to_world_ray`（完全な逆行列計算を行わずLookAt基底ベクトルとカメラ画角から直接算出）によりワールドレイを構築し、`AtomBvh::query_ray` によるSlab法AABB枝刈り（原子半径マージン付き）と原子球との精密交差判定によってカーソル直下の最近接原子を高速に特定する。ヒット時は最上位スタックレイヤー上のウィンドウ内コンテキストメニューカード（`frontend/src/context_menu.rs`）を表示し、全画面透明 `mouse_area` バックドロップにより外側クリックで閉じる。8.4節の原則に準拠し、メニュー操作はすべて `Selector` および `Representation` の操作（`visible: false` による非表示、`RepresentationKind` による表示形式変更、`ColorScheme` による配色変更）に厳密にマッピングされ、専用のコマンド言語は新設しない。Select Atom / Select Residue は Representation を足さず、利用者ごとの選択の状態を変える（8.9節）。
- **Leftペイン / 構造ツリー（フェーズ6d-iii）** および **Rightペイン / Representation一覧（フェーズ6d-iv）:** フェーズ6c時点では、両ペインともプレースホルダーのテキストであり操作可能なウィジェットではない — コンテキストメニューが作用する対象がまだ実質的に存在しない。各フェーズはまず該当ペインを実際のツリー/リストウィジェットとして構築してから、そのコンテキストメニューを追加すること。詳細は`ROADMAP.md`を参照。

**メニューをウィンドウに収める。** ウィンドウの大きさは `Yui.window_size` に持つ。起動時の大きさは `window::latest()` + `window::size` で取り、以後の変化は subscription で `window::Event::Resized`（と `Opened`）を受け取る（`Message::WindowResized`）。すべてのメニュー（ドロップダウンと右クリックのメニュー 3 つ）は 1 つの関数 `menu::popup_menu` で作り、置き場所は `frontend/src/menu/placement.rs` の純粋関数で決める。
- 置く領域は、ウィンドウからメニューバーとステータスバーを除いたもの（`layer_area`）。
- 右クリックのメニューは、カーソルを左上にして開く。右に収まらなければカーソルの左側に、下に収まらなければカーソルの上側に開く。どちらにも余地がなければ、収まる位置までずらす（`place_at_cursor`。余白は 8 px）。
- ドロップダウンは、ボタンの真下に開く。高さだけを制限する（`place_dropdown`）。
- 内容が領域より高いときは、メニューの高さを領域に収まる値に制限し、内容を `scrollable` にする。スクロールにするのはそのときだけなので、収まるメニューは従来と同じ見た目になる。
- iced では、描画前にウィジェットの実際の高さを得られない。そこで、高さはメニューモデルから見積もる。文字の高さ = 文字の大きさ × 1.3（既定の行の高さ）、ボタンはこれに縦の余白を足し、さらに場所ごとの項目の間隔と余白を足す。右クリックのカードは、横に収まるかも計算できるよう、幅を固定（240 / 270 px）にした。見積もりが少しずれても、余白とスクロールで吸収する。カーソルの位置は、従来どおり、メニューを描く層の座標としてそのまま使う。

### 8.4 アクションパイプライン（メニュー/コンテキストメニュー/マウス/Python APIの統一）
メニューバーの操作、コンテキストメニューの選択、直接のマウス操作、およびPythonコンソール（6.1節）は、いずれも同一の`Selector`/`Representation`/`AppState`操作（6.2節）に帰着する — これらのいずれも独自のコマンド言語を新設しない（6.2節の新規DSL禁止ルールと整合）。メニュー項目やコンテキストメニューのアクションは、スクリプティング層が使うのと同じPython API相当のRust関数呼び出しとして実装するため、どの入力手段で発生した操作であっても、その操作履歴を常にスクリプトとして再現・保存・エクスポートできる。

### 8.5 CJK/IME対応
`iced` の `cosmic-text` ベースのテキストシェイピングにより、日本語入力・表示の安定化を図る。

### 8.6 利用者ごとの設定ファイル
プロジェクトではなく、アプリを使う人に属する設定は、ワークスペースのドキュメント（2.4節）とは別の、小さな利用者ごとの YAML ファイルに置く。ワークスペースには保存せず、協調編集でも共有せず、`AppState` にも含めない。実装は `frontend/src/user_settings.rs`。

- **場所:** ネイティブ版は、OS の利用者ごとの設定フォルダの `yui/settings.yaml`（`dirs::config_dir()`。Linux は `~/.config/yui/settings.yaml`、macOS は `~/Library/Application Support/yui/settings.yaml`、Windows は `%APPDATA%\yui\settings.yaml`）。OS の設定フォルダが分からないときは、ファイルを使わず、既定値で動く。ブラウザ版は `localStorage` のキー `yui.settings`（同じ YAML の文字列）。File →「Open Settings File」（ネイティブ版のみ）は、設定ファイルをテキストのタブで開く。ファイルがなければ、先にフォルダごと、すべてコメントの雛形（`DEFAULT_SETTINGS_TEMPLATE`）で作る。すでにあるファイルは上書きしない。
- **読むとき:** 起動時に 1 回だけ（`main`、ウィンドウを作る前）。変えたら YUI を再起動する必要があり、起動中の読み直しはない。アプリが書き戻す設定は、View メニューで選んだもの（テーマ（8.8節）と選択の強調（8.9節））だけ。`theme:` の行だけを書き換え、コメントと他のキーは残す。
- **項目（今のところ）:**
  ```yaml
  theme: dark                       # "dark"（既定）または "light"（8.8節）
  selection_highlight: glow         # "glow"（既定）、"halo"、"off"（8.9節）
  editor:
    font_family: "JetBrains Mono"   # 既定: 同梱の等幅フォント
    font_size: 14                   # ピクセル、6〜72。既定は 14
    scrollbar: true                 # テキストエディタのスクロールバー。true（既定）か false
  terminal:
    font_size: 12                   # ピクセル、6〜72。既定 12（フォントの種類は editor.font_family）
  ui:                               # メニューとタブの見た目（8.8節）
    tab_style: underline            # "underline"（既定。選択中のタブの下にアクセント色の線）か "filled"
    tab_font_size: 12               # ピクセル、8〜24。既定 12
    corner_radius: 8                # メニューとダイアログの角の丸み（ピクセル）、0〜16。既定 8（項目は、その半分）
    hide_empty_docks: false         # true: タブがなくなった左・右・下のパネルを隠す（8.1節）。既定 false
    colors: {accent: "#5b8def"}     # 色の上書き。"#rrggbb" か "#rrggbbaa"。両方のテーマ
    dark_colors: {}                 # 同じ。暗いテーマだけ
    light_colors: {}                # 同じ。明るいテーマだけ
  render:                           # 描画の大きさ（4.3節）。半径の単位は Å
    ball_scale: 0.35                # ボール・スティックの球 ＝ ファンデルワールス半径の何倍か、0.1〜1.0
    stick_radius: 0.12              # ボール・スティックのスティック、0.02〜0.5
    licorice_radius: 0.2            # ライコリスのスティック、0.05〜0.6
    line_radius: 0.04               # 線表示、0.01〜0.2
    interaction_radius: 0.08        # 相互作用の線（3D と SVG）、0.02〜0.3
    interaction_dash: 0.35          # 相互作用の線の、描く長さ（Å）、0.05〜2.0
    interaction_gap: 0.25           # 破線の空白の長さ（Å。0 で実線）、0〜2.0
    bond_color: [0.7, 0.7, 0.7]     # ボール・スティックのスティックの色、RGB、各 0〜1
    ribbon_helix_width: 2.6         # カートゥーンの、ヘリックスのリボンの幅（Å）、0.4〜6.0
    ribbon_helix_thickness: 0.7     # ヘリックスのリボンの厚さ（Å）、0.1〜2.0
    ribbon_sheet_width: 2.4         # シートの矢印の幅（Å）、0.4〜6.0
    ribbon_sheet_thickness: 0.3     # シートの厚さ（Å）、0.1〜2.0
    ribbon_loop_radius: 0.25        # ループの管の半径（Å）、0.05〜1.0
    selection_size: 1.0             # 選択のグロー・ハローの大きさの倍率、0.5〜3.0
    ssao_strength: 0.0              # 環境遮蔽（SSAO）。0 でオフ、0〜1（4.7節）
    ssao_radius: 3.0                # SSAO が遮蔽を探す半径（Å）、0.3〜10
    surface_grid_spacing: 0.5       # 分子表面の格子間隔（Å）、0.25〜1.5
    surface_probe_radius: 0.0       # 分子表面のプローブ半径（Å）、0〜3.0
  ```
  `render:` の値は `RenderSettings`（`frontend/src/user_settings.rs`）で、`viewport/scene.rs`（`AtomStyle`）、相互作用の線、SVG の書き出しが `user_settings::startup_render()` から読む。`scene.rs` の定数は既定値である。ほかの項目と同じく 1 つずつ検査する（型が違う、範囲外の値は、その項目だけ既定値にして警告）。破線の長さは、カメラのユニフォームバッファ（`CameraUniforms::style`。選択の強調のシェーダーへの `selection_size` も、ここを通る）を通して相互作用のシェーダーに渡す（SVG の書き出しは、線の太さに従う自前の破線のままである）。分子表面の項目は、アプリが作るすべての `SurfaceParams` の `grid_spacing` と `probe_radius` になる（間隔を細かくすると、その 3 乗に比例して時間とメモリが増える）。
  `editor.font_family` はテキストエディタとターミナルに効く（太字は同じファミリーの太字を使う）。`editor.font_size` はテキストエディタだけに効く。ターミナルの大きさは `terminal.font_size`（既定 12 px、`fonts::TERMINAL_FONT_SIZE`。ほかと同じく、起動時に読む）で、行・桁の数、範囲選択、ホイールのスクロールは、それから計算する（文字の幅は大きさの 0.6 倍、行の高さは 1.3 倍 + 1）。`font_family` には、同梱のファミリー名（`JetBrains Mono`、`Noto Sans JP`。大文字・小文字は区別しない）か、ネイティブ版ではそのコンピュータに入っているフォントのファミリー名を書ける。入っているかは、システムのフォントの一覧（`fontdb`）で調べる（ファイル名ではなく、ファミリー名で照合する）。ブラウザ版にあるのは同梱のフォントだけである。フォントの*ファイルのパス*を指定する項目はない。`iced` のフォント名は `&'static str` なので、選んだファミリー名は起動時に 1 回だけ確保する（`fonts::init`）。`editor.scrollbar`（起動時に 1 回だけ読む）は、テキストエディタの横に、つまみを掴んで動かせるスクロールバーを出す。`iced` の `text_editor` にはスクロールバーがなく、スクロール位置も外から読めないので、エディタの高さを内容に合わせ、`scrollable` で包んでいる（`text_editor_pane::view_document`）。その代わり、エディタが自分でカーソルへスクロールしなくなるので、カーソルの行を、文字の幅から（折り返しも含めて）見積もり、測った内容の高さに合わせて補正して、キーボードの移動と編集のあとに `scroll_to` を出す（マウスのクリックのあとは出さない）。`scrollbar: false` なら、内部でスクロールする従来のエディタを使う。
- **検証（アプリを止めない）:** ファイルがなければ、黙って既定値。YAML として読めない、または一番外が対応表でないテキストは、すべて既定値にして、ファイルの名前を書いた警告を 1 つ出す。それ以外は項目ごとに調べ、型が違う、`font_size` が 6〜72 の外（NaN・無限大を含む）、入っていないフォントのファミリー、のいずれかなら、**その項目だけ**既定値に戻して警告を出す（入っていないフォントは、任意のシステムフォントではなく、JetBrains Mono に戻す）。ほかの項目は反映する。知らないキーは警告なしで無視するので、新しい版の設定キーがあっても古い版で読める。起動時の警告は、まとめて 1 つのステータスバーの警告として表示する（8.1節）。

### 8.7 メニューモデルとコマンド
メニューは、コードにボタンを直接並べるのではなく、実行時にデータから組み立てる。将来プラグインが項目を足せるようにするためである（プラグインの仕組み自体 — 言語、読み込み、権限 — は 6.4節で設計する）。コードは `frontend/src/menu/` にある。`core` ではなく `frontend` に置くのは、モデルがボタンの大きさなど UI 寄りの情報と、アプリの `Message` 型を持つからである。
- **コマンド。** メニューの項目は、処理そのものではなく、`CommandInvocation { id, args }`（コマンドの識別子と、シリアライズできる引数）を持つ。識別子は `file.new`、`file.export_structure`、`view.toggle_pane`、`viewport.show_as` など。引数は JSON の値で、ペイン、表示形式、配色、原子 ID、行の番号などである。`CommandRegistry` が、識別子と引数から、既存の操作 — `MenuAction`、`ContextMenuAction`、`TreeContextMenuAction`、`RepContextMenuAction` のいずれかを持つアプリの `Message` — を引き当て、既存の `handle_*` がそのまま処理する。項目を押すと `Message::RunCommand` が送られ、引き当てたメッセージを処理する。未登録の識別子や正しくない引数でも、パニックしない。何も実行せず、警告を出力してステータスバーにも表示する。組み込みのコマンドと、その引数・橋渡し先は、`menu::register_menu_commands` のドキュメントコメントに一覧がある。右クリックのメニューの対象（原子、ツリーのノード、Representation の行）は引数に入る（`atom_id`、`chain_id`/`residue_id`、`index`）。ただし構造ツリーのメニューの対象は、メニューを開いたときにアプリが覚えているノード（`Yui.tree_context_menu`）なので、そのコマンドはノードの引数を取らない。
- **メニューモデル。** `MenuItem` は、コマンド（`CommandItem`: 表示名、コマンド、押せるか、チェック状態、見た目）、区切り線、見出し、カードのヘッダー（タイトルと対象の説明）、グループ（横または縦に並べる）、子メニュー（今は見出しと、その下に項目を並べる形で描く。見た目は menu-styling のタスクで決める）のいずれかである。項目が押せるか・チェックされているかは、項目を作るコードが、アプリの状態（`MenuContext`: ペインの表示、分割、視点の連動、構造の有無、右クリックの対象）から求める。条件を書く言語はない。チェックのある項目は、表示名の前に `✓ `（チェックなしは空白 2 つ）を付けて描く。項目ごとの `ItemStyle`（文字の大きさ、余白、横幅いっぱい、注意の色）で、既存の見た目を再現する。`menu::render_items` が、ドロップダウン（`Surface::Dropdown`）と右クリックのカード（`Surface::Card`）を描く。
- **場所と受け口。** メニューは場所で識別する: `menu/file`、`menu/view`、`menu/help`、`context/viewport`、`context/structure_tree`、`context/representation`、`context/file`（プロジェクトツリーのファイルを右クリックしたときの「Open with」。マウスのそばに開き、そのファイルの開き方の候補を並べる。コマンドは `file.open_with`、引数は `path` と `handler`）。`MenuRegistry::contribute(location, order, producer)` が項目を足す（決まった項目は `contribute_item`）。組み込みのメニューも同じ呼び出し（`menu::register_builtin_menus`）で登録するので、プラグインは全く同じ受け口を使う。場所の中の項目は、`order` の小さい順、同じなら足した順に並ぶ。組み込みは 100 の倍数で足す（File: 100 がファイル操作、200 がテキストファイル、300 が Quit。View: 100 が Reset Camera、200 が背景色、300 がペインとトグル）ので、プラグインはその間（例: 150）にも、末尾（例: 1000）にも足せる。
- **8.4節・6.2節との関係。** コマンドの識別子は、既存の操作に付けた名前であり、新しい言語ではない。文字列を解析・評価する仕組みはなく、コマンドはこれまでと同じ `Selector` / `Representation` / `AppState` の操作に帰着する。引数は単純なデータなので、将来 Python コンソールやプラグインから、同じコマンドを呼べる。

---

### 8.8 UI のテーマ（暗い / 明るい）
UI には、**暗い**（既定。テーマができる前の見た目）と**明るい**の2つのテーマがある。テーマは利用者ごとの設定（8.6節）で、ワークスペース・`AppState`・協調編集には入れない。
- **パレット（`frontend/src/ui_style.rs`）:** UI の色はウィジェットのコードに直接書かず、色の*役割*ごとの値をまとめた `UiPalette` から取る（カードの背景・枠線・影、文字の5段階 `text` > `text_value` > `text_secondary` > `text_muted` / `text_subtle` > `text_faint`、押せない文字、項目のホバー・押下の背景、強調色、区切り線、メニューバーとステータスバーの背景、選択・チップの背景、注意・警告・エラー・接続中の色、パネル、案内の帯、暗幕、ターミナルの背景・文字・カーソル）。値は `UiPalette::dark()` と `UiPalette::light()` が返し、`UiPalette::for_theme(ThemeKind)` で選ぶ。スタイル関数は `ui_style::palette()` を呼ぶ。これは今のテーマ（`Yui::view` の先頭で更新する atomic の値）を参照するので、すべての描画関数にパレットを渡して回らなくてよい。以前に直書きしていた似た灰色は5段階の文字の色にまとめたので、暗いテーマでも、灰色の文字が以前から数%ずれる箇所がある。 **フラットな見た目とカスタマイズ:** タブバー（`pane_view::view_tab_bar`）は、フラットである。選択していないタブは控えめな文字、選択中は濃い文字と、その下の 2 ピクセルのアクセント色の線（`tab_style: underline`。`filled` は、線ではなく選択中のタブの背景を塗る）、マウスを重ねると淡い背景、タブバーとメニューバーの下には細い区切り線。ペインのタイトルバーは、メニューバーと同じ帯の色の上の、小さく控えめな文字で、各ペインには、区切り線の色の細い枠がある（`pane_title_style`、`pane_body_style`）。標準のウィジェットの iced のテーマ（`ThemeKind::iced_theme`）は、組み込みの暗い / 明るいテーマの `primary` の色だけをパレットの `accent` に置き換えたもので、標準のボタン、テキスト入力のフォーカス、ペインの大きさ変更の強調も、`ui.colors.accent` に従う。線は、アクセント色を背景にした外側の枠の、下の余白で、その内側を帯の色で塗りつぶす（幅を `Fill` にした線を足すと、タブが折り返すとき、どのタブも 1 行を占めてしまう）。入りきらないタブは、ドック（左・右・下。細いことが多い）では次の行へ折り返し、エディタのグループでは VSCode と同じく横にスクロールする（トラックパッド、Shift + ホイール、または普通のホイール。普通のホイールは `TabBarWheel` で横の `scroll_by` に変える）。タブは、そのペインの中でドラッグして並べ替える。タブを押すと（離したときしか知らせないボタンの内側の `mouse_area` で受ける）、選択してドラッグを始め（`Yui::tab_drag`）、同じペインのほかのタブの上に入ると、そのタブの位置へ移す（`PaneViewState::move_view_to`。選択中のビューは変えない）。どこでボタンを離しても終わる（ドラッグ中だけある購読）。クリックでは、ほかのタブに入らないので、並べ替わらない。ドラッグ中は、押した位置（`Yui::tab_drag_start`）から 4 ピクセル動かすと、VSCode と同じく、タブの名前を入れた小さな四角形がマウスと一緒に動く（`Yui::view_tab_drag_ghost`。ウィンドウ全体の上の層に `iced::widget::pin` で置く。不透明なカードの色に、アクセント色の枠: `ui_style::tab_drag_ghost_style`）。四角形はマウスの右 12 ピクセル、下 10 ピクセルに置き、マウスの真下には置かない。そうすれば、下のタブの帯や落とす先の判定に、マウスが届く。パレットには、タブの役割 `tab_bar_bg`、`tab_indicator`、`tab_active_text`、`tab_inactive_text`、`tab_active_bg`、`tab_hover_bg` がある。設定ファイルの `ui:`（8.6節）で、`tab_style`、`tab_font_size`、`corner_radius`（`UiSettings`。起動時に `ui_style::set_ui_settings` で 1 回だけ設定）を変え、`ui_style::COLOR_NAMES` にある名前の色を上書きできる（`UiPalette::set_color`。`ui_style::palette()` が、テーマの上に、`colors`、次に `dark_colors` / `light_colors` の順で重ねる）。正しくない値は既定値のままにして、ステータスバーに警告を出す。
- **iced 自身のテーマ:** `Yui::theme` が `Theme::Dark` / `Theme::Light` を返す（`ThemeKind::iced_theme`）ので、標準のウィジェット（テキスト入力、スクロールバー、テキストエディタ）も合う。以前はテーマを指定せず、iced が OS の配色に合わせていた。今は選んだものを明示する。
- **テーマと別に扱うもの:** 3D ビューポートの背景色（`AppState.scene.background_color`、View → Background Color）は、テーマを切り替えても変わらない。分子を描く色（原子・鎖・二次構造・相互作用・分子表面）と、ターミナルの ANSI の色の対応表（端末のデータ）も同じ。ターミナル自身の背景・文字・カーソルの色はパレットにあるが、ターミナルの中のプログラムは暗い背景を前提に ANSI の色を使うので、明るいパレットでも暗いままにする。
- **切り替えと保存:** View → Theme: Dark / Light（コマンド `view.set_theme`、引数 `theme`。今のものにチェックが付く）。切り替えはすぐ反映され、利用者ごとの設定に `theme: dark|light` として書き込む。ネイティブ版は `settings.yaml` の `theme:` の行（なければ雛形から作る。コメントと他のキーは残す。`set_theme_in_text`）、ブラウザ版は `localStorage` の文字列。書き込みに失敗してもアプリは止まらず、テーマは切り替わり、ステータスバーに警告を出す。起動時は、`theme` キーを他の設定と同じように読む（8.6節）。`dark` / `light` 以外の値は、警告を出して暗いテーマにする。
- **対象外:** 上の3D の描画の色、OS の配色への自動追従、利用者が作るパレット。

### 8.9 選択（利用者ごと）
「どの原子が選ばれているか」は、Representation とは別の状態として持つ: `Yui.selection`（原子 ID の `BTreeSet<u32>`。操作は `frontend/src/app/selection_ops.rs`、純粋な規則は `frontend/src/selection.rs`）。**選択は利用者ごとのもので、`AppState`、ワークスペースの YAML、協調編集には入れない**（選択してもYAML と `AppState` が変わらないことを、自動テストで確かめている）。残したい選択は、名前付き選択として保存する（View →「Save Selection as Named Selection」、コマンド `selection.save_as_named`）。名前付き選択は、従来どおりワークスペースに含まれる。構造・ワークスペースを開いたとき、File → New で、選択は捨てる。
- **左クリック:** 左ボタンを押して `CLICK_MAX_DISTANCE` = 4 px 以内で離すとクリック、それより動かすとカメラのドラッグ（回転）。押した位置から 4 px 以内の動きではカメラを動かさないので、クリックは、ポインターの下にあった原子を選ぶ。右クリックと同じレイキャスト（`ViewportProgram::pick_atom`）で選び、`Message::ViewportClicked` でアプリへ知らせる。原子のないところのクリックは、選択を外す。
- **左ドラッグ:** ドラッグは、押している修飾キーでカメラの動きが変わる（`viewport::camera::DragMode::from_modifiers`。動くたびに読むので、ドラッグ中にキーを変えてもよい）。修飾キーなし: 注視点のまわりを回す（`orbit_camera`）。Shift: 平行移動、つまり視点と注視点を画面に平行に一緒に動かす（`pan_camera`。1 ピクセルで、注視点の距離で `2 · 距離 · tan(fov/2) / 分子図の高さ` Å だけ分子が動くので、どの拡大率でも分子はポインターについてくる。向きと距離は変わらない）。Ctrl（macOS は Cmd）: 縦の動きで拡大・縮小（`drag_zoom_factor`。下へドラッグで縮小、上へで拡大。距離の範囲はホイールと同じ）。両方を押していれば Shift を優先する。macOS では Ctrl+クリックがシステムの右クリックになるので、Cmd を使う。修飾キー+クリック（ドラッグなし）は、これまで通り選択（追加 / 切り替え、8.9節）。ホイールはこれまで通りズーム。
- **修飾キー:** なし = 置き換え、Shift = 追加、Ctrl（macOS は Cmd）= 切り替え（選ばれていれば外す。両方なら切り替え）。Shift や Ctrl を押して何もないところをクリックしても、選択は残る。
- **単位:** クリックは原子単位。**ダブルクリック**（400 ms 以内・6 px 以内の 2 回目のクリック）は、クリックした原子の残基全体（同じ鎖・同じ残基番号）を選ぶ。単位はメニューの設定にはしない。
- **外す:** Esc キー（テキストの部品が Esc を使ったときを除く）、または View → Clear Selection（コマンド `selection.clear`）。
- **ほかの経路:** 右クリックのメニューの Select Atom / Select Residue と、構造ツリーのノードのクリック（とそのメニューの Select）は、黄色の Representation を足す代わりに、この選択を置き換える。以前に保存したワークスペースにある `sel_atom_…` / `sel_res_…` / `tree_sel_…` の Representation は、通常の Representation としてそのまま読み込む（消さない）。
- **強調の設定:** View → Selection Highlight: Glow（既定）/ Halo / Off（コマンド `view.set_selection_highlight`、引数 `style`。今のものにチェックが付く）。選んだ方法は、テーマと同じ利用者ごとの設定（8.6節、8.8節）で、`settings.yaml` のトップレベルのキー `selection_highlight: glow|halo|off`（ネイティブ版。その行を書き換え、コメントと他のキーは残す）、またはブラウザ版の `localStorage` に保存する。正しくない値は、警告を出してグローにする。強調の描き方は 4.10 節。

### 8.10 再生（利用者ごと）
「フレーム」の列を、時間に合わせて進める仕組み。`frontend/src/playback.rs`（純粋な規則と時計）、`frontend/src/app/playback_ops.rs`（フレームを表示するモデルに結び付ける処理）、操作のバーは `frontend/src/playback_bar.rs`。**再生は利用者ごとのもので、`Yui.playback` に持ち、`AppState`、ワークスペースの YAML、協調編集には入れない**（`AppState.trajectory` には触らない。軌跡ファイル用に取ってある）。
- **フレーム。** 再生の基盤が知っているのは「フレームの総数」と「今のフレーム」だけで、フレームが何かは知らない。フレームは、マルチモデル構造のモデル（`model_atom_ranges` の順に `model_1`、`model_2`、…）で、`AppState.active_model` を書き換えて表示する。軌跡を開いているとき（4.2節）は、軌跡のフレームで、原子の座標を差し替えて表示する。`Yui::frame_count`、`current_frame`、`show_frame` が、フレームと表示するものを結び付ける唯一の場所なので、軌跡のフレームも、ここに足した（4.2節）。
- **時計。** 再生中だけ、`iced::time::every`（ネイティブも WASM も）が、1 フレームの長さの半分の間隔（最速で 16 ms）で `Message::PlaybackTick` を届ける。止まっているときは購読しないので、何も起きず、再描画もない（`Playback::tick_interval` が `None`）。進めるフレーム数は、タイマーの回数ではなく、経過時間と速さ（`Playback::steps_due`。1〜30 fps、既定 5、端数は持ち越す）から求める。描画が遅れても再生は遅くならず、間のフレームを飛ばして追いつく（1 回に 1000 まで）。
- **ループの方法。** `step` / `advance`（純粋関数）: *Once* は最後で止まる（そのあと分子表面を作り直す）、*Repeat* は先頭に戻る、*Ping-pong* は端のフレームを 2 回続けて出さずに向きを反転する。全モデル表示から始めると先頭のモデルから。*Once* で最後にいるときに始めると、先頭からやり直す。
- **操作。** フレームが 2 つ以上あるときだけ、「3D Viewport」のタブの下端に出す: 先頭へ / 1 つ戻る / 再生・一時停止 / 1 つ進む / 末尾へ、フレームのスライダー、フレームの名前と番号、速さ（1〜30 fps）、ループの方法（押すたびに切り替えるボタン）。同じ操作を、コマンドの登録簿のコマンド（`playback.play_pause`、`playback.first`、`playback.prev`、`playback.next`、`playback.last`、`playback.set_loop`）としても持ち、View →「Playback」に出す（2 つ以上のフレームがあるときだけ押せる）。キー: Space = 再生 / 一時停止、← / → = 1 フレーム戻る / 進む。どのウィジェット（文字入力欄、エディタ、ターミナル）も処理しなかったキーだけを受ける。
- **ステータスバー。** 右の、表示中のモデルの欄に `model_3 (3/20)` を出し、再生中は先頭に `Playing` を付ける。再生中は、フレームごとに `StatusMessage` を出さない（手でモデルを切り替えたときのメッセージは、今まで通り）。
- **手での変更。** 再生中に、構造ツリーでモデルを選ぶと、再生を止めて、そのモデルを表示する。構造やワークスペースを開く、File → New をすると、再生を止める（`update_model_atom_ranges` が止める）。1 つ戻る / 進む、先頭 / 末尾のボタンとスライダーは、再生中なら再生を続ける。
- **選択。** 原子の ID はモデルごとに違うので、**表示するモデルが変わったら、選択を外す**（再生、ボタン、スライダー、構造ツリーのどれでも）。既知の制約: `Selector::AtomIds` の Representation と名前付き選択には触らないので、モデルが変わると、何も選ばなくなることがある。
- **分子表面。** 再生中と、スライダーを動かしている間、分子表面は**遅れて追従**する（ネイティブ版）。すべてのフレームを作るのではなく、`Yui::follow_surface` が、作っている最中でなければバックグラウンドで表示中のモデルの生成を始め、作っている最中なら `surface_follow_pending` に印を付けるだけにする。その生成の結果が届いたとき（最新の世代なので捨てられない）、そのとき表示しているモデルでもう一度作る。間のフレームは飛ばすので、分子表面は原子より、生成にかかる時間ほど遅れる（1HLS、782 原子、格子 0.5 Å で、リリースビルドで約 43 ms。開発機で 1 回測っただけで、デバッグビルドは数倍遅い）。再生が止まったとき（一時停止、または最後に着いた）、スライダーを離したときは、表示中のモデルで作り直し（4.4節）、途中の古い生成は捨てる。止まっているときのボタンによるコマ送りは、手でモデルを切り替えるときと同じく、作り直す。生成が同期的で UI を止めてしまうブラウザ版では、これまでどおり、再生中とドラッグ中は分子表面を描かない（`Playback::hides_surface`）。非共有結合相互作用の線は、毎フレーム、表示中のモデルで絞り込む。その負荷は、ここでは測っていない。モデルごとのメッシュを前もって作る方法は、範囲外。
- **2つの分子図。** 2つとも同じ `active_model` を描くので、両方が再生に追従する。操作のバーは、1つ目の分子図の下だけにある。

### 8.11 カメラのアニメーション（利用者ごと）
カメラの自動回転、首振り、滑らかな移動。カメラの計算は純粋関数（`frontend/src/camera_anim.rs`）で、いつ動かすか、タイマー、止める条件は `frontend/src/app/camera_anim_ops.rs`。**アニメーションは利用者ごとのもので、`Yui.camera_anims`（分子図ごとに 1 つ）は `AppState`、ワークスペースの YAML、協調編集に入れない。** アニメーションは、普通の経路（1つ目の分子図は `AppState.camera`、2つ目は `Yui.secondary_camera`、8.1節）でカメラを変えるだけで、この画面で変えたカメラは、ドラッグと同じく協調編集では送らないので、通信があふれることはない。
- **種類。** *Spin* は、注視点のまわりを、マウスの左右のドラッグと同じ軸（`orbit_camera`）で、一定の速さで回す（View →「Spin」。「Spin Speed」で 10 / 20 / 40 / 80 度/秒を順に切り替え、既定 20）。*Rock* は、オンにした時点のカメラを中心に、±30° を、周期 4 秒の正弦波で往復する（View →「Rock」）。*Move* は、今のカメラから別のカメラへ、0.4 秒で、始まりと終わりをゆっくりにして（`smoothstep`）動く。Spin と Rock は、どちらか一方が動く。Move は、その分子図の Spin / Rock を置き換える。
- **補間（Move）。** カメラを成分ごとには補間しない。`interpolate_camera` は、注視点を線形、注視点からの距離を対数（拡大・縮小の見え方が一定になる）、注視点から視点への向きと up の向きを球面補間（`slerp`。反対向きのときは、垂直な向きを通る）で補間する。そのため、距離は両端の距離の間に収まり、2つの視点がどれだけ離れていても、カメラが注視点を横切ることはない。画角は線形に補間する。0 と 1 では、結果は始まりと終わりに一致する。
- **滑らかに動くもの。** View → Reset Camera と View →「Zoom to Selection」。合わせる視点は、これまで通り分子図のウィジェットが求める（ペインの縦横比が要るため、8.1節）。結果が届いたとき、要求が Reset Camera / Zoom to Selection なら（`Yui.animate_fit`）、すぐに切り替えず、今の視点を保って、合わせた視点への Move を始める。構造、ワークスペースを開く、File → New のときの合わせ込みは、これまで通り、すぐに切り替える。Zoom to Selection は、選ばれている原子に合わせる（`ViewportProgram::fit_selection`、`selection::positions_of`）。規則は構造全体に合わせるときと同じ（原子の余白 2 Å、余白の倍率 1.1）で、選択がないと押せない。
- **タイマー。** 動いているアニメーションが 1 つ以上ある間だけ、`iced::time::every(16 ms)` が `Message::CameraAnimTick` を届ける。ないときは、購読しない。進める量は、経過時間（1 回に 0.1 秒まで）から求めるので、速さはタイマーの回数に依存しない。再生と同じく、タイマーの時刻は使わない（WASM のタイマーは別の型の `Instant` を返すため）。
- **止まる条件。** 分子図でのマウス操作（ドラッグ、ホイール。ウィジェットが、合わせた結果ではない視点の変更として知らせる）は、その分子図のアニメーションを止める。視点が連動していれば、もう一方も止める。構造、ワークスペースを開く、File → New、Reset Camera / Zoom to Selection の要求は、先にすべてのアニメーションを止める。
- **2つの分子図。** View メニューの操作は、最後に視点を動かした分子図に対して行う（分割をオフにしていれば1つ目）。「Link Viewport Cameras」がオンのとき、Spin と Rock は、両方の分子図に同じ視点を入れる。合わせた視点への Move は、縦横比が違うので、合わせ込みと同じく、分子図ごとに行う。

## 9. ライセンス・配布方針

- **ライセンス:** GPLv3。依存する `ProteinDF_bridge`（およびそのRust移植版 `proteindf-bridge`）もGPLv3であり、ライセンス上の矛盾はない。
- **配布形態:** macOS (dmg), Linux (deb と AppImage。rpm は作っていない)、Windows (NSIS の setup .exe。msi は作っていない。下記) のネイティブインストーラに加え、`backend` + WASM `frontend` によるブラウザ実行を提供する。
- モデリング外部ツールの同梱要否は別途検討する。
- **インストーラー（対応済み。署名なし。確認したのは Linux だけ）:** `cargo-packager` で作る（設定は `frontend/Cargo.toml` の `[package.metadata.packager]`。製品名 YUI、識別子 `io.github.proteindf.yui`、アイコンは `packaging/icons/`（`packaging/make-icons.py` で生成。PNG と `.ico`。`.icns` は cargo-packager が作る）、バイナリは `yui-frontend`（主）と `yui-backend`、リリースビルドを行う `beforePackagingCommand`。バージョンはワークスペースのもの）。`scripts/build-installers.sh` が、そのマシンの OS 向けを作る。形式は、Linux が `.deb` と `.AppImage`、macOS が `.dmg`、Windows が NSIS の `setup.exe`。CI は `.github/workflows/release-installers.yml`: **手動実行だけ**（`workflow_dispatch`。すべて作ると時間がかかる。macOS が最も長い）。`ubuntu-22.04`（deb と AppImage。移植性のため古い glibc で作る）、`macos-14`（dmg。Apple Silicon だけ）、`windows-latest`（NSIS）のマトリクスで、ファイルを artifacts として上げる（14 日。GitHub のリリースへは手で付ける）。Debian のパッケージは、winit と wgpu が実行時に読み込むライブラリに依存する（バイナリがリンクするのは libc、libm、libgcc だけ）。**署名も公証もしていない**（証明書もアカウントも設定していない）。最初の起動で、macOS と Windows が警告する。署名には、Apple Developer のアカウントとコード署名の証明書、つまりお金と秘密の判断が要り、cargo-packager はどちらにも対応している。**`.msi` は作らない:** WiX は、製品バージョンの最初の欄が 256 未満であることを求め、カレンダーバージョン（`2026.10.4`）はこれを破る。cargo-packager には、対応づける方法がないので、Windows のインストーラーは NSIS の `.exe` にした（上の `msi` の記述に代わる）。`rpm` も作らない。ここで確認したこと: `.deb` と `.AppImage` のビルド、deb のメタデータと中身（`dpkg-deb`）、deb から取り出したバイナリと AppImage が起動すること（仮想の画面、ソフトウェア描画）、ワークフローの YAML が読めること。**確認していないこと:** macOS と Windows のインストーラー、ワークフロー自体（一度も動かしていない）、きれいなシステムへの deb のインストール、実 GPU。
- **Web 版の配備（対応済み。ローカルでの利用）:** `scripts/build-web.sh` が、リリースビルドの一式（`trunk build --release`、wasm-opt。`target/web/dist` は約 14 MB、`yui-web-<version>.zip` は約 6 MB。サブパスには `--public-url`）を作る。`backend` は、`YUI_WEB_ROOT=<dist>` で、それを配信する（ルーターのフォールバックに `tower_http::services::ServeDir`。`/api/` のルートはすべて、それより先に当たる。`/` は `index.html`、`.wasm` は `application/wasm`、ないものは 404、フォルダの外は決して配信しない。フォルダに `index.html` がなければ、メッセージを出して終了する）。ページと API が同じオリジンから来るので、既定のコラボの URL（1 章）は、設定が要らない。開発用の `trunk serve` は、`/api/` と WebSocket の `/api/collab` を `127.0.0.1:8080` に転送する（`frontend/Trunk.toml`）。`backend` がなくても、同じ一式がスタンドアロンで動くので、ふつうの静的ホストにも置ける。
- **ネットワークへの公開（フェーズ6j は完了）。** `backend` は、既定で `127.0.0.1:8080` にバインドする。`YUI_BIND` で変えられるのは、`YUI_BEHIND_TLS_PROXY=1` と一緒のときだけ（TLS は、リバースプロキシの仕事）で、そのときも、ターミナルとファイルの API は、`YUI_ENABLE_TERMINAL=1` / `YUI_ENABLE_FS=1` がない限り閉じている（7.4節）。すべての API が秘密を求める: Host の、起動ごとのランダムな秘密か、Editor / Viewer の招待（期限つき、取り消せる。ターミナルを開くことも、状態を変えることも、できない）。認証の失敗は、回数を制限する。対象外: ユーザーアカウント / シングルサインオン、ロールの中の利用者ごとの権限。静的な一式そのものには秘密がなく、どこにでも置ける。
- 未了: `backend` 自身の応答での `.wasm` の圧縮（そのままで 13.5 MB。リバースプロキシや静的ホストが圧縮するし、zip は 6 MB）、`ServeDir` の既定以外のキャッシュヘッダー、インストーラーの署名（上記）。

---

## 10. 外部依存プロジェクト: `proteindf-bridge`

構造データI/O・フォーマット変換・構造前処理（水素付加、イオン対、SSBond、末端キャッピング、重ね合わせ等）は、既存の Python 資産 `ProteinDF_bridge`（`../ProteinDF_bridge`, GPLv3）をRustに1:1移植した **`proteindf-bridge`** として、本リポジトリとは別プロジェクトで管理する。

- **多言語対応:** Rust製コアライブラリを中心に、
  - **C/C++:** `cdylib` + `cbindgen` によるヘッダー生成でCABIを公開。
  - **Python:** `PyO3` + `maturin` によるバインディング（既存 `ProteinDF_bridge` ユーザーの移行コストを抑える）。
  - **Rust:** ネイティブクレートとして本プロジェクトの `core`（無条件の依存として参照し、ネイティブ向けには `zstd`、`wasm32` 向けには `ruzstd` feature をターゲット別に指定）から利用し、`frontend` ではテスト用の `dev-dependencies` としてのみ参照。`renderer-native` は直接依存せず `core` を経由して利用する。
- **移植範囲:** データモデル（`PeriodicTable`, `Vector`, `Matrix`, `Position`, `Atom`, `Bond`, `AtomGroup`）、フォーマット変換（PDB, mmCIF, MOL2, GRO, XYZ, Amber PRMTOP）、構造操作（`Select`, `AminoAcid`, `SSBond`, `IonPair`, `Neutralize`, `Modeling`, `Superposer`/`Superposer_quaternion`）を1:1移植し、既存の `tests/test_*.py` をRustテストとして移植して正当性を担保する。
- **新フォーマット対応:** **PDBx/mmCIF** を正式な主力フォーマットとして堅牢にサポートする。レガシーPDB形式はカラム桁数制約により原子数99,999件・特定のチェーンID表現等にハード上限があり、4.2節の「最大約100万原子」目標と両立しない。既存 `ProteinDF_bridge/mmcif.py` は `SimpleMmcif` という名の通り簡易実装であるため、`proteindf-bridge` では大規模構造を確実に往復できるレベルまで堅牢化する（詳細仕様はproteindf-bridge側の `SPEC.md` で定義）。
- **ProteinDF計算結果I/Oの統合:** 構造データ（`ProteinDF_bridge`由来）とは別に、ProteinDFの計算結果（`pdfparam.h5`: 基底関数系・MO係数行列・軌道エネルギー・Mulliken電荷等のポピュレーション解析）を扱う既存Pythonライブラリ **`ProteinDF_pytools`**（`orbinfo.py`, `matrix.py`/`vector.py`, `basisset.py`/`basis2.py`, `pdfparam_hdf5.py`, `poputils.py` 等）も `proteindf-bridge` に統合し、Rust側でHDF5読み込み（`hdf5-metno`クレート等）に対応する。
- **Cubeファイル対応はパーサのみで良い:** ProteinDF本体のC++ツール（`pdf-mkfld-dens`/`pdf-mkfld-mo`/`pdf-mkfld-esp`, 実装は `src/tools/mkfld_common.cpp` の `saveCubeData()`）が標準Gaussian CUBE形式でファイル出力する仕組みを既に持つため、`proteindf-bridge`側でGTOをグリッド展開してcubeを生成する処理は**不要**。標準Gaussian CUBEファイルの読み込みパーサ（4.5節の等値面パイプラインへの入力）のみを実装すればよい。
- **本リポジトリでの扱い:** 本プロジェクトの `core`（および `frontend` の `dev-dependencies`）からは、`proteindf-bridge` をGit依存（リリースタグ `tag = "2026.10.7"` 指定、将来的にはcrates.io）として参照し、`renderer-native` は直接依存しない。詳細設計・実装は `../ProteinDF_bridge/RUST_PORT_SPEC.md` で管理する。

### 10.1 二次構造推定（DSSP相当）

現行 `ProteinDF_bridge` には存在しないため、`proteindf-bridge` 側で新規実装する（4.3節のリボン表示で使用）。Kabsch-Sanderの静電モデル（`E = q1*q2*(1/r(ON) + 1/r(CH) - 1/r(OH) - 1/r(CN)) * 332`、`E < -0.5 kcal/mol` で水素結合と判定）による主鎖 N-H...O=C 水素結合パターンからヘリックス/シートを推定する、オリジナルDSSPと同様のアルゴリズムを採用する。この主鎖水素結合検出処理は、10.2節の水素結合検出とロジックを共有する。

### 10.2 非共有結合相互作用の検出（塩橋・水素結合・CH-π等）

分子構造から非共有結合的な相互作用を検出し、構造データとは独立した **`InteractionSet`**（`{ kind, atoms, distance, angle, donor/acceptor role }` のリスト）として出力する機能を `proteindf-bridge` に持たせる。他のフォーマット同様、MessagePack/YAMLで往復可能にし、`core`/レンダラー側は再計算せずこれを消費できるようにする。

| 種別 | 実装方針 |
| --- | --- |
| **ジスルフィド結合** | 既存 `ssbond.py`(`SSBond`) を1:1移植（SG-SG距離 < 2.31Å）。 |
| **塩橋 (Salt Bridge)** | 既存 `ionpair.py`(`IonPair`) を1:1移植（GLU/ASP等の官能基中心とLYS/ARG/末端基の中心が4.0Å未満）。 |
| **水素結合 (Hydrogen Bond)** | **新規実装。** 主鎖水素結合は、10.1節のDSSP相当アルゴリズムの内部処理（Kabsch-Sander静電モデルによるN-H...O=C検出）をそのまま流用する。側鎖水素結合（Ser/Thr/Tyrの水酸基、Asn/Gln/Hisのアミド/イミダゾール等）は、ドナー/アクセプター原子タイプ表を新設し、距離(D...A < 3.5Å目安)・角度(D-H...A > 120°目安)の幾何基準で検出する。水素原子が明示的に存在しない構造（水素なしPDB等）向けに、重原子のみでの簡易判定モードも用意する。 |
| **CH-π相互作用** | **新規実装。** 芳香環を持つ残基(PHE/TYR/TRP/HIS)の環構成原子テーブルを新設し、環の重心・法線ベクトルを算出。C-H側の原子（または水素）と環重心の距離、および環法線とのなす角を幾何基準として検出する。文献上の定義に幅があるため、既定値（例: 距離4.5Å以内・角度40°以内）は**チューニング可能なパラメータ**として公開し、決め打ちにしない。 |

**計算タイミング:** `InteractionSet` は基本的に構造読み込み時（1フレーム分のスナップショット）に計算する。軌跡再生中に毎フレーム再計算するのはコスト（特に100万原子規模）が高いため、既定はロード時1回計算の静的オーバーレイとし、「フレームごとにライブ再計算する」は任意設定（既定オフ）とする。近傍探索は4.2節で導入する空間分割（BVH/Octree）を再利用し、全原子対の総当たり(O(n²))を避ける。

**表示（フェーズ6i-ii 実装仕様）:**
- **データモデル:** 6.2節の `Representation` に `kind: RepresentationKind::Interaction` および個別種別フィルタ `interaction_kinds: Vec<InteractionKind>`（既定は空、空のときは全種別を表示）として統合。UIインスペクタおよびPython APIから種別ごと（`HydrogenBond`、`SaltBridge`, `CHPi`, `Disulfide`）の個別トグルが可能。
- **選択規則:** 相互作用の両端点のうち「少なくとも一方の端点が `Representation.selection` に含まれる原子集合と重複する」場合に描画（`core::interaction::resolve_interaction_atom_ids` で解決）。`Representation.selection` が `Selector::All` の場合は `interaction_kinds` に一致する全相互作用を表示。
- **配色:** 相互作用の種別ごとの意味的既定色を適用: 水素結合=黄（ゴールド、`[1.0, 0.85, 0.1, 1.0]`）、塩橋=オレンジ（`[1.0, 0.5, 0.0, 1.0]`）、CH-π=シアン（`[0.0, 0.8, 0.9, 1.0]`）、ジスルフィド結合=マゼンタ（`[0.85, 0.2, 0.85, 1.0]`）。`color` に `ColorScheme::Uniform(c)` が指定された場合は単色で上書き。
- **破線ジオメトリとシェーダー:** 両端点を結ぶ円柱スティックインスタンスを半径 `0.08 Å`（共有結合スティック `0.12 Å` より細い）で配置。フラグメントシェーダー（`fs_interaction`）にて円柱軸に沿ったÅ単位の距離を周期 `0.60 Å`、実線部 `0.35 Å`、空白部 `0.25 Å`（`dist - floor(dist / period) * period > 0.35` で破棄）で判定し、相互作用の長さによらず均一な破線パターンを描画。
- **カリングとLODの扱い:** 相互作用数は原子数に比べ圧倒的に少ない（1HLS 全体で38本、20モデル全展開でも877本）ため、GPU視錐台カリングおよびビルボードLOD階層は経由せず、両面描画（`cull_mode: None`）・深度テスト付きの専用wgpuパイプラインで直接描画（常時明瞭に視認可能）。

**読み込み時の検出と端点座標の導出（フェーズ6i-i 実装仕様）:**
- **検出と保存:** 構造読み込みパイプライン（`structure_loader::parse_structure_bytes`）において、結合トポロジ解決（`setup()`）および二次構造推定の後、モデルごとに `proteindf_bridge::InteractionSet::detect_all(model, None, None)` を呼び出し、結果を1つの `InteractionSet` にまとめて `AppState.interactions` に保存する（描画側での再計算は行わない）。
- **描画用端点座標の解決 (`resolve_interaction_endpoints`):** 各 `Interaction` の両端の3次元座標（`[f32; 3]`）を次の規則で求める:
  1. パスが原子を指す場合: その原子の 3D 座標。
  2. CH-π 相互作用の芳香環（残基パス）: `proteindf_bridge::ch_pi::calc_ring_geometry` で求めた芳香環の重心。
  3. 塩橋（残基・官能基パス）: **【暫定仕様】** `proteindf-bridge` 側に官能基中心座標を返す公開関数が提供されるまでの暫定措置として、該当グループに含まれる全構成原子の重心（算術平均）を使用する。
  4. 端点の解決に失敗した場合はパニックせず `None` を返す。
- **端点対応原子ID集合の導出 (`resolve_interaction_atom_ids`):** `iter_flat_atoms` と同一の順序・1始まり連番で各端点に対応する原子ID（原子指定なら1件、残基指定ならその残基配下の全原子ID）を解決する。6i-ii において「`Representation.selection` に含まれる原子に関わる相互作用だけを表示する」フィルタリングに使用される。

### 10.3 構造ファイルの書き出し（PDB / mmCIF）

YUI は `core::structure_export::export_structure_to_string` において、`proteindf-bridge` の `Pdb` および `SimpleMmcif` を直接呼び出すことで、PDB および mmCIF 形式への構造ファイル書き出しを提供する（ROADMAP §5の原則: YUI 側でフォーマットシリアライザを自作しない）。
- **モデルの書き出し範囲:** マルチモデル構造（NMRアンサンブル等）の場合、現在ビューポートやツリーで表示選択されているモデル（`active_model`）に関わらず、全モデルを書き出す。これによりモデルの欠損を防ぎ、可逆的な保存を保証する。
- **電荷の扱い:**
  - **PDB:** `is_charge2tempfactor = false` とし、電荷を温度因子フィールドではなくPDBの電荷カラムに書く。bridge はこのカラムを整数として書く（bridge の `pdb.rs` の `format!("{:+1.0}", charge)`）ため、整数でない部分電荷（`.mol2` 由来など）は整数に丸められる。つまり、PDB の書き出しでも部分電荷は保持されない。
  - **mmCIF:** 既定オプション（`charge_to_b_factor = false`）を使用する。PDBx/mmCIF仕様上の形式電荷（`_atom_site.pdbx_formal_charge`）は整数値に制限されているため、部分電荷を持つ構造（`.mol2`由来など）を書き出そうとした場合は、数値を勝手に丸めたりオプションを自動切り替えしたりせず、利用者にPDB形式での書き出しを案内する分かりやすいエラーを返す。このエラーには、PDB でも部分電荷が整数に丸められることを明記する。

---

## 11. QCLObot 連携（フラグメント成長計算の可視化）

`QCLObot` のプレイブック形式ファイルを読み込み、フラグメントベース計算プロセスそのものを可視化対象とする。

### 11.1 `*.QCLO.yaml`（QCLO計算プレイブック）
各タスクは `brd_file`（構造ファイル参照）と `fragments:` リストを持ち、各フラグメントは `brd_select: /model_1/A/6/` のような**パス文字列**で構造を参照する。これは6.2節で定義した `Selector`（`proteindf-bridge`の`Select_Path_wildcard`/`Select_PathRegex`）と同一構文であるため、追加のパーサを新設せず既存の `Selector` でそのまま解決できる。

- **ステップ再生:** QCLO法はStep1（単一残基）→Step2（3残基窓）→Step3以降（順次拡張）という段階的拡張で計算されるため、既存の軌跡（Trajectory）再生UI・GPUリングバッファ機構（5章）を転用し、計算ステップをアニメーション再生できるようにする。
- **フラグメント表示:** ステップごとのフラグメント分割を色分け表示し、`add_ACE`/`add_NME` で付与されたキャッピング基は本来の構造と区別できるよう半透明等で描画する。
- **QCパラメータ表示:** 選択中フラグメント/タスクの `basis_set`, `XC_functional`, `guess`, `J_engine`/`K_engine`/`XC_engine` をインスペクタパネル（右ペイン）に表示する。

### 11.2 双方向リンク & YAML編集（ツリー ⇔ YAML本文 ⇔ 3Dビュー）

`.QCLO.yaml` のツリー表示・YAML本文表示・3Dビューの三者を**双方向にリンク**させ、かつYAML本文は**その場で編集可能**にする。

- **span保持パース:** YAMLは `serde_yaml` ではなく、ノードごとに元テキストの行・列レンジ（span）を保持できるパーサ（例: `marked_yaml`）で読み込む。これにより「本文の任意位置 → 対応するツリーノード」の逆引きが可能になる。
- **`SelectionIndex`:** 各ツリーノード(Task/Fragment) → `Selector`（`brd_select`をパース） → `proteindf-bridge`で解決した原子ID集合、という対応表をキャッシュする。ツリークリック・本文クリックのどちらも、最終的にはこのインデックス経由で `AppState.highlight: HighlightSet(Vec<AtomId>)` を更新するだけでよい。
- **逆方向（3D→ツリー/本文）:** 3Dビューで原子をクリックした場合、現在アクティブなStep（再生中のフラグメント成長ステップ）に属するノードの中から、その原子を含むものを`SelectionIndex`で逆引きし、対応するツリーノードをハイライト・スクロールし、YAML本文側も該当行レンジをハイライト・スクロールする。複数Stepにまたがる曖昧一致を避けるため、逆引きは「現在アクティブなStepの範囲内」に限定する。
- **編集方式（フォーマット非破壊）:** ユーザーの編集経路は2つあるため、方式を分ける。
  1. **本文編集（iced `text_editor` ウィジェット）:** ユーザーが直接テキストを編集した場合は、変更をデバウンスして再パースし、ツリー/`SelectionIndex`/3Dハイライトを再構築する。パース失敗時はエラーガター表示とし、直前の正常なハイライト状態を保持する。
  2. **ツリー操作起点の編集（GUI操作、例: フラグメント名変更・並べ替え）:** ドキュメント全体を再シリアライズするとコメントやレイアウトが失われるため、**該当ノードのspan範囲だけを文字列置換するサージカル編集**を行い、それ以外のテキスト（コメント、セクション区切り等）はそのまま保持する。
- **保存:** 編集はメモリ上（テキストバッファ）に即時反映されるが、ディスクへの書き込みは明示的な保存操作（Ctrl+S 等）で行い、未保存の変更はタブに変更マーク（dirty indicator）を表示する。

### 11.3 `modeler.yml`（構造前処理プレイブック）
`protonate` → `opt` → `neutralize` → `md`(solvation) → `opt` のように、各タスクが `reference:` で前段タスクを参照するDAG構造を持つ。これをUI上でDAG表示し、各ノードは3章で定義した `ModelingProvider` の呼び出し（水素付加・構造最適化・イオン中和・MD溶媒和等）に対応させる。ノード選択で該当ステージの構造をプレビューできるようにする。

### 11.4 巨大構造コンテキスト中のQM計算領域のハイライト

本プラットフォームの狙いは「正準分子軌道計算そのものの可視化研究」であり、QM計算対象（QCLObotで定義されたフラグメント群）自体は数千〜数万原子規模だが、**それをクライオEM複合体等の巨大な構造的コンテキスト（〜100万原子）の中に位置づけて表示する**ことを主要ユースケースとする（4.2節の100万原子対応GPU駆動レンダリングは、主にこの用途のために必要となる）。

- QCLObotのプレイブックで定義されたQM計算対象原子集合（`brd_select`で解決される全フラグメントの和集合）を、周囲の巨大構造（結晶構造・クライオEM構造等、通常は同一 `AtomGroup` 内の残り部分）に対して視覚的に区別する専用ハイライト（例: QM領域は通常表示、それ以外はモノクロ/低彩度・半透明の"コンテキスト表示"）を提供する。
- LOD（4.2節）とも連動させ、QM領域は常に最高詳細度（ball-and-stick/リボン等）、コンテキスト領域は距離に応じてインポスター/表面のみに簡略化する、という描き分けを既定動作とする。

---

## 12. 正準分子軌道計算の可視化研究支援機能

タンパク質正準分子軌道計算（ProteinDF/QCLObot）そのものの可視化研究を主目的とするため、以下の4機能を追加する。

### 12.1 MOブラウザ（大規模軌道リストのナビゲーション）

タンパク質全体の正準MO計算では占有軌道だけで数千〜数万本に達するため、専用の閲覧UIを設ける。

- **一覧・フィルタ:** `pdfparam.h5` から読み込んだ軌道エネルギー・占有数・スピンチャンネル（UHF/ROHFの場合）を、HOMO/LUMOを基準にした相対インデックス（HOMO-5〜LUMO+5等）やエネルギー範囲でフィルタ・ソートできる一覧表示。数万件規模になるため、iced側は表示行を仮想化（バーチャルスクロール）して描画負荷を抑える。
- **エネルギー準位図:** HOMO/LUMOを明示したエネルギー準位（ラダー）図を2D表示し、クリックで一覧・3Dビューと連動させる。
- **オンデマンドcube生成:** 全軌道分のcubeファイルを事前生成するのは非現実的なため、cubeが未生成の軌道が選択された場合、3章の `ModelingProvider` と同様の外部ツール呼び出し抽象化を用いて、ProteinDF本体の `pdf-mkfld-mo` をその場で実行しcubeを生成・読み込む（既存cubeがあればそれを優先利用し、生成状況をアイコン等で一覧に表示する）。

### 12.2 軌道別ポピュレーション/寄与マッピング

選択中のMOに対し、どの原子・どのフラグメントがどれだけ寄与しているかを構造上に可視化する。

- `proteindf-bridge`（`ProteinDF_pytools`統合分）に、MO係数ベクトルと重なり行列(S)から**軌道ごとの原子別Mulliken寄与**を計算する関数を新設し、`OrbitalContribution { orbital_index, atom_contributions: Vec<(AtomId, f32)> }` として出力する。
- 描画側は、11.2節で導入した `HighlightSet`（原子ID集合の2値ハイライト）を一般化した **`ScalarOverlay(Vec<(AtomId, f32)>)`** を新設し、寄与の大小を色グラデーション（例: 青-白-赤）として基本表現の色に重畳する。この仕組みは将来的にB-factorやRMSF等、他の原子ごとのスカラー値の可視化にも転用できる。

### 12.3 QCLO成長ステップ間の収束・忠実度比較

11章で可視化するのはフラグメント分割の**トポロジー**だが、QCLO法自体の妥当性検証には**各ステップの計算結果**の比較が本質的に重要となる。

- 各 `.QCLO.yaml` タスク（ステップ）に対応する計算結果（`pdfparam.h5`）を関連付け、ステップを跨いだ以下の指標を算出・表示する:
  - **エネルギー収束:** タスクの全エネルギー（または注目フラグメントの軌道エネルギー）をステップ順に並べた折れ線グラフ。
  - **QCLO忠実度:** 前段で得たQCLOと後段の正準軌道との重なり積分（`proteindf-bridge`の行列演算機能で計算、対角成分が1に近いほど良い引き継ぎ）。
  - **幾何RMSD:** 既存の `Superposer`（Kabschアルゴリズム）を用いた、ステップ間で共通する原子部分構造のRMSD。
- 専用パネル「QCLO収束ビュー」でこれらを可視化し、任意のステップをクリックすると3Dビュー・MOブラウザ（12.1節）が該当ステップの結果に同期する。

### 12.4 複数計算の比較（差分表示）

異なる計算条件（基底関数系、XC汎関数、フラグメント分割方式等）の結果を並べて比較する。

- 中央パネルの複数タブ機構（8章）を用い、2つ以上の `pdfparam.h5` 結果をタブ間でカメラ同期しながら並べて表示する「比較モード」を設ける。
- **差分密度/差分MO表示:** 2つの計算のcube格子が同一空間（同一原点・同一グリッド間隔）であれば、ボクセル単位の差分（`cube_A - cube_B`）を計算し、正負の領域を異なる色の等値面として表示する（結晶学における差分電子密度マップと同様の見せ方）。グリッドが異なる場合はリサンプリングしてから差分を取る。
- 総エネルギー・HOMO-LUMOギャップ等のスカラー量は、比較用のサマリーテーブルとして並記する。

---

## 13. ターミナル & ログビューワー

本プラットフォームの主要ユースケースは、長時間実行されるProteinDF/QCLObot量子化学計算ジョブの監視・制御であるため、8章のBottomペインに、既存のPythonコンソールと並んでシェルターミナルと `tail -f` 相当のログビューワーを追加する。

### 13.1 実現可能性

両機能とも、既存アーキテクチャと矛盾なく技術的に実現可能である。5章の非同期I/O設計、および7章のWebSocket差分同期の仕組みをそのまま再利用できる。新規に必要となる実装は主に (a) `iced` には組み込みのターミナルウィジェットが存在しないため、ターミナルのセルグリッドを描画するカスタムウィジェット、および (b) `CollabPacket` と並走する生バイト/行ストリーム用チャンネルへのトランスポート層の拡張、の2点である。

### 13.2 ターミナル
- **セッションの所在:** 通常は `backend` 側でホストする。これにより `frontend` はネイティブ/WASMいずれの場合も、7.2節の `CollabTransport` と同系統のトランスポート抽象を介してターミナルセッションと通信する。PTY を扱う処理は、ネイティブのクレート `yui-pty`（`pty/`）にあり、`backend` が使う。ネイティブのデスクトップ版は、同じクレートで「Connect (Local shell)」（standalone。backend なし）も使う。同じマシンのシェルを、プロセス内で動かし、端末の画面は変えない。WASM（ブラウザ版）にはローカルのシェルがないので、必ず `backend` が要る。
- **専用WebSocketエンドポイント:** `backend` に双方向ストリーミング用エンドポイント `/api/terminal?rows={rows}&cols={cols}` を新設（秘密は、URL ではなく、ヘッダーか WebSocket のサブプロトコルで渡す。7.4節）。キーストローク/コマンドを上り、PTY生出力を下りとして伝送する。
- **PTYの起動:** `portable-pty` クレート（Unix pty / Windows ConPTY を吸収するクロスプラットフォームPTY）を用いる。子プロセス（`$SHELL` または `/bin/sh`）を `TERM=xterm-256color` で起動し、バックグラウンドスレッドで入出力を仲介する。
- **ターミナルの状態機械:** ANSI/VTエスケープシーケンス解析とセルグリッド状態管理について `alacritty_terminal` と `vt100` を比較検証した結果、前者はウィンドウ管理やレンダリング系と密結合で過剰であるのに対し、**`vt100`（v0.16）** は純粋なインメモリデータ構造として動作し、C依存ゼロ、完全なWASM互換性、充実したVT100/xtermエミュレーション、256色/RGB、スクロールバック、カーソル追跡を備えることから採用した。
- **描画:** カスタム `iced` ターミナルペイン（`frontend/src/terminal_pane.rs`）が `vt100::Screen` の状態を読み出し、`iced::widget::rich_text` と色付き `span` の連続ランによりセルグリッドを描画する。等幅フォント（Monospace）、各セルの前景色（`vt100_color_to_iced` によるANSI 16色、256色カラーキューブ/グレースケール、および24bit Truecolor RGBのマッピング）、太字（bold）属性、カーソル強調表示（`█`）、ステータスツールバー、クイックコマンド/制御キーボタン（Enter, Tab, Ctrl+C）に対応する。セルの背景色（`cell.bgcolor()`）、反転（SGR 7: 前景と背景の入れ替え）、下線も、`span` の背景・下線として描く（`CellStyle`）。背景色か反転のある空白セルは、行末のトリムで落とさないので、行全体の帯（`less` の下の行など）が出る。描かないもの: 斜体、薄字、取り消し線、点滅。
- **入出力経路:** 全二重WebSocketチャンネル。キー入力やコマンド送信は生バイト列またはJSON制御メッセージ（`TerminalInput`）としてキューイング送信され、PTY出力バイト列（`TerminalOutput`）は直接 `vt100::Parser::process` に投入される。WASMでは、`gloo-net` WebSocket を用い、ユーザー入力送信タスクとPTY出力受信タスクを並行駆動しつつ、`mpsc` を介して `iced` の購読ストリームへブリッジする。
- **直接の入力と大きさ。** ターミナルの画面をクリックすると、画面がキーボードを受け取る（枠がシアンになる）。ほかの場所のクリック、タブの切り替え、ペインを隠すこと、切断で、元に戻る。受け取っている間は、どの部品も処理しなかったキーを、`terminal_pane::key_bytes` で変換して入力として送る（文字、Ctrl+文字、ESC つきの Alt+文字、矢印・Home・End・PageUp・PageDown・Delete・Insert・F1〜F12 はエスケープ列、プログラムが `vim` や `less` のようにアプリケーションカーソルのモードにしたときは `ESC O A` 形式の矢印）。この間は、アプリのショートカット（Esc、Space、矢印、Ctrl+S）は購読されない（キーの購読を差し替える。8章）。Cmd+キーと Ctrl+Shift+キーは送らない（Ctrl+Shift は、コピー・貼り付けのために空けてある。まだ作っていない）。画面の下の入力欄は残す（日本語などの入力メソッド経由の入力と、貼り付けのため）。画面の大きさは `sensor` で測り、文字の大きさから行・桁を計算する（`grid_for`。幅は文字の大きさの 0.6 倍、高さは 1.3 倍 + 1 px）。変わったら `vt100` の画面の大きさを変え、JSON のメッセージ `{"type":"resize","rows":R,"cols":C}`（`TerminalOut::Resize`）で PTY に伝える。接続の URL の行・桁は、「Connect」を押したときの値（`connect_rows` / `connect_cols`）で固定する（URL が購読の識別子なので、変えるとつなぎ直しになる）。
- **範囲選択・コピー・貼り付け:** 画面をドラッグするとセルを選ぶ（`selection`。行・桁の組で、マウスの位置から、グリッドと同じ見積もりのセルの大きさで求める。パレットの選択色で強調する。ドラッグせずにクリックすると外れ、出力で選んだ文字が変わったときも外れる）。コピー（`vt100` の `contents_between`、行末の空白は落とす）と貼り付けは、`Yui::terminal_clipboard` が `iced::clipboard` で行う。Ctrl+Shift+C/V、Cmd+C/V、Shift+Insert（`clipboard_key`。Ctrl+C だけは、これまでどおり割り込み）か、Copy / Paste ボタンで呼ぶ。貼り付ける文字は、プログラムがブラケット貼り付けを有効にしていれば ESC[200~ … ESC[201~ で囲み、そうでなければ改行を CR にする。履歴の表示でも選べる（スクロールすると選択は外れる）。
- **履歴（スクロールバック）:** `vt100` のパーサーが 1000 行の履歴を持つ。マウスのホイール（1 目盛り 3 行、`Scrolled`）と Shift+PageUp/PageDown（`scroll_key`。1 画面より 1 行少ない）で、`set_scrollback` により表示を動かす（新しい出力が来ても、`vt100` が、見ている行を動かさない）。履歴を見ている間は、カーソルを描かず、ツールバーに位置を出す。何か入力する（`enqueue_input`）と、今の画面に戻る。代替画面（`less`、`vim`）には履歴がないので、そこでのスクロールは無視する。
- **複数セッション:** フェーズ6a-iiでは単一セッションを実装。将来拡張として複数タブ機構（8章、12.4節）に対応可能とする。
- **アクセス制御:** ターミナルセッションはホストマシン上で事実上任意のコマンド実行を許すため、接続開始時点で7.1節の `RoleResolver` によるロール検証を厳格に適用する。**Host ロールのみに許可**され、秘密がなければ HTTP 401 Unauthorized、知らない秘密や Editor / Viewer の秘密は HTTP 403 Forbidden で拒絶する。ほかの Web サイトからの要求（`Origin` / `Host`。7.4節）は、さらに先に拒絶し、loopback 以外のバインドでは、`YUI_ENABLE_TERMINAL=1` がない限り、このエンドポイントは閉じている。

### 13.3 ログビューワー
- **入力ソース:** (a) 任意のログファイルパス（ローカル、または `backend` を実行しているマシン上）、(b) 本プラットフォーム自身が既に呼び出しているサブプロセス — 3章の `ModelingProvider` 外部ツール呼び出し、および12.1節の `pdf-mkfld-mo` によるオンデマンドcube生成 — の標準出力/標準エラー出力を直接キャプチャしたもの、の2種類。
- **専用ストリーミングエンドポイント & セキュリティ:** バイナリMessagePack（`AppStateDelta`）を伝送する協調編集用エンドポイント `/api/collab` に混在させず、独立したWebSocketエンドポイント `/api/logs?path={path}&lines={initial_lines}` を新設した。これによりライフサイクル管理やデータ型（JSON `LogStreamMessage` による行ストリーム）が分離され、複数のログソースを独立に購読可能となる。
- **許可ディレクトリ制限 & パストラバーサル防止:** 任意のシステムファイル（SSH鍵や設定ファイル等）の漏洩を防ぐため、tailing対象は `backend` 側の許可ディレクトリ `log_root`（既定: `./logs`、環境変数 `YUI_LOG_ROOT` で設定可能）の配下に厳格に制限される。クライアントから受け取ったパスは `..` による親ディレクトリ移動やルート絶対パス等の不正コンポーネントを排除した上で `log_root` と結合され、実パスの正規化（canonicalize）によって `log_root` 配下にとどまっていることが検証される。範囲外へのトラバーサル要求は HTTP 403 Forbidden で即座に拒絶される。
- **アクセス制御 & ロール検証:** 匿名アクセスを許さず、7.1節のコラボレーション機能と同様に `RoleResolver` を用いたトークン認証を行う。接続クエリにトークンが存在しない場合は HTTP 401 Unauthorized、無効または未解決のトークンの場合は HTTP 403 Forbidden で WebSocket アップグレード前に接続を遮断する。ログビューワーは読み取り専用であるため、解決された正当なロール（Host, Editor, Viewer）すべてに閲覧が許可される。
- **tail機構:** `backend` が `notify` クレート（macOS: FSEvents, Linux: inotify, Windows: ReadDirectoryChangesW）と非同期チャネルを組み合わせてファイルの増分を監視する。ストリーミング開始時点で対象ファイルがまだ存在しない場合（ジョブ起動待ち等）は非同期ループで待機し、生成検知とともに自動でtailingを開始する。追記されたバイト列はファイルオフセットからUTF-8行としてデコードされ、7.3節の差分同期原則に沿ってクライアントへプッシュされる。WASM環境ではクライアントが `gloo-net` WebSocket を介して接続し、受信した行メッセージをネイティブと全く同一のリングバッファへシームレスに格納する。
- **表示およびクライアント状態:** クライアント側（`frontend`）では、受信した行を上限付きリングバッファ（既定5,000行、上限超過時は古い行から順次破棄）へ蓄積する。UIは `tail -f` 相当の自動スクロール（手動トグル/一時停止対応）、部分文字列による即時フィルタリング、パス入力ツールバー、クリア機能を備える。
- **バックプレッシャー/サイズ制限:** ProteinDF/QCLObotのジョブログは数GBに達することがあるため、クライアント側はメモリ上に直近の行のみを上限付きリングバッファとして保持する。接続時は末尾N行（既定100行）のみを初期読み出しする。

### 13.4 UI配置

両機能とも、8章および14.2節で定義したBottomペインに、4タブバー（**Log Viewer / Python Console / Terminal / AI Chat**）として統合配置する。ボタンによるタブ切り替えに対応し、レイアウトの他ペインと同様に `pane_grid` で個別にリサイズ可能とする。

---

## 14. AIチャットアシスタント & LLMプロバイダ抽象化

従来6.3節で扱っていた単発の音声コマンド処理に加え、読み込み済みの構造・計算結果について質問応答でき、ツール呼び出し（tool-calling）を通じて6.1/6.2節で定義したPython APIを操作できる、常駐・対話型の **AIチャットアシスタント** を追加する。アシスタントは **pluggableな `LlmProvider` 抽象** を介して動作し、3章の `ModelingProvider`、7.2節の `CollabTransport` と同じ設計思想により、クラウドAPI（Claude、Gemini）とローカルLLMをユーザーが切り替えられるようにする。

### 14.1 `LlmProvider` 抽象
```rust
trait LlmProvider {
    fn chat(&self, messages: &[ChatMessage], tools: &[ToolSpec]) -> Result<ChatStream>;
}
```
- Anthropic（Claude）、Google（Gemini）、および Ollama のOpenAI互換HTTPエンドポイント経由で提供されるローカルモデルは、いずれもHTTPのchat completion APIとしてアクセス可能であり、違いはエンドポイントURL・認証ヘッダー・薄いリクエスト/レスポンススキーマアダプタのみである。そのため `reqwest` ベースの単一実装で3種類すべてをカバーでき、プロバイダ固有の実装はアダプタモジュールのみに限定される。
- 使用するバックエンド（モデル名、エンドポイント、APIキー）はYAMLで設定し、3.1節の `ModelingProvider` バックエンド切り替え機構を踏襲する。
- **ホスティング:** 13.2節でターミナルに採用したパターンに倣い、`LlmProvider` の呼び出しは（ネイティブデスクトップ版であっても）常に `backend` 側から行う。これによりAPIキーがWASMクライアントに到達することはなく、`frontend`（ネイティブ/Webいずれも）は既存のWebSocket/RESTトランスポート経由で `backend` とのみ通信すればよい。これはチャット履歴の永続化（14.5節）を一箇所に集約する上でも都合が良い。

### 14.2 チャットパネル（UI）
- 8章/13.4節で定義したBottomペインに、Pythonコンソール/ターミナル/ログビューワーと並ぶ4番目のタブとして追加する: **Pythonコンソール / ターミナル / ログビューワー / AIチャット**。
- 標準的なチャットUI: スクロール可能なメッセージ一覧（ユーザー/アシスタントの発話、トークン単位でストリーミング表示）と、テキスト入力欄で構成する。
- 音声入力（6.3節、Web Speech API）は、同じ入力欄にテキストを流し込む代替手段として扱う。すなわち音声認識結果は独立した単発パイプラインを経由するのではなく、そのままチャットメッセージとなる。

### 14.3 既存Python API上でのツール呼び出し
- アシスタントに独自のコマンド言語は持たせない。ツール呼び出しは6.2節で既に定義済みの `Selector`/`Representation` Python API（`yui.select(...).show(...).color_by(...)` 等）へ1:1で対応させ、二重の実行系を維持する必要をなくす。
- **コンテキスト付与:** 構造データ全体（最大約100万原子）をLLMへ送るのではなく、`backend` が現在の選択範囲・鎖/残基サマリー・MOブラウザの状態（12.1節）・QCLO収束指標（12.3節）等をまとめた軽量な **状態ダイジェスト（state digest）** を組み立て、これをモデルがツール呼び出しで読み取れる関数（例: `get_selection()`, `get_orbital_info(index)`）として公開する。モデルはそのターンで実際に必要な情報だけを要求すればよい。
- 生成されたツール呼び出しは、呼び出し元ユーザーの7.1節のロール権限で実行される。例えばViewerのチャットセッションは状態の照会・説明の取得はできるが、Viewerがマウス/Python直接操作で `AppState` を変更できないのと同様に、`AppState` の変更はできない。

### 14.4 セキュリティ・プライバシー
- 構造データや計算結果の一部でもクラウドプロバイダ（Claude、Gemini）へ送信することは、未発表の構造や独自計算のデータがローカル/クラスタ環境の外に出ることを意味し、重要な検討事項となる。そのため、クラウド版 `LlmProvider` バックエンドの有効化はワークスペース単位の明示的なオプトインとし、既定のデータがローカルに留まる選択肢としてローカルLLMバックエンド（Ollama）を提供する。
- APIキーは `backend` の設定（例: 環境変数）にのみ保持し、`frontend` には一切公開しない。これは14.1節の「常に`backend`経由」という設計と整合する。
- 14.3節の状態ダイジェストは配列情報やアノテーションを含み得るため、フィールド単位で送信可否を設定可能とする。例えば幾何由来のスカラー量（RMSD、エネルギー等）はクラウドプロバイダへの送信を許可しつつ、残基名/配列情報は伏せる、といった運用ができるようにする。

### 14.5 ストリーミングと履歴
- 応答はストリーミング（Claude/Gemini/Ollamaの各APIがネイティブ対応するSSE）で受信し、7.3節と同様の差分同期スタイルのWebSocketチャンネル経由でクライアントへ逐次転送する。これは5章の非同期I/Oタスクパターンを再利用する。
- チャット履歴は（`frontend`ではなく）`backend`側でセッション/ワークスペース単位に永続化し、クライアントの再接続後も失われないようにする。また2.1節のYAML状態と併せてエクスポートできるようにし、「アシスタントに何を尋ね、何を実行したか」を再現可能な記録として残す。

### 14.6 6.3節との関係
6.3節はこれまで、音声認識テキストを解析しそのままPythonコマンドへ変換する単発パイプラインとして記述していた。この流れは本章に統合され、音声は14.2節で定義した同一のチャットメッセージストリームへの入力手段の一つに過ぎず、Python実行は14.3節で述べたツール呼び出しループの結果として起こり得る一つの帰結にすぎない、という位置付けになる。
