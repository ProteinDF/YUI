use iced::widget::pane_grid;
use serde::{Deserialize, Serialize};

use crate::context_menu::{ContextMenuAction, ContextMenuTarget};
use crate::file_open_dispatch::FileOpenHandler;
use crate::log_viewer;
use crate::menu_bar::MenuAction;
use crate::named_selections::NamedSelectionAction;
use crate::pane_view::ViewId;
use crate::project_tree_pane;
use crate::representation_inspector::{RepContextMenuAction, RepresentationAction};
use crate::structure_tree::{StructureTreeAction, TreeContextMenuAction};
use crate::terminal_pane;
use crate::text_editor_pane;
use yui_core::state::{AppState, AppStateDelta};

/// エディタのグループの識別子（中央の領域を分割した葉。`architecture.md` 8.1節）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GroupId(pub u32);

/// ペインの種別（第8章 レイアウト）。
///
/// 固定のドック（Left / Right / Bottom）と、中央の領域のエディタのグループ（`Group`）がある。
/// グループは、いくつでも、左右にも上下にも分割できる（`pane_view::GroupNode`）。最初の 2 つの番号には、名前がある
/// （`CENTER` と `CENTER_RIGHT`。保存の YAML のキー `center` と `center_right`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PaneKind {
    Left,
    Right,
    Bottom,
    Group(GroupId),
}

impl PaneKind {
    /// 最初のグループ。3D Viewport を必ず置く（以前の `Center`）
    pub const CENTER: PaneKind = PaneKind::Group(GroupId(0));
    /// 2 つ目のグループ。中央を左右に分割したときの右側（以前の `CenterRight`）
    pub const CENTER_RIGHT: PaneKind = PaneKind::Group(GroupId(1));

    pub fn title(&self) -> &'static str {
        match *self {
            PaneKind::Left => "Explorer / Playbook",
            PaneKind::Right => "Inspector",
            PaneKind::Bottom => "Bottom Panels",
            // エディタのグループは、どのタブでも置けるので、見出しは表示中のタブの名前にする（`Yui::view`）。
            // これは、タブのない（空の）グループの見出し。
            PaneKind::Group(_) => "Editor",
        }
    }

    /// 保存の YAML のキー（以前の `serde(rename_all = "snake_case")` と同じ名前）
    pub fn key(&self) -> String {
        match *self {
            PaneKind::Left => "left".to_string(),
            PaneKind::Right => "right".to_string(),
            PaneKind::Bottom => "bottom".to_string(),
            PaneKind::CENTER => "center".to_string(),
            PaneKind::CENTER_RIGHT => "center_right".to_string(),
            PaneKind::Group(GroupId(n)) => format!("group_{n}"),
        }
    }

    pub fn from_key(key: &str) -> Option<PaneKind> {
        Some(match key {
            "left" => PaneKind::Left,
            "right" => PaneKind::Right,
            "bottom" => PaneKind::Bottom,
            "center" => PaneKind::CENTER,
            "center_right" => PaneKind::CENTER_RIGHT,
            other => PaneKind::Group(GroupId(other.strip_prefix("group_")?.parse().ok()?)),
        })
    }

    /// 並び順（保存の YAML のキーの順を、以前の `Left, Center, Right, Bottom, CenterRight` と同じに保つ）
    fn rank(&self) -> u64 {
        match *self {
            PaneKind::Left => 0,
            PaneKind::CENTER => 1,
            PaneKind::Right => 2,
            PaneKind::Bottom => 3,
            PaneKind::Group(GroupId(n)) => 3 + u64::from(n),
        }
    }
}

impl Ord for PaneKind {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.rank().cmp(&other.rank())
    }
}

impl PartialOrd for PaneKind {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Serialize for PaneKind {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.key())
    }
}

