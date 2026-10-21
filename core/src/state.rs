use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// 3次元ベクトル
pub type Vector3 = [f32; 3];
/// RGBAカラー (0.0〜1.0)
pub type Color4 = [f32; 4];

/// カメラ状態
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CameraState {
    pub eye: Vector3,
    pub target: Vector3,
    pub up: Vector3,
    pub fov: f32,
}

impl Default for CameraState {
    fn default() -> Self {
        Self {
            eye: [0.0, 0.0, 100.0],
            target: [0.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            fov: 45.0,
        }
    }
}

/// デフォルトの3Dビューポート背景色（index.html のダークテーマ背景色 #121316 に合わせる）
pub const DEFAULT_BACKGROUND_COLOR: Color4 = [0.0706, 0.0745, 0.0863, 1.0];

/// 3Dビューポートのシーン全体設定（背景色など）。
/// 将来的にフォグ・グリッド表示等の追加設定もこの構造体に集約する想定。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SceneSettings {
    pub background_color: Color4,
}

impl Default for SceneSettings {
    fn default() -> Self {
        Self {
            background_color: DEFAULT_BACKGROUND_COLOR,
        }
    }
}

/// 原子（Atom）の情報
/// 1AON（約58,000原子）の高速パース・レンダリングに耐えうるコンパクトな設計
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Atom {
    pub id: u32,
    pub element: String,
    pub position: Vector3,
    pub radius: f32,
    pub color: Color4,
    pub residue_name: String,
    pub residue_index: i32,
    pub chain_id: String,
}

/// 結合（Bond）の情報
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Bond {
    pub atom_a: u32,
    pub atom_b: u32,
    pub order: f32, // 結合次数 (1.0, 2.0 等)
}

/// 分子（Molecule）の構造データ
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Molecule {
    pub atoms: Vec<Atom>,
    pub bonds: Vec<Bond>,
}

/// 3D空間内のアノテーションデータ
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Annotation {
    pub id: String,
    pub text: String,
    pub position: Vector3,
    pub color: Color4,
    pub visible: bool,
    /// 紐づく原子のID
    pub target_atom_id: Option<u32>,
}

/// 開いている CUBE の表示の設定（データごとに決めて、ワークスペースに保存する）。
/// どの軌道（データセット）をどの等値レベルで見るか、と、面の見た目。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct CubeView {
    /// 表示するデータセット（軌道）の番号（0 から）
    pub dataset: usize,
    /// 等値レベル（正の値。負の面は `-isolevel`）。0 以下は「そのデータセットの既定値」
    pub isolevel: f32,
    /// 等値面の不透明度（0.0〜1.0）
    pub opacity: f32,
    /// 正の面の色（フロントエンドの色の一覧の番号）
    pub positive_color: usize,
    /// 負の面の色（同上）
    pub negative_color: usize,
}

impl Default for CubeView {
    fn default() -> Self {
        Self {
            dataset: 0,
            isolevel: 0.0,
            opacity: 0.6,
            positive_color: 0,
            negative_color: 1,
        }
    }
}

/// 外部データファイルへの参照パス
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct DataPaths {
    /// PDBファイルのパス（オプション）
    pub pdb_path: Option<String>,
    /// グリッドデータ（Cube等）のパス（オプション）
    pub cube_path: Option<String>,
}

use crate::atom_group::{AtomGroup, AtomGroupExt, SecondaryStructureMap};
use crate::representation::Representation;
use crate::selector::Selector;
use proteindf_bridge::InteractionSet;

/// `AppState`の状態フォーマットバージョン。
///
/// バージョン1: Phase 6e以前のローカルスタンドイン`AtomGroup`
///   （`Model`/`Chain`/`Residue`/`AtomRecord`による独自ツリー構造）。
/// バージョン2: Phase 6e-ii以降、実際の`proteindf_bridge::AtomGroup`をラップした
///   newtypeによる形式（`rmpv`経由のシリアライズ）。
///
/// この値と一致しない`version`を持つYAML/MessagePackファイルは、
/// 構造的に非互換であり読み込めない（`core::packing::unpack_app_state`/
/// `core::yaml::import_app_state_from_yaml[_str]`が読み込み前に検証し、
/// 明確なエラーを返す）。
pub const ATOM_GROUP_FORMAT_VERSION: u32 = 2;