impl<'de> Deserialize<'de> for PaneKind {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let key = String::deserialize(deserializer)?;
        PaneKind::from_key(&key)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown pane: {key}")))
    }
}

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)] // アプリケーションイベントの型安全な統一メッセージ定義としてバリアント直接保持を維持
pub enum Message {
    PaneResized(pane_grid::ResizeEvent),
    PaneDragged(pane_grid::DragEvent),
    InitialStateFetched(Result<AppState, String>),
    CollabConnected,
    CollabDisconnected,
    CollabDeltaReceived(AppStateDelta),
    /// タブを押した（選択し、ドラッグを始める）
    PaneTabSelected {
        pane: PaneKind,
        view: ViewId,
    },
    /// マウスがタブの上に入った（ドラッグ中なら、押したタブをそこへ移す）
    PaneTabHovered {
        pane: PaneKind,
        view: ViewId,
    },
    /// タブのドラッグ中に、マウスがエディタのグループの本体（タブの帯の下）の、ある場所（中央・端）に入った
    TabDropTargetEntered(PaneKind, crate::pane_view::DropZone),
    /// タブのドラッグ中に、マウスがその場所から出た
    TabDropTargetLeft(PaneKind, crate::pane_view::DropZone),
    /// マウスのボタンを離した（タブのドラッグを終える。本体の中央なら末尾へ移し、端なら分割する）
    TabDragEnded,
    /// タブの帯の上で、縦のホイールを回した（横にスクロールする）
    TabBarWheel {
        pane: PaneKind,
        dx: f32,
    },
    LogViewerMsg(log_viewer::LogViewerMessage),
    TerminalPaneMsg(terminal_pane::TerminalPaneMessage),
    /// テキストファイル（ドキュメント）のエディタの操作
    Editor(text_editor_pane::EditorMessage),
    /// File → Open Text File... のダイアログの結果
    TextFileOpened(Result<Option<(String, Option<std::path::PathBuf>, String)>, String>),
    /// ドキュメントのタブの閉じるボタン
    CloseTabRequested(text_editor_pane::DocumentId),
    /// 未保存のファイルを閉じるときの確認の選択
    CloseDialogChoice(text_editor_pane::CloseChoice),
    /// Ctrl+S（macOS は Cmd+S）: 選んでいるテキストファイルを保存
    SaveActiveDocument,
    /// 終了の要求（File → Quit、ウィンドウを閉じる操作）。未保存のファイルがあれば確認する
    QuitRequested,
    /// 終了するときの、未保存のファイルの確認の選択
    QuitDialogChoice(text_editor_pane::CloseChoice),
    ProjectTreePaneMsg(project_tree_pane::ProjectTreePaneMessage),
    FileContentFetched {
        path: String,
        handler: FileOpenHandler,
        result: Result<Vec<u8>, String>,
    },
    OpenHandlerChosen(FileOpenHandler),
    /// 「Open with」の選択を、開かずに閉じる
    OpenHandlerCancelled,
    /// ファイルの右クリックメニュー（Open with）で、開き方を選んだ
    OpenFileWith {
        path: String,
        handler: FileOpenHandler,
    },
    /// ファイルの右クリックメニューを閉じた（メニューの外をクリックした）
    FileContextMenuClosed,
    MenuAction(MenuAction),
    /// メニューの項目が呼ぶコマンド（登録簿が、既存の操作のメッセージに引き当てる）
    RunCommand(crate::menu::CommandInvocation),
    /// プラグインのコマンドを、Python の専用スレッドで実行する（`payload` は `{"target", "args"}`）
    PluginCommand {
        plugin: String,
        id: String,
        payload: serde_json::Value,
    },
    /// プラグインの一覧を開く（View → Plugins...）
    PluginsDialogOpen,
    /// プラグインの一覧を閉じる
    PluginsDialogClose,
    /// 一覧で、このプラグインの有効・無効を切り替える（設定に書く。次回の起動から効く）
    PluginToggled(String),
    /// プラグインのコマンドが終わった。失敗なら理由が入る（そのプラグインは外される）
    PluginCommandDone {
        plugin: String,
        result: Result<(), String>,
    },
    /// 分子図の視点が変わった（ドラッグ、ホイール、構造に合わせた結果）
    ViewportCameraChanged {
        id: crate::viewport::ViewportId,
        camera: yui_core::state::CameraState,
        /// 構造に合わせた結果か（連動では伝えない）
        fitted: bool,
    },
    ViewportRightClicked {
        screen_pos: [f32; 2],
        target: Option<ContextMenuTarget>,
    },
    ContextMenuAction(ContextMenuAction),
    StructureTreeAction(StructureTreeAction),
    TreeContextMenuAction(TreeContextMenuAction),
    RepresentationAction(RepresentationAction),
    RepContextMenuAction(RepContextMenuAction),
    NamedSelectionAction(NamedSelectionAction),
    CursorMoved([f32; 2]),
    /// マウスの左ボタンを、どこかで押した（ターミナルの画面にキー入力を送っている間だけ購読する）
    MousePressed,
    /// ターミナルの画面にキー入力を送っている間の、キーの押下
    TerminalKey {
        key: iced::keyboard::Key,
        modifiers: iced::keyboard::Modifiers,
        text: Option<String>,
    },
    /// クリップボードから読んだ文字を、ターミナルへ貼り付ける（読めなかったときは `None`）
    TerminalPasteText(Option<String>),
    /// 分子図の左クリック（ほとんど動かさずに離した）。選択を変える。
    ViewportClicked(crate::selection::ViewportClick),
    /// 散布図の左クリック（点、または何もないところ）。選択を変える。
    PlotClicked(crate::pane_view::PlotKind, crate::plot::PlotClick),
    /// pytools のグラフ（JSON）を開いた結果（File → Open Plot (pytools JSON)...）。
    /// `(ファイル名, JSON の文字列)`。キャンセルは `Ok(None)`
    PytoolsPlotOpened(Result<Option<(String, String)>, String>),
    /// 軌跡を開いた結果（File → Open Trajectory...、Project タブ）。キャンセルは `Ok(None)`
    TrajectoryOpened(
        Result<Option<std::sync::Arc<yui_core::xyz_trajectory::XyzTrajectory>>, String>,
    ),
    /// CUBE の等値面を開いた結果（File → Open CUBE...）。キャンセルは `Ok(None)`
    CubeOpened(Result<Option<crate::cube_view::CubeOpenResult>, String>),
    /// CUBE のカードの操作（軌道の選択、等値レベル、閉じる）
    CubeAction(crate::cube_view::CubeAction),
    /// CUBE の等値面の計算が終わった
    CubeJobDone(Result<std::sync::Arc<crate::cube_view::CubeIsosurface>, String>),
    /// カメラのアニメーションの操作（Spin、Rock、Zoom to Selection）
    CameraAction(crate::camera_anim::CameraAction),
    /// カメラのアニメーションのタイマー（時刻は、受け取った側で `Instant::now()` を読む）
    CameraAnimTick,
    /// 再生の操作（操作のバー、メニュー、キーボード）
    PlaybackControl(crate::playback::PlaybackAction),
    /// 再生中のタイマー（時刻は、受け取った側で `Instant::now()` を読む。WASM のタイマーは、別の型の時刻を返すため）
    PlaybackTick,
    /// 選択を外す（Esc キー、メニュー）
    ClearSelection,
    /// 今の選択を、名前付き選択として保存する
    SaveSelectionAsNamed,
    /// ウィンドウの大きさが変わった（起動時の大きさも、この形で届く）
    WindowResized(iced::Size),
    SurfaceGenerated {
        generation: u64,
        results: Vec<crate::surface_render::SurfaceBuildResult>,
    },
    WorkspaceFileOpened(Result<Option<(String, Option<std::path::PathBuf>, String)>, String>),
    WorkspaceFileSaved(Result<Option<(String, Option<std::path::PathBuf>)>, String>),
    StructureExported(Result<Option<(String, Option<std::path::PathBuf>)>, String>),
    /// SVG の書き出しのダイアログの操作
    SvgDialog(crate::svg_export::SvgDialogAction),
    /// PNG の書き出しのダイアログの操作
    PngDialog(crate::png_export::PngDialogAction),
    /// 「Share Session...」ダイアログの操作と、その要求の結果
    Share(crate::share_dialog::ShareAction),
    /// アニメーションの書き出し（ネイティブ版のみ）
    MovieDialog(crate::movie_export::MovieDialogAction),
    #[cfg(not(target_arch = "wasm32"))]
    MovieFolderChosen(Result<Option<std::path::PathBuf>, String>),
    MovieRendererReady(Result<crate::movie_export::SharedRenderer, String>),
    /// 1 枚が描けた。メモリに溜める書き出し（ブラウザ版）では PNG のバイト列、フォルダへ書く書き出しでは `None`
    MovieFrameDone(Result<Option<Vec<u8>>, String>),
    #[cfg(not(target_arch = "wasm32"))]
    MovieEncoded(Result<Option<std::path::PathBuf>, String>),
    /// zip のダウンロード（ブラウザ版）が終わった。戻りは、ファイル名（取り消しなら `None`）
    #[cfg(target_arch = "wasm32")]
    MovieZipSaved(Result<Option<String>, String>),
    MovieCancel,
    ImageExported(Result<Option<(String, Option<std::path::PathBuf>)>, String>),
}