/// 統合可視化プラットフォーム「結 (YUI)」の全体状態。
/// (architecture.md - 2. データ構造 & 状態管理)
/// スレッド安全 (Send + Sync) であり、ポインタコピーを避けるためのデータ構造。
///
/// 【フィールド追加時の規則（後方互換性維持）】
/// `AppState` に新しいフィールドを追加する際は、**必ず構造体の末尾に配置し、`#[serde(default)]` を付与**してください。
/// 構造体の途中にフィールドを追加すると、MessagePack の配列形式でシリアライズされた旧データ（2026.10.0 等）を
/// デシリアライズする際に、以降のフィールド値が位置ずれを起こして誤ったフィールドに読み込まれ、
/// エラーにならずに内部状態が破損する原因となります。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppState {
    /// 状態のバージョン
    pub version: u32,

    /// camera情報
    pub camera: CameraState,

    /// 3Dビューポートのシーン全体設定（背景色など）
    #[serde(default)]
    pub scene: SceneSettings,

    /// 名前付き選択グループ（名前 → Selector）。表示スタイルを持たず、複数のRepresentationから
    /// 再利用できる名前付き集合（PyMOLの `select groupname, ...` に相当）。
    #[serde(default)]
    pub named_selections: BTreeMap<String, Selector>,

    /// 分子構造データの真実の単一情報源（Single Source of Truth）
    /// (architecture.md - 2.2節)
    /// proteindf-bridge から受け取る鎖→残基→原子の階層木構造。
    pub atom_group: Option<AtomGroup>,

    /// 二次構造ラベルの一時的なサイドマップ（residueのpath文字列 → SecondaryStructureKind）。
    /// `atom_group.rs`の`SecondaryStructureMap`と同じ理由の一時的代替実装。
    #[serde(default)]
    pub secondary_structure_map: SecondaryStructureMap,

    /// レンダリング用キャッシュとしての平坦化分子データ。
    ///
    /// 【注意: 単方向派生（One-way derivation）】
    /// 本フィールドは GPU インスタンシング描画等の便宜のために `atom_group` から
    /// 単方向に派生（`to_flat_molecule`）して生成・更新されるキャッシュです。
    /// `molecule` への直接変更が `atom_group` へ逆流・自動反映されることはありません。
    pub molecule: Molecule,

    /// レンダリング表現（Representation）のリスト。
    /// (architecture.md - 6.2.2節)
    /// 表示の重なりは後勝ち（PyMOL方式）で解決されます。
    pub representations: Vec<Representation>,

    /// 3Dアノテーションのリスト
    pub annotations: Vec<Annotation>,

    /// 外部データファイルへのパス（YAML相対パス解決用）
    pub data_paths: DataPaths,

    /// 軌跡再生のステート
    pub trajectory: TrajectoryState,

    /// 共同作業中のレーザーポインター状態
    pub laser_pointer: LaserPointer,

    /// 非共有結合相互作用およびジスルフィド結合（proteindf-bridge の InteractionSet）。
    /// 構造読み込み時に一度だけ検出され、描画側での再計算は行わない。
    ///
    /// 【フィールド追加規則】
    /// 配列形式の旧データ（2026.10.0 等の12要素データ）との後方互換性を保ち、位置ずれによる
    /// 意図しないデータ破損を防ぐため、本フィールドのように新設されるフィールドは必ず
    /// 構造体の末尾に配置し、`#[serde(default)]` を付与して旧データ読み込み時にデフォルト値
    /// （空の `InteractionSet`）が適用されるようにする。
    #[serde(default)]
    pub interactions: InteractionSet,

    /// マルチモデル構造（NMRアンサンブル等）において現在表示対象として選択されているモデルのキー。
    /// `None` の場合は全モデルを表示する。
    ///
    /// 【フィールド追加規則】
    /// 構造体の末尾に配置し、`#[serde(default)]` を付与することで、旧形式のワークスペースや
    /// 配列形式データを読み込んだ際にデフォルト値（`None`＝全モデル表示）が適用される。
    /// 本フィールドはローカル表示設定であり、協調編集（`AppStateDelta`）の差分同期対象には含めない。
    #[serde(default)]
    pub active_model: Option<String>,

    /// 開いている CUBE の等値面の見た目（不透明度と色）。
    ///
    /// 【フィールド追加規則】構造体の末尾に置き、`#[serde(default)]` で旧データには既定値を使う。
    #[serde(default)]
    pub cube_view: CubeView,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            version: ATOM_GROUP_FORMAT_VERSION,
            camera: Default::default(),
            scene: Default::default(),
            named_selections: Default::default(),
            atom_group: Default::default(),
            secondary_structure_map: Default::default(),
            molecule: Default::default(),
            representations: Default::default(),
            annotations: Default::default(),
            data_paths: Default::default(),
            trajectory: Default::default(),
            laser_pointer: Default::default(),
            interactions: Default::default(),
            active_model: None,
            cube_view: Default::default(),
        }
    }
}

impl AppState {
    /// `atom_group` が存在する場合、そこから `molecule`（レンダリング用キャッシュ）を再生成・同期する。
    pub fn sync_molecule_from_atom_group(&mut self) {
        if let Some(ref ag) = self.atom_group {
            self.molecule = ag.to_flat_molecule();
        }
    }
}

/// AppStateの差分(Delta)構造体。
/// 帯域を節約し低遅延を実現するため、同期にはAppState全体ではなく差分のみを送信する。
/// (architecture.md - 7.3 差分(Delta)メッセージ同期)
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct AppStateDelta {
    pub camera: Option<CameraState>,
    pub scene: Option<SceneSettings>,
    pub named_selections: Option<BTreeMap<String, Selector>>,
    pub representations: Option<Vec<Representation>>,
    pub annotations: Option<Vec<Annotation>>,
    pub laser_pointer: Option<LaserPointer>,
}

/// 軌跡（Trajectory）データの単一フレーム
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FrameData {
    pub step: u32,
    /// 各原子の座標位置（対応する atom.id 順）
    pub positions: Vec<Vector3>,
}

/// 軌跡データのメタ情報とバッファリング状態
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct TrajectoryState {
    pub file_path: String,
    pub total_frames: u32,
    pub current_frame_index: u32,
    pub play_speed_fps: u32,
    pub is_playing: bool,
}

/// 共同作業中のレーザーポインター情報
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct LaserPointer {
    pub active: bool,
    pub position: Vector3,
    pub user_id: String,
    pub color: Color4,
}
