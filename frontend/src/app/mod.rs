mod camera_anim_ops;
mod editor;
pub mod message;
mod movie_ops;
mod playback_ops;
mod selection_ops;
mod trajectory_ops;
mod update;
mod viewport_cameras;

pub use message::*;

use iced::widget::pane_grid::{self, PaneGrid};
use iced::widget::{column, container, mouse_area, text};
use iced::{Element, Fill, Task};
use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::context_menu::{self, ContextMenuTarget};
use crate::file_open_dispatch::FileOpenHandler;
use crate::log_viewer;
use crate::menu::{self, MenuTarget};
use crate::menu_bar::{self, MenuAction, MenuCategory};
use crate::named_selections::view_named_selections;
use crate::pane_view::{PaneViewState, ViewId, default_pane_views, view_tab_bar};
use crate::project_tree_pane;
use crate::representation_inspector::{
    RepContextMenuAction, RepContextTarget, view_rep_context_menu, view_representation_inspector,
};

use crate::state_sync;
use crate::streams::*;
use crate::structure_tree::{
    StructureTreeState, TreeContextMenuAction, TreeContextTarget, view_structure_tree,
    view_tree_context_menu,
};
use crate::terminal_pane;
use crate::text_editor_pane;
use crate::viewport;
use yui_core::state::AppState;

/// ドロップダウンのカテゴリに対応する、メニューの場所
/// 再生のキーボード操作: Space = 再生 / 一時停止、← = 1 フレーム戻る、→ = 1 フレーム進む
fn playback_key_action(key: &iced::keyboard::Key) -> Option<crate::playback::PlaybackAction> {
    use crate::playback::PlaybackAction;
    use iced::keyboard::{Key, key::Named};
    match key.as_ref() {
        Key::Named(Named::Space) => Some(PlaybackAction::PlayPause),
        Key::Named(Named::ArrowLeft) => Some(PlaybackAction::Prev),
        Key::Named(Named::ArrowRight) => Some(PlaybackAction::Next),
        _ => None,
    }
}

fn menu_location(category: MenuCategory) -> &'static str {
    match category {
        MenuCategory::File => menu::location::FILE,
        MenuCategory::View => menu::location::VIEW,
        MenuCategory::Help => menu::location::HELP,
    }
}

pub struct Yui {
    pub(super) panes: pane_grid::State<PaneKind>,
    pub(super) pane_views: crate::pane_view::PaneLayout,
    /// ドラッグ中のタブ（押したペインとビュー）。ボタンを離すと `None` に戻る。
    pub(super) tab_drag: Option<(PaneKind, ViewId)>,
    /// タブを押した位置（ウィンドウの座標）。ドラッグ中の四角形を出すかの判定に使う。
    pub(super) tab_drag_start: [f32; 2],
    /// ドラッグ中のタブを落とす先（マウスが本体の上にある、エディタのグループと、その中の場所）
    pub(super) tab_drop_target: Option<(PaneKind, crate::pane_view::DropZone)>,
    pub(super) state: AppState,
    pub(super) collab_url: Option<String>,
    pub(super) collab_connected: bool,
    pub(super) rest_error: Option<String>,
    pub(super) log_viewer: log_viewer::LogViewerState,
    pub(super) terminal_pane: terminal_pane::TerminalPaneState,
    /// 開いているテキストファイル（保存しない、利用者ごとのもの）
    pub(super) documents: text_editor_pane::DocumentStore,
    /// 未保存のため、閉じる確認のダイアログを出しているドキュメント
    pub(super) pending_close: Option<text_editor_pane::DocumentId>,
    /// 「保存して閉じる」の保存が終わるのを待っているドキュメント
    pub(super) close_after_save: Option<text_editor_pane::DocumentId>,
    /// 終了するときの、未保存のファイルの確認のダイアログを出しているか
    pub(super) quit_dialog: bool,
    /// 「Save All」で保存して終了する途中の、これから保存するドキュメント（先頭が保存中のもの）
    pub(super) quit_save_queue: Option<Vec<text_editor_pane::DocumentId>>,
    pub(super) project_tree: project_tree_pane::ProjectTreePaneState,

    // メニューバー & 表示状態（第8章 8.2節）
    pub(super) active_menu: Option<MenuCategory>,
    pub(super) show_left: bool,
    /// タブのなくなったドックを隠すか（設定 `ui.hide_empty_docks`。起動時に読む）
    pub(super) hide_empty_docks: bool,
    pub(super) show_right: bool,
    pub(super) show_bottom: bool,
    pub(super) show_about: bool,
    /// プラグインの一覧を開いているか（ネイティブ版だけ使う）
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) show_plugins: bool,
    /// 一覧の行（開いたとき、切り替えたときに作り直す）
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) plugin_rows: Vec<crate::plugin_list::Row>,
    /// 一覧の下に出す案内（保存できたか、できなかったか）
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) plugin_notice: Option<String>,
    pub(super) show_controls: bool,
    pub(super) camera_fit_token: u64,
    /// 構造に合わせる視点を、まだアプリへ知らせていない分子図
    pub(super) fit_pending: std::collections::BTreeSet<viewport::ViewportId>,
    /// 2つ目の分子図の視点（その画面だけのもの。保存・協調編集の対象にしない）。
    /// `None` は、最初に表示するときに1つ目の視点をコピーする。
    pub(super) secondary_camera: Option<yui_core::state::CameraState>,
    /// 2つの分子図の視点の連動
    pub(super) camera_link: bool,
    /// UI のテーマ（利用者ごとの設定。ワークスペースには入れない）
    pub(super) theme: crate::ui_style::ThemeKind,
    /// 今選ばれている原子（利用者ごとのもの。ワークスペースにも協調編集にも入れない）
    pub(super) selection: std::collections::BTreeSet<u32>,
    /// 再生の状態（利用者ごとのもの。ワークスペースにも協調編集にも入れない）
    pub(super) playback: crate::playback::Playback,
    /// 開いている軌跡（利用者ごとのもの。ワークスペースにも協調編集にも入れない）
    pub(super) trajectory: Option<crate::trajectory::LoadedTrajectory>,
    /// 開いている CUBE の等値面（File → Open CUBE）。構造とは独立で、ワークスペースには保存しない
    pub(super) cube: Option<crate::cube_view::CubeState>,
    pub(super) cube_entries: Vec<crate::surface_render::SurfaceMeshEntry>,
    /// 動いているカメラのアニメーション（分子図ごと。利用者ごとのもので、保存も協調編集もしない）
    pub(super) camera_anims: BTreeMap<viewport::ViewportId, crate::camera_anim::CameraAnimation>,
    /// アニメーションを前回進めた時刻
    pub(super) camera_anim_clock: Option<iced::time::Instant>,
    /// 自動回転の速さ（度 / 秒）
    pub(super) spin_speed: f32,
    /// 構造に合わせた結果が届いたとき、すぐ切り替えずに滑らかに動く分子図
    pub(super) animate_fit: std::collections::BTreeSet<viewport::ViewportId>,
    /// 合わせる対象が、構造全体ではなく選択か（Zoom to Selection）
    pub(super) fit_selection: bool,
    /// 最後に視点を動かした分子図（メニューの Spin / Rock の対象）
    pub(super) active_viewport: viewport::ViewportId,
    /// 選択の強調の方法（利用者ごとの設定）
    pub(super) highlight: crate::selection::HighlightStyle,
    /// コマンドの登録簿（メニューの項目が呼ぶコマンドを、既存の操作に引き当てる）
    pub(super) commands: menu::CommandRegistry,
    /// メニューの場所ごとの項目の受け口（組み込みのメニューもここに登録する）
    pub(super) menus: menu::MenuRegistry,
    pub(super) status_message: Option<crate::status_bar::StatusMessage>,

    // ビューポートコンテキストメニュー（第8章 8.3節）
    pub(super) context_menu: Option<([f32; 2], ContextMenuTarget)>,

    // 構造ツリー & コンテキストメニュー（第8章 8.3節・8.4節）
    pub(super) structure_tree: StructureTreeState,
    pub(super) tree_context_menu: Option<([f32; 2], TreeContextTarget)>,
    pub(super) cursor_position: [f32; 2],
    /// ウィンドウの大きさ（メニューをウィンドウに収めるために使う。起動時の大きさと、変化を受け取って更新する）
    pub(super) window_size: iced::Size,
    /// 3D ペインが最後に描かれた大きさ。書き出しの `3D pane` の縦横比に使う
    pub(super) viewport_sizes: viewport::ViewportSizes,

    // Representation インスペクター & コンテキストメニュー（第8章 8.3節・8.4節）
    pub(super) rep_context_menu: Option<([f32; 2], RepContextTarget)>,

    /// プロジェクトツリーのファイルの右クリックメニュー（Open with）。位置はマウスのそば
    pub(super) file_context_menu: Option<([f32; 2], crate::file_open_dispatch::OpenWithTarget)>,

    // 名前付き選択のリネーム中ドラフト（(元のキー, 編集中テキスト)）
    pub(super) renaming_selection: Option<(String, String)>,

    /// 複数の開き方候補があるファイルについて、ユーザーの選択待ちの状態
    /// (相対パス, 候補ハンドラ一覧)
    pub(super) pending_open_choice: Option<(String, Vec<FileOpenHandler>)>,
    /// PNG の書き出しのダイアログ（開いているときは `Some`）と、前回選んだ設定
    pub(super) png_dialog: bool,
    /// 「Share Session...」ダイアログ（開いているときだけ）
    pub(super) share_dialog: Option<crate::share_dialog::ShareState>,
    /// SVG の書き出しのダイアログと、前回の設定
    pub(super) svg_dialog: bool,
    pub(super) svg_options: crate::svg_export::SvgOptions,
    /// アニメーションの書き出しのダイアログと、前回の設定、進行中の書き出し（ネイティブ版のみ）
    pub(super) movie_dialog: bool,
    pub(super) movie_options: crate::movie_export::MovieOptions,
    pub(super) movie: Option<movie_ops::MovieJob>,
    pub(super) png_options: crate::png_export::PngOptions,

    /// 描画用解決済み相互作用キャッシュ（端点座標・端点原子ID）
    pub(super) resolved_interactions: Vec<yui_core::interaction::ResolvedInteraction>,

    /// 各モデルに属する原子ID範囲キャッシュ（モデルキー・開始ID・終了ID）
    pub(super) model_atom_ranges: Vec<yui_core::atom_group::ModelAtomRange>,

    /// 表面描画用のRepresentation別キャッシュエントリ
    pub(super) surface_entries: Vec<crate::surface_render::SurfaceMeshEntry>,
    /// GPU描画用に不透明／半透明に集約された表面バッチ
    pub(super) surface_batches: crate::surface_render::SurfaceBatches,
    /// 表面生成の世代追跡器
    pub(super) surface_generation_tracker: crate::surface_render::SurfaceGenerationTracker,
    /// 分子表面のメッシュを生成中か（ステータスバーに表示する）
    pub(super) surface_generating: bool,
    /// 再生中に、生成が終わる前に次のフレームへ進んだか（終わったら、最新のモデルでもう一度作る）
    pub(super) surface_follow_pending: bool,

    /// 現在開いている・保存したワークスペースファイルのパス（ネイティブのみ有効）
    pub(super) current_workspace_path: Option<PathBuf>,
    /// 現在開いている・保存したワークスペースファイル名
    pub(super) current_workspace_file_name: Option<String>,
}

impl Default for Yui {
    fn default() -> Self {
        // コラボレーションURLの決定（ネイティブ: CLI引数/環境変数、WASM: URLクエリ/ホスト名）
        #[cfg(not(target_arch = "wasm32"))]
        let collab_url = {
            let mut url = std::env::var("YUI_COLLAB_URL").ok();
            let args: Vec<String> = std::env::args().collect();
            for i in 0..args.len() {
                if args[i] == "--collab" && i + 1 < args.len() {
                    url = Some(args[i + 1].clone());
                }
            }
            url
        };
        #[cfg(target_arch = "wasm32")]
        let collab_url = {
            let window = web_sys::window();
            let (search, host, secure) = if let Some(ref w) = window {
                let location = w.location();
                (
                    location.search().unwrap_or_default(),
                    location.host().unwrap_or_default(),
                    location.protocol().unwrap_or_default() == "https:",
                )
            } else {
                (String::new(), String::new(), false)
            };
            Some(state_sync::resolve_wasm_collab_url(&search, &host, secure))
        };

        // 初期ペイン構成（Left / Center / Right / Bottom 全表示）
        let config =
            menu_bar::build_pane_configuration(true, true, true, default_pane_views().editor());

        // 起動時は File → New と同じ空の状態（構造も表示設定もない）で始める。
        // 構造を読み込むと、既定の表示設定が追加される（`load_structure_into_state`）。
        let initial_state = menu_bar::create_empty_state();

        let resolved_interactions = match initial_state.atom_group.as_ref() {
            Some(ag) => {
                yui_core::interaction::resolve_all_interactions(ag, &initial_state.interactions)
            }
            None => Vec::new(),
        };

        let model_atom_ranges = match initial_state.atom_group.as_ref() {
            Some(ag) => yui_core::calculate_model_atom_ranges(ag),
            None => Vec::new(),
        };

        let mut commands = menu::CommandRegistry::new();
        menu::register_menu_commands(&mut commands);
        let mut menus = menu::MenuRegistry::new();
        menu::register_builtin_menus(&mut menus);
        #[cfg(not(target_arch = "wasm32"))]
        crate::plugin_menu::register_loaded(&mut commands, &mut menus);
        #[cfg(not(target_arch = "wasm32"))]
        crate::plugin_list::register(&mut commands, &mut menus);

        Self {
            panes: pane_grid::State::with_configuration(config),
            pane_views: default_pane_views(),
            tab_drag: None,
            tab_drag_start: [0.0, 0.0],
            tab_drop_target: None,
            state: initial_state,
            collab_url,
            collab_connected: false,
            rest_error: None,
            log_viewer: log_viewer::LogViewerState::new(),
            terminal_pane: terminal_pane::TerminalPaneState::new(),
            documents: text_editor_pane::DocumentStore::default(),
            pending_close: None,
            close_after_save: None,
            quit_dialog: false,
            quit_save_queue: None,
            project_tree: project_tree_pane::ProjectTreePaneState::default(),
            active_menu: None,
            show_left: true,
            hide_empty_docks: crate::ui_style::ui_settings().hide_empty_docks,
            show_right: true,
            show_bottom: true,
            show_about: false,
            #[cfg(not(target_arch = "wasm32"))]
            show_plugins: false,
            #[cfg(not(target_arch = "wasm32"))]
            plugin_rows: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            plugin_notice: None,
            show_controls: false,
            camera_fit_token: 0,
            fit_pending: std::collections::BTreeSet::new(),
            secondary_camera: None,
            camera_link: false,
            theme: crate::user_settings::startup_theme(),
            selection: std::collections::BTreeSet::new(),
            playback: crate::playback::Playback::default(),
            trajectory: None,
            cube: None,
            cube_entries: Vec::new(),
            camera_anims: BTreeMap::new(),
            camera_anim_clock: None,
            spin_speed: crate::camera_anim::DEFAULT_SPIN_SPEED,
            animate_fit: std::collections::BTreeSet::new(),
            fit_selection: false,
            active_viewport: viewport::ViewportId::Primary,
            highlight: crate::user_settings::startup_highlight(),
            commands,
            menus,
            // 設定ファイルに問題があれば、起動時にステータスバーへ警告を出す
            status_message: startup_settings_message(),
            context_menu: None,
            structure_tree: StructureTreeState::new(),
            tree_context_menu: None,
            cursor_position: [240.0, 150.0],
            // iced の既定のウィンドウの大きさ。起動直後に実際の大きさに更新する
            window_size: iced::Size::new(1024.0, 768.0),
            viewport_sizes: Default::default(),
            rep_context_menu: None,
            file_context_menu: None,
            renaming_selection: None,
            pending_open_choice: None,
            png_dialog: false,
            share_dialog: None,
            svg_dialog: false,
            svg_options: crate::svg_export::SvgOptions::default(),
            movie_dialog: false,
            movie_options: crate::movie_export::MovieOptions::default(),
            movie: None,
            png_options: crate::png_export::PngOptions::default(),
            resolved_interactions,
            model_atom_ranges,
            surface_entries: Vec::new(),
            surface_batches: crate::surface_render::SurfaceBatches::default(),
            surface_generation_tracker: crate::surface_render::SurfaceGenerationTracker::new(),
            surface_generating: false,
            surface_follow_pending: false,
            current_workspace_path: None,
            current_workspace_file_name: None,
        }
    }
}

async fn fetch_initial_state(http_url: String) -> Result<AppState, String> {
    let resp = crate::backend_auth::get(&http_url)
        .await
        .map_err(|e| format!("HTTP request failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("HTTP status error: {}", resp.status()));
    }

    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("Failed to read response body: {e}"))?;

    let (_version, _compression, state) = yui_core::packing::unpack_app_state(&bytes)
        .map_err(|e| format!("Failed to decode AppState from MsgPack: {e}"))?;

    Ok(state)
}

impl Yui {
    pub fn new() -> (Self, Task<Message>) {
        let mut app = Self::default();
        let (tree_state, tree_task) =
            project_tree_pane::ProjectTreePaneState::new(app.collab_url.clone());
        app.project_tree = tree_state;

        let mut tasks = Vec::new();
        if let Some(ref url) = app.collab_url {
            let http_url = state_sync::derive_http_state_url(url);
            #[cfg(target_arch = "wasm32")]
            web_sys::console::log_1(
                &format!("[YUI WASM] Connecting to backend at {url} (fetching {http_url})").into(),
            );
            tasks.push(Task::perform(
                fetch_initial_state(http_url),
                Message::InitialStateFetched,
            ));
        }
        tasks.push(tree_task.map(Message::ProjectTreePaneMsg));
        tasks.push(app.trigger_surface_rebuild());
        // 起動時のウィンドウの大きさ（以後の変化は subscription の Resized で受け取る）
        tasks.push(
            iced::window::latest()
                .and_then(iced::window::size)
                .map(Message::WindowResized),
        );

        (app, Task::batch(tasks))
    }

    /// `atom_group` や `interactions` が更新された際に、描画用相互作用キャッシュを同期する
    pub(super) fn update_resolved_interactions(&mut self) {
        self.resolved_interactions = match self.state.atom_group.as_ref() {
            Some(ag) => {
                yui_core::interaction::resolve_all_interactions(ag, &self.state.interactions)
            }
            None => Vec::new(),
        };
    }

    /// `atom_group` が更新された際に、モデルごとの原子ID範囲キャッシュを同期する
    pub(super) fn update_model_atom_ranges(&mut self) {
        // 構造が入れ替わるときに呼ばれる。フレームの出どころが変わるので、再生は止め、開いていた軌跡は閉じる
        // （構造の座標は、入れ替わるので戻さない）。
        self.trajectory = None;
        self.reset_playback();
        self.model_atom_ranges = match self.state.atom_group.as_ref() {
            Some(ag) => yui_core::calculate_model_atom_ranges(ag),
            None => Vec::new(),
        };
    }

    /// 表面メッシュの再生成をトリガーする。
    ///
    /// 構造（`atom_group`）、表示モデル（`active_model`）、
    /// または Surface 表現の選択・可視性・パラメータが変更された場合に呼び出す。
    /// UI をブロックしないよう、ネイティブ版ではバックグラウンドスレッドで生成し、
    /// WASM版では同期的（単一スレッド環境）に生成する。
    pub(super) fn trigger_surface_rebuild(&mut self) -> Task<Message> {
        // この作り直しは今の状態から作るので、待たせていた分は不要になる
        self.surface_follow_pending = false;
        let surface_reps: Vec<_> = self
            .state
            .representations
            .iter()
            .filter(|r| r.kind == yui_core::representation::RepresentationKind::Surface)
            .cloned()
            .collect();

        if surface_reps.is_empty() || self.state.atom_group.is_none() {
            self.surface_entries.clear();
            self.refresh_surface_batches();
            // 生成中の古い世代の結果が後から届いても反映しないよう、世代を進めておく
            self.surface_generation_tracker.next_generation();
            self.surface_generating = false;
            return Task::none();
        }

        let requests = self.surface_build_requests();

        let generation = self.surface_generation_tracker.next_generation();
        self.surface_generating = true;

        #[cfg(not(target_arch = "wasm32"))]
        {
            Task::future(async move {
                let results = tokio::task::spawn_blocking(move || {
                    crate::surface_render::process_surface_build_requests(&requests)
                })
                .await
                .unwrap_or_default();

                Message::SurfaceGenerated {
                    generation,
                    results,
                }
            })
        }

        #[cfg(target_arch = "wasm32")]
        {
            let results = crate::surface_render::process_surface_build_requests(&requests);
            Task::done(Message::SurfaceGenerated {
                generation,
                results,
            })
        }
    }

    /// 今の状態（表示中のモデル、原子の座標）から、分子表面の表現ごとの生成の依頼を作る。
    /// 分子表面の表現がない、または構造がなければ空。
    pub(super) fn surface_build_requests(&self) -> Vec<crate::surface_render::SurfaceBuildRequest> {
        let Some(ag) = self.state.atom_group.as_ref() else {
            return Vec::new();
        };
        let all_atoms = &self.state.molecule.atoms;
        let active_model = self.state.active_model.as_deref();
        self.state
            .representations
            .iter()
            .filter(|r| r.kind == yui_core::representation::RepresentationKind::Surface)
            .map(|rep| {
                let selected_ids = rep.selection.select_atom_ids(ag);
                let filtered_atoms = crate::surface_render::filter_surface_atoms(
                    all_atoms,
                    &selected_ids,
                    active_model,
                    &self.model_atom_ranges,
                );
                crate::surface_render::SurfaceBuildRequest {
                    rep_id: rep.id.clone(),
                    atoms: filtered_atoms.iter().map(|&a| a.into()).collect(),
                    params: {
                        let r = crate::user_settings::startup_render();
                        yui_core::surface::SurfaceParams {
                            grid_spacing: r.surface_grid_spacing,
                            probe_radius: r.surface_probe_radius,
                            ..Default::default()
                        }
                    },
                    color_scheme: rep.color.clone(),
                    opacity: rep.opacity,
                    visible: rep.visible,
                }
            })
            .collect()
    }

    /// 再生中（とつまみを動かしている間）に、表示中のモデルへ分子表面を追従させる。
    ///
    /// 生成は、すべてのフレームについては行わない。生成中なら印だけ付けて、終わったときに最新のモデルで
    /// 作り直す（途中のフレームは飛ばす）。分子表面は、生成にかかる時間のぶんだけ遅れて動く。
    /// ブラウザ版は生成が同期的で UI を止めるので、再生中は分子表面を描かない（`Playback::hides_surface`）。
    pub(super) fn follow_surface(&mut self) -> Task<Message> {
        if cfg!(target_arch = "wasm32") {
            return Task::none();
        }
        if self.surface_generating {
            self.surface_follow_pending = true;
            return Task::none();
        }
        self.trigger_surface_rebuild()
    }

    /// 既存の表面メッシュの頂点色（および不透明／半透明バッチ）を再計算する。
    ///
    /// 色（`ColorScheme`）や透明度（`opacity`）、可視性（`visible`）のみが変更された場合に呼び出す。
    /// メッシュ（ジオメトリ）の再生成は行わず、頂点カラーの更新のみを純粋関数で行う。
    pub(super) fn update_surface_colors(&mut self) {
        if self.surface_entries.is_empty() {
            self.refresh_surface_batches();
            return;
        }

        let atom_map: std::collections::HashMap<u32, &yui_core::state::Atom> = self
            .state
            .molecule
            .atoms
            .iter()
            .map(|a| (a.id, a))
            .collect();

        let mut unique_chains: Vec<&str> = Vec::new();
        for a in &self.state.molecule.atoms {
            if !unique_chains.contains(&a.chain_id.as_str()) {
                unique_chains.push(&a.chain_id);
            }
        }

        let mut unique_residues: Vec<(&str, i32)> = Vec::new();
        for a in &self.state.molecule.atoms {
            let key = (a.chain_id.as_str(), a.residue_index);
            if !unique_residues.contains(&key) {
                unique_residues.push(key);
            }
        }

        let atom_sec_map = match self.state.atom_group.as_ref() {
            Some(ag) => crate::surface_render::build_atom_secondary_structure_map(
                ag,
                &self.state.secondary_structure_map,
            ),
            None => std::collections::HashMap::new(),
        };

        for entry in &mut self.surface_entries {
            if let Some(rep) = self
                .state
                .representations
                .iter()
                .find(|r| r.id == entry.rep_id)
            {
                entry.color_scheme = rep.color.clone();
                entry.opacity = rep.opacity;
                entry.visible = rep.visible;
            }

            let colors = crate::surface_render::resolve_surface_vertex_colors(
                &entry.mesh.vertex_atom_ids,
                &entry.color_scheme,
                entry.opacity,
                &atom_map,
                &unique_chains,
                &unique_residues,
                &atom_sec_map,
            );

            entry.vertices = crate::surface_render::build_surface_vertices(&entry.mesh, &colors);
            entry.indices = entry.mesh.indices.clone();
        }

        self.refresh_surface_batches();
    }

    /// 描画用のバッチを、分子表面と CUBE の等値面から作り直す
    pub(super) fn refresh_surface_batches(&mut self) {
        self.surface_batches = crate::surface_render::SurfaceBatches::build_from_groups(&[
            &self.surface_entries,
            &self.cube_entries,
        ]);
    }

    /// CUBE を開いた結果を受け取り、最初のデータセットの等値面の計算を始める
    pub(super) fn handle_cube_opened(
        &mut self,
        result: Result<Option<crate::cube_view::CubeOpenResult>, String>,
    ) -> Task<Message> {
        match result {
            Ok(Some((name, set, path))) => {
                // 構造を開いていなければ、CUBE の原子を構造として見せる
                let mut tasks = Vec::new();
                if !has_structure(&self.state) {
                    match yui_core::structure_loader::load_cube_atoms_into_state(
                        &mut self.state,
                        &set,
                    ) {
                        Ok(true) => {
                            self.update_resolved_interactions();
                            self.update_model_atom_ranges();
                            self.selection.clear();
                            self.request_camera_fit();
                            tasks.push(self.trigger_surface_rebuild());
                        }
                        Ok(false) => {}
                        Err(e) => {
                            self.status_message =
                                Some(crate::status_bar::StatusMessage::warning(e));
                        }
                    }
                }
                // ワークスペースには、ファイルのパス（ネイティブ版のみ）を保存する
                // 別の CUBE を開いたときは、見た目を既定に戻す。ワークスペースから同じファイルを
                // 開き直したとき（パスが同じ）は、保存されている見た目を使う。
                if path.is_none() || path != self.state.data_paths.cube_path {
                    self.state.cube_view = Default::default();
                }
                // 保存された設定が、このファイルにない軌道や、おかしな値を指していても、止まらない
                let view = &mut self.state.cube_view;
                view.dataset = view.dataset.min(set.num_datasets().saturating_sub(1));
                if !view.isolevel.is_finite() || view.isolevel < 0.0 {
                    view.isolevel = 0.0;
                }
                if !view.opacity.is_finite() {
                    view.opacity = yui_core::state::CubeView::default().opacity;
                }
                view.opacity = view.opacity.clamp(0.0, 1.0);
                self.state.data_paths.cube_path = path;
                self.cube = Some(crate::cube_view::CubeState::new(name, set));
                self.cube_entries.clear();
                self.refresh_surface_batches();
                tasks.push(self.start_cube_job());
                Task::batch(tasks)
            }
            Ok(None) => Task::none(),
            Err(e) => {
                self.status_message = Some(crate::status_bar::StatusMessage::error(format!(
                    "Error: {e}"
                )));
                Task::none()
            }
        }
    }

    /// カードの操作（軌道の選択、等値レベル、閉じる）
    pub(super) fn handle_cube_action(
        &mut self,
        action: crate::cube_view::CubeAction,
    ) -> Task<Message> {
        use crate::cube_view::CubeAction;
        match action {
            CubeAction::Close => {
                self.close_cube();
                Task::none()
            }
            CubeAction::SetDataset(index) => {
                let Some(cube) = self.cube.as_ref() else {
                    return Task::none();
                };
                let index = index.min(cube.set.num_datasets().saturating_sub(1));
                let view = &mut self.state.cube_view;
                if index != view.dataset {
                    view.dataset = index;
                    view.isolevel = 0.0; // 新しいデータセットの既定値
                }
                self.start_cube_job()
            }
            CubeAction::SetIsolevel(level) => {
                if self.cube.is_some() {
                    self.state.cube_view.isolevel = if level.is_finite() {
                        level.max(0.0)
                    } else {
                        0.0
                    };
                }
                self.start_cube_job()
            }
            CubeAction::SetPositiveColor(_)
            | CubeAction::SetNegativeColor(_)
            | CubeAction::SetOpacity(_) => {
                if self.cube.is_none() {
                    return Task::none();
                }
                let app = &mut self.state.cube_view;
                match action {
                    CubeAction::SetPositiveColor(i) => {
                        app.positive_color = i.min(crate::cube_view::COLOR_PRESETS.len() - 1)
                    }
                    CubeAction::SetNegativeColor(i) => {
                        app.negative_color = i.min(crate::cube_view::COLOR_PRESETS.len() - 1)
                    }
                    CubeAction::SetOpacity(v) => {
                        app.opacity = if v.is_finite() {
                            v.clamp(0.0, 1.0)
                        } else {
                            0.6
                        }
                    }
                    _ => {}
                }
                // 見た目だけの変更なので、計算はせずに、表示中の面を作り直す
                self.rebuild_cube_entries();
                Task::none()
            }
        }
    }

    /// 表示したい軌道・等値レベルになるよう、バックグラウンドで計算する。
    ///
    /// 計算中に操作が続いても、計算は重ねない。終わったときに、そのときの「表示したいもの」と違えば、
    /// もう一度計算する（スライダーを動かしている間のレベルは、飛ばして最新のものにする）。
    fn start_cube_job(&mut self) -> Task<Message> {
        let Some(cube) = self.cube.as_mut() else {
            return Task::none();
        };
        if cube.busy || !cube.needs_update(&self.state.cube_view) {
            return Task::none();
        }
        let job = cube.job(&self.state.cube_view);
        cube.busy = true;
        Task::perform(crate::cube_view::run_job_async(job), Message::CubeJobDone)
    }

    /// 等値面の計算が終わったとき
    pub(super) fn handle_cube_job_done(
        &mut self,
        result: Result<std::sync::Arc<crate::cube_view::CubeIsosurface>, String>,
    ) -> Task<Message> {
        let Some(cube) = self.cube.as_mut() else {
            return Task::none(); // 計算中に閉じられた
        };
        cube.busy = false;
        match result {
            Err(e) => {
                self.status_message = Some(crate::status_bar::StatusMessage::error(format!(
                    "Error: {e}"
                )));
                // 何も表示していなければ、開けなかったものとして閉じる
                if cube.shown.is_none() {
                    self.cube = None;
                }
                Task::none()
            }
            Ok(iso) => {
                if iso.dataset == self.state.cube_view.dataset {
                    let first = cube.shown.as_ref().is_none_or(|s| s.dataset != iso.dataset);
                    if first {
                        let what = match cube.set.orbital_number(iso.dataset) {
                            Some(o) => format!("orbital {o}"),
                            None => "the data".to_string(),
                        };
                        self.status_message = Some(
                            if iso.positive.indices.is_empty() && iso.negative.indices.is_empty() {
                                crate::status_bar::StatusMessage::warning(format!(
                                    "CUBE '{}' ({what}) has no surface at ±{:.4}; lower the isolevel",
                                    cube.file_name, iso.isolevel
                                ))
                            } else {
                                crate::status_bar::StatusMessage::info(format!(
                                    "Opened CUBE '{}' ({what}), isosurface at ±{:.4}",
                                    cube.file_name, iso.isolevel
                                ))
                            },
                        );
                    }
                    cube.shown = Some(iso);
                    self.rebuild_cube_entries();
                }
                self.start_cube_job()
            }
        }
    }

    /// 表示中の CUBE の等値面から、描画用のエントリを作り直す（見た目は `state.cube_view`）
    fn rebuild_cube_entries(&mut self) {
        if let Some(shown) = self.cube.as_ref().and_then(|c| c.shown.clone()) {
            self.cube_entries = crate::cube_view::to_entries(&shown, &self.state.cube_view);
            self.refresh_surface_batches();
        }
    }

    /// 開いている CUBE の等値面を閉じる
    pub(super) fn close_cube(&mut self) {
        if self.cube.take().is_some() {
            self.cube_entries.clear();
            self.state.data_paths.cube_path = None;
            self.state.cube_view = Default::default();
            self.refresh_surface_batches();
            self.status_message = Some(crate::status_bar::StatusMessage::info(
                "Closed the CUBE file".to_string(),
            ));
        }
    }

    /// ワークスペースの状態を入れ替えたあと（File → New、ワークスペースを開く）に呼ぶ。
    /// 開いていた CUBE は閉じ、新しい状態に CUBE のパスがあれば（ネイティブ版）、それを開く。
    pub(super) fn sync_cube_with_workspace(&mut self) -> Task<Message> {
        self.cube = None;
        self.cube_entries.clear();
        self.refresh_surface_batches();
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(path) = self.state.data_paths.cube_path.clone() {
            return Task::perform(crate::cube_view::open_cube_path(path), Message::CubeOpened);
        }
        self.state.data_paths.cube_path = None;
        Task::none()
    }

    /// 非同期生成タスクの結果を受け取り、世代照合の上で反映する
    pub(super) fn apply_surface_build_results(
        &mut self,
        generation: u64,
        results: Vec<crate::surface_render::SurfaceBuildResult>,
    ) {
        if !self.surface_generation_tracker.is_latest(generation) {
            // 古い世代の結果は破棄する
            return;
        }
        self.surface_generating = false;

        self.surface_entries = results
            .into_iter()
            .map(|res| crate::surface_render::SurfaceMeshEntry {
                rep_id: res.rep_id,
                mesh: res.mesh,
                color_scheme: res.color_scheme,
                opacity: res.opacity,
                visible: res.visible,
                vertices: Vec::new(),
                indices: Vec::new(),
            })
            .collect();

        self.update_surface_colors();
    }

    /// iced の標準のウィジェット（テキスト入力、スクロールバーなど）のテーマ
    pub fn theme(&self) -> iced::Theme {
        self.theme.iced_theme()
    }

    pub fn title(&self) -> String {
        if self.collab_connected {
            String::from("YUI [Online / Collab Connected]")
        } else if let Some(ref err) = self.rest_error {
            format!("YUI [Offline / REST Error: {err}]")
        } else if self.collab_url.is_some() {
            String::from("YUI [Connecting to Backend...]")
        } else {
            String::from("YUI [Local Standalone]")
        }
    }

    /// メニューを組み立てるための、アプリの状態（項目の「押せるか」「チェック状態」の元）
    fn menu_context(&self, target: Option<menu::MenuTarget>) -> menu::MenuContext {
        menu::MenuContext {
            show_left: self.show_left,
            show_right: self.show_right,
            show_bottom: self.show_bottom,
            editor_split: crate::pane_view::is_editor_split(&self.pane_views),
            camera_link: self.camera_link,
            theme: self.theme,
            highlight: self.highlight,
            has_selection: !self.selection.is_empty(),
            has_trajectory: self.trajectory.is_some(),
            has_cube: self.cube.is_some(),
            spinning: self.is_spinning(),
            rocking: self.is_rocking(),
            spin_speed: self.spin_speed,
            has_playback: self.has_playback(),
            playing: self.playback.playing,
            loop_mode: self.playback.loop_mode,
            // File → Export Structure を押せるかは、構造（atom_group）があるかで決める
            has_structure: self.state.atom_group.is_some(),
            target,
        }
    }

    /// タブやペインの見出しに表示する名前（ドキュメントはファイル名。未保存なら `*` 付き）
    fn view_label(&self, view: ViewId) -> String {
        match view {
            ViewId::Document(id) => self
                .documents
                .get(id)
                .map(|d| d.tab_label())
                .unwrap_or_else(|| "(closed)".to_string()),
            other => other.label().to_string(),
        }
    }

    /// 次の描画で、カメラを表示中の構造に合わせる（構造がなければ既定のカメラにする）。
    ///
    /// 構造が変わったとき（構造を開く、ワークスペースを開く、File → New、backend からの初期状態）と
    /// Reset Camera で呼ぶ。表示中のモデルを切り替えたときは呼ばない（視点が飛ばないように）。
    pub(super) fn request_camera_fit(&mut self) {
        // 構造に合わせるので、動いているアニメーションは止める（滑らかに動くときは、呼び出し側があとで指定する）
        self.stop_all_camera_animations();
        self.fit_selection = false;
        self.camera_fit_token = self.camera_fit_token.wrapping_add(1);
        self.fit_pending.insert(viewport::ViewportId::Primary);
        if crate::pane_view::has_second_viewport(&self.pane_views) {
            self.fit_pending.insert(viewport::ViewportId::Secondary);
        } else {
            // 2つ目の分子図は表示していない。次に表示するとき、1つ目の視点をコピーする。
            self.fit_pending.remove(&viewport::ViewportId::Secondary);
            self.secondary_camera = None;
        }
    }

    /// 表示・非表示の設定と、テキストエディタの分割の有無から、ペインの配置を作り直す
    pub(super) fn rebuild_pane_grid(&mut self) {
        let config = menu_bar::build_pane_configuration(
            self.show_left,
            self.show_right,
            self.show_bottom,
            self.pane_views.editor(),
        );
        self.panes = pane_grid::State::with_configuration(config);
    }

    /// 今の `pane_grid` の分割の比率（利用者が境界をドラッグした大きさ）
    fn current_split_ratios(&self) -> Vec<menu_bar::SplitRatio> {
        menu_bar::grid_split_ratios(self.panes.layout(), &|pane| self.panes.get(pane).copied())
    }

    /// エディタのグループの木を変える前に呼ぶ。今の大きさを木へ写し、変える前の木を返す（`end_layout_change` に渡す）。
    pub(super) fn begin_layout_change(&mut self) -> crate::pane_view::GroupNode {
        self.sync_editor_ratios();
        self.pane_views.editor().clone()
    }

    /// 利用者がドラッグした、エディタのグループの境界の割合を、木へ写す（作り直しと、ワークスペースの保存の前）
    pub(super) fn sync_editor_ratios(&mut self) {
        let ratios = self.current_split_ratios();
        self.pane_views
            .sync_ratios(&|a, b| menu_bar::find_split_ratio(&ratios, a, b));
    }

    /// エディタのグループの木を変えたあとに呼ぶ。空のグループを取り除き、木が変わっていれば、ペインを作り直す。
    /// 作り直しても、変わっていない分割（ドックの境界を含む）の大きさは保つ。
    ///
    /// 設定 `ui.hide_empty_docks` がオンなら、タブのなくなったドック（左・右・下）を隠す。
    pub(super) fn end_layout_change(&mut self, before: crate::pane_view::GroupNode) {
        self.pane_views.prune_empty_groups();
        let hid_docks = self.hide_empty_docks && self.hide_empty_dock_panes();
        if *self.pane_views.editor() == before && !hid_docks {
            return;
        }
        let ratios = self.current_split_ratios();
        let config = menu_bar::build_pane_configuration(
            self.show_left,
            self.show_right,
            self.show_bottom,
            self.pane_views.editor(),
        );
        self.panes =
            pane_grid::State::with_configuration(menu_bar::with_split_ratios(config, &ratios));
    }

    /// タブのなくなった、表示中のドックを隠す（View メニューの印も外れる）。隠したら `true`。
    fn hide_empty_dock_panes(&mut self) -> bool {
        let mut hid = false;
        for (kind, shown) in [
            (PaneKind::Left, &mut self.show_left),
            (PaneKind::Right, &mut self.show_right),
            (PaneKind::Bottom, &mut self.show_bottom),
        ] {
            if *shown
                && self
                    .pane_views
                    .get(&kind)
                    .is_some_and(|s| s.views.is_empty())
            {
                *shown = false;
                hid = true;
            }
        }
        hid
    }

    /// タブのドラッグを終える。グループの本体の中央に落としたら、そのグループの末尾へ移し、端に落としたら、
    /// その向きに分割して新しいグループへ移す。タブのなくなったグループは消す。
    pub(super) fn end_tab_drag(&mut self) {
        let target = self.tab_drop_target.take();
        let Some((from, view)) = self.tab_drag.take() else {
            return;
        };
        // ドラッグ中に、タブの帯でほかのグループへ移したときも、元のグループが空になっていれば、ここで消す
        let before = self.begin_layout_change();
        match target {
            Some((pane, crate::pane_view::DropZone::Center)) => {
                self.pane_views.move_view_between(view, from, pane, None);
            }
            Some((pane, zone)) => {
                self.pane_views.split_with_view(view, from, pane, zone);
            }
            None => {}
        }
        self.end_layout_change(before);
    }

    /// ドラッグ中のタブを、このグループのこの場所に落としたら、配置が変わるか（落とす先を色で示すかどうか）
    fn drop_changes_layout(&self, pane: PaneKind, zone: crate::pane_view::DropZone) -> bool {
        let Some((from, _)) = self.tab_drag else {
            return false;
        };
        if zone == crate::pane_view::DropZone::Center {
            from != pane
        } else {
            // 端に落として分割するのは、エディタのグループだけ
            matches!(pane, PaneKind::Group(_))
                && (from != pane
                    || self
                        .pane_views
                        .get(&from)
                        .is_some_and(|s| s.views.len() > 1))
        }
    }

    /// ウィンドウ最下部のステータスバー
    fn view_status_bar(&self) -> Element<'_, Message> {
        use crate::status_bar::{
            ConnectionStatus, StatusBarInput, build_status_bar_fields, view_status_bar,
        };

        let connection = ConnectionStatus::from_flags(
            self.collab_connected,
            self.rest_error.as_deref(),
            self.collab_url.is_some(),
        );
        // backend のデモ構造のように、平坦化キャッシュだけが届いている場合も数を表示する
        let structure_counts = has_structure(&self.state).then_some((
            self.state.molecule.atoms.len(),
            self.state.molecule.bonds.len(),
        ));
        let model_keys: Vec<&str> = self
            .model_atom_ranges
            .iter()
            .map(|r| r.model_key.as_str())
            .collect();

        let trajectory_label = self.trajectory.as_ref().map(|t| {
            format!(
                "step {} ({}/{})",
                t.step,
                t.current + 1,
                t.traj.frame_count()
            )
        });
        let fields = build_status_bar_fields(&StatusBarInput {
            message: self.status_message.as_ref(),
            structure_counts,
            active_model: self.state.active_model.as_deref(),
            model_keys: &model_keys,
            surface_generating: self.surface_generating,
            playing: self.playback.playing,
            trajectory_label: trajectory_label.as_deref(),
            connection: &connection,
        });
        view_status_bar(fields)
    }

    pub fn subscription(&self) -> iced::Subscription<Message> {
        let mut subs = Vec::new();
        if let Some(ref url) = self.collab_url {
            subs.push(collab_subscription(url.clone()));
        }
        if let Some(ref log_path) = self.log_viewer.active_log_path {
            let base_url = self.collab_url.as_deref().unwrap_or("ws://127.0.0.1:8080");
            let logs_url = state_sync::derive_ws_logs_url(base_url, log_path);
            subs.push(log_stream_subscription(logs_url).map(Message::LogViewerMsg));
        }
        if self.terminal_pane.connect_requested {
            #[cfg(not(target_arch = "wasm32"))]
            if self.terminal_pane.local {
                subs.push(
                    local_terminal_subscription(
                        self.terminal_pane.connect_rows,
                        self.terminal_pane.connect_cols,
                    )
                    .map(Message::TerminalPaneMsg),
                );
            }
            if !self.terminal_pane.local {
                let base_url = self.collab_url.as_deref().unwrap_or("ws://127.0.0.1:8080");
                let terminal_url = state_sync::derive_ws_terminal_url(
                    base_url,
                    self.terminal_pane.connect_rows,
                    self.terminal_pane.connect_cols,
                );
                subs.push(terminal_stream_subscription(terminal_url).map(Message::TerminalPaneMsg));
            }
        }
        // カメラのアニメーションが動いている間だけ、タイマーを購読する（約 60 回 / 秒）
        if !self.camera_anims.is_empty() {
            subs.push(
                iced::time::every(std::time::Duration::from_millis(16))
                    .map(|_| Message::CameraAnimTick),
            );
        }
        // 再生中だけ、タイマーを購読する（止まっているときは、タイマーも再描画もない）
        if let Some(interval) = self.playback.tick_interval() {
            subs.push(iced::time::every(interval).map(|_| Message::PlaybackTick));
        }
        // ウィンドウを閉じる操作（exit_on_close_request(false) にしているので、ここで受けて確認する）
        subs.push(iced::window::close_requests().map(|_| Message::QuitRequested));
        subs.push(iced::event::listen_with(
            |event, _status, _window| match event {
                iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                    Some(Message::CursorMoved([position.x, position.y]))
                }
                iced::Event::Window(iced::window::Event::Resized(size)) => {
                    Some(Message::WindowResized(size))
                }
                iced::Event::Window(iced::window::Event::Opened { size, .. }) => {
                    Some(Message::WindowResized(size))
                }
                _ => None,
            },
        ));
        if self.terminal_has_keyboard() {
            // ターミナルの画面にキー入力を送っている間は、キーはすべて端末へ送る（Esc・Space・矢印・Ctrl+S などの
            // アプリのショートカットは、その間は働かない）。ほかをクリックすると、元に戻る。
            subs.push(iced::event::listen_with(
                |event, status, _window| match event {
                    iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                        key,
                        modifiers,
                        text,
                        ..
                    }) if status == iced::event::Status::Ignored => Some(Message::TerminalKey {
                        key,
                        modifiers,
                        text: text.map(|t| t.to_string()),
                    }),
                    _ => None,
                },
            ));
        } else {
            subs.push(iced::event::listen_with(|event, status, _window| {
                match event {
                    // Esc: 選択を外す（エディタなどが Esc を使ったときは、何もしない）
                    iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                        key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                        ..
                    }) if status == iced::event::Status::Ignored => Some(Message::ClearSelection),
                    // Space: 再生 / 一時停止、← →: 1 フレームずつ（文字を入力しているところは、先に処理して Captured にする）
                    iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                        key,
                        modifiers,
                        ..
                    }) if status == iced::event::Status::Ignored
                        && modifiers.is_empty()
                        && playback_key_action(&key).is_some() =>
                    {
                        playback_key_action(&key).map(Message::PlaybackControl)
                    }
                    // Ctrl+S（macOS は Cmd+S）。エディタにフォーカスがあるときは、エディタ側が先に処理する。
                    iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                        key,
                        modifiers,
                        ..
                    }) if status == iced::event::Status::Ignored
                        && text_editor_pane::is_save_shortcut(&key, modifiers) =>
                    {
                        Some(Message::SaveActiveDocument)
                    }
                    _ => None,
                }
            }));
        }
        // タブのドラッグ中は、どこでボタンを離しても終わるように、離したことを全体で受ける
        if self.tab_drag.is_some() {
            subs.push(iced::event::listen_with(
                |event, _status, _window| match event {
                    iced::Event::Mouse(
                        iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)
                        | iced::mouse::Event::CursorLeft,
                    )
                    | iced::Event::Touch(
                        iced::touch::Event::FingerLifted { .. }
                        | iced::touch::Event::FingerLost { .. },
                    ) => Some(Message::TabDragEnded),
                    _ => None,
                },
            ));
        }
        if self.terminal_pane.focused {
            subs.push(iced::event::listen_with(
                |event, _status, _window| match event {
                    iced::Event::Mouse(iced::mouse::Event::ButtonPressed(
                        iced::mouse::Button::Left,
                    )) => Some(Message::MousePressed),
                    _ => None,
                },
            ));
        }
        iced::Subscription::batch(subs)
    }

    /// ターミナルのコピー・貼り付け。コピーは選択した文字をクリップボードへ、貼り付けはクリップボードを読んで
    /// `TerminalPasteText` で受ける。
    pub(super) fn terminal_clipboard(
        &mut self,
        action: crate::terminal_pane::ClipboardKey,
    ) -> Task<Message> {
        use crate::terminal_pane::ClipboardKey;
        match action {
            ClipboardKey::Copy => match self.terminal_pane.selected_text() {
                Some(text) if !text.is_empty() => iced::clipboard::write(text),
                _ => Task::none(),
            },
            ClipboardKey::Paste => iced::clipboard::read().map(Message::TerminalPasteText),
        }
    }

    /// ターミナルの画面にキー入力を送っているか（接続中で、画面が見えているとき）
    fn terminal_has_keyboard(&self) -> bool {
        self.terminal_pane.focused
            && self.terminal_pane.connected
            && self.pane_views.iter().any(|(kind, state)| {
                state.active_view() == Some(ViewId::Terminal)
                    && match kind {
                        PaneKind::Left => self.show_left,
                        PaneKind::Right => self.show_right,
                        PaneKind::Bottom => self.show_bottom,
                        PaneKind::Group(_) => true,
                    }
            })
    }

    /// SVG の書き出しのダイアログの操作。「Export...」で、保存先を選んで書き出す。
    pub(super) fn handle_svg_dialog(
        &mut self,
        action: crate::svg_export::SvgDialogAction,
    ) -> Task<Message> {
        use crate::svg_export::SvgDialogAction;
        match action {
            SvgDialogAction::Cancel => {
                self.svg_dialog = false;
                Task::none()
            }
            SvgDialogAction::Export => {
                self.svg_dialog = false;
                let options = self.svg_options;
                // 画面と同じ原子・結合・相互作用で、指定の大きさと構図の SVG を作り、保存ダイアログで選んだ場所へ書く
                let svg = self.export_canvas(&options.canvas, options.width).and_then(
                    |(size, camera)| {
                        crate::svg_export::render_view_svg(
                            &self.state,
                            &self.model_atom_ranges,
                            &self.resolved_interactions,
                            // 再生中（ブラウザ版）は、画面と同じく、分子表面を入れない
                            if self.playback.hides_surface() {
                                crate::surface_render::SurfaceBatches::empty_ref()
                            } else {
                                &self.surface_batches
                            },
                            &camera,
                            size,
                        )
                    },
                );
                let svg = match svg {
                    Ok(svg) => svg,
                    Err(e) => {
                        self.status_message = Some(crate::status_bar::StatusMessage::error(
                            format!("Export failed: {e}"),
                        ));
                        return Task::none();
                    }
                };
                let suggested_name = self.current_workspace_file_name.as_ref().map(|name| {
                    let stem = std::path::Path::new(name)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("view");
                    format!("{stem}.svg")
                });
                Task::perform(
                    crate::svg_export::save_svg_file(suggested_name, svg),
                    Message::ImageExported,
                )
            }
            other => {
                self.svg_options.apply(other);
                Task::none()
            }
        }
    }

    /// PNG の書き出しのダイアログの操作。「Export...」で、保存先を選んで書き出す。
    /// `backend` の HTTP のベース URL（接続先が決まっていなければ、ローカルの既定）
    pub(super) fn backend_http_base(&self) -> String {
        state_sync::derive_http_base_url(
            self.collab_url.as_deref().unwrap_or("ws://127.0.0.1:8080"),
        )
    }

    /// 「Share Session...」ダイアログの操作。要求を出す操作は `Task` にし、結果は `Message::Share` で戻る。
    /// `open` が真なら、ダイアログを開く（メニューから）。
    pub(super) fn handle_share_action(
        &mut self,
        action: crate::share_dialog::ShareAction,
        open: bool,
    ) -> Task<Message> {
        use crate::share_dialog::{self as share, ShareAction as A};
        if open {
            self.share_dialog = Some(share::ShareState::default());
        }
        let Some(state) = self.share_dialog.as_mut() else {
            // 閉じたあとに、遅れて届いた結果は、捨てる
            return Task::none();
        };
        if matches!(action, A::Close) {
            self.share_dialog = None;
            return Task::none();
        }
        state.apply(&action);
        let base = self.backend_http_base();
        let state = self.share_dialog.as_ref().expect("checked above");
        match action {
            A::Create => Task::perform(
                share::create_invite(base, state.role, state.lifetime),
                |r| Message::Share(A::Created(r)),
            ),
            // 作れたら、一覧も新しくする
            A::Created(Ok(_)) | A::Revoked(Ok(_)) => {
                Task::perform(share::list_invites(base), |r| Message::Share(A::Listed(r)))
            }
            A::Refresh => {
                Task::perform(share::list_invites(base), |r| Message::Share(A::Listed(r)))
            }
            A::Revoke(id) => Task::perform(share::revoke_invite(base, id), |r| {
                Message::Share(A::Revoked(r))
            }),
            A::CopyLink => match &state.created {
                Some(c) => iced::clipboard::write(share::invite_link(&base, &c.token)),
                None => Task::none(),
            },
            A::CopyToken => match &state.created {
                Some(c) => iced::clipboard::write(c.token.clone()),
                None => Task::none(),
            },
            _ => Task::none(),
        }
    }

    pub(super) fn handle_png_dialog(
        &mut self,
        action: crate::png_export::PngDialogAction,
    ) -> Task<Message> {
        use crate::png_export::PngDialogAction;
        match action {
            PngDialogAction::Cancel => {
                self.png_dialog = false;
                Task::none()
            }
            PngDialogAction::Export => {
                self.png_dialog = false;
                let options = self.png_options;
                let ((width, height), camera) =
                    match self.export_canvas(&options.canvas, options.width) {
                        Ok(v) => v,
                        Err(e) => {
                            self.status_message = Some(crate::status_bar::StatusMessage::error(
                                format!("Export failed: {e}"),
                            ));
                            return Task::none();
                        }
                    };
                // 描くものの準備は、画面と同じ処理をここで行い、GPU での描画と保存は非同期で行う
                let primitive = self.offscreen_primitive_with(
                    width,
                    height,
                    options.transparent,
                    &camera,
                    &self.surface_batches,
                );
                let suggested_name = self.current_workspace_file_name.as_ref().map(|name| {
                    let stem = std::path::Path::new(name)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("view");
                    format!("{stem}.png")
                });
                self.status_message = Some(crate::status_bar::StatusMessage::info(
                    "Choose a file for the PNG; the image is rendered after that...".to_string(),
                ));
                Task::perform(
                    crate::png_export::export_png_file(
                        suggested_name,
                        primitive,
                        width,
                        height,
                        options.smoothing,
                    ),
                    Message::ImageExported,
                )
            }
            other => {
                self.png_options.apply(other);
                Task::none()
            }
        }
    }

    /// 書き出す画像の大きさ（幅、高さ）と、書き出しに使う視点。`width` は、形が `Custom` でないときの幅。
    /// 構図が `Fit all` なら、表示中の構造全体が、その縦横比に収まるように、今の向きのまま距離を合わせる。
    pub(super) fn export_canvas(
        &self,
        canvas: &crate::canvas::CanvasOptions,
        width: u32,
    ) -> Result<((u32, u32), yui_core::state::CameraState), String> {
        // `3D pane` は、最初の分子図（書き出しに使う視点の持ち主）が最後に描かれた大きさの縦横比。
        // まだ描かれていないときは、ウィンドウの縦横比にする
        let pane_aspect = match self.viewport_sizes.get(viewport::ViewportId::Primary) {
            Some((w, h)) => w / h,
            None => self.window_size.width / self.window_size.height.max(1.0),
        };
        let (w, h) = canvas.size(width, pane_aspect)?;
        let points = if canvas.fit_all {
            viewport::fit_target_positions(
                &self.state.molecule,
                self.state.active_model.as_deref(),
                &self.model_atom_ranges,
            )
        } else {
            Vec::new()
        };
        let camera = canvas.camera(&self.state.camera, points, w as f32 / h as f32);
        Ok(((w, h), camera))
    }

    /// 画面外への描画（PNG の書き出し）に渡す、描画の準備ができたもの。
    /// 画面と同じ原子・結合・表面・相互作用を、今の視点で、`width` x `height` の縦横比に描く。
    /// 選択の強調は入れない。
    #[cfg(test)]
    pub(super) fn offscreen_primitive(
        &self,
        width: u32,
        height: u32,
        transparent: bool,
    ) -> viewport::primitive::ViewportPrimitive {
        self.offscreen_primitive_with(
            width,
            height,
            transparent,
            &self.state.camera,
            &self.surface_batches,
        )
    }

    /// `offscreen_primitive` の、視点と分子表面を指定する版（アニメーションの書き出し）
    pub(super) fn offscreen_primitive_with(
        &self,
        width: u32,
        height: u32,
        transparent: bool,
        camera: &yui_core::state::CameraState,
        surface_batches: &crate::surface_render::SurfaceBatches,
    ) -> viewport::primitive::ViewportPrimitive {
        use iced::widget::shader::Program;
        let no_selection = std::collections::BTreeSet::new();
        // 画面外の描き出しなので、画面の大きさの記録は更新しない（使い捨ての記録先）
        let scratch_sizes = viewport::ViewportSizes::default();
        let program = viewport::ViewportProgram::<Message> {
            sizes: &scratch_sizes,
            id: viewport::ViewportId::Primary,
            camera,
            molecule: &self.state.molecule,
            atom_group: self.state.atom_group.as_ref(),
            secondary_structure_map: &self.state.secondary_structure_map,
            representations: &self.state.representations,
            resolved_interactions: &self.resolved_interactions,
            active_model: self.state.active_model.as_deref(),
            model_atom_ranges: &self.model_atom_ranges,
            surface_batches,
            background_color: self.state.scene.background_color,
            selection: &no_selection,
            highlight: crate::selection::HighlightStyle::Off,
            selection_color: self.state.scene.background_color,
            camera_fit_token: 0,
            fit_pending: false,
            fit_selection: false,
            on_camera_changed: None,
            on_click: None,
            on_right_click: None,
        };
        let bounds = iced::Rectangle::new(
            iced::Point::ORIGIN,
            iced::Size::new(width as f32, height as f32),
        );
        let mut primitive = program.draw(
            &viewport::ViewportState::default(),
            iced::mouse::Cursor::Unavailable,
            bounds,
        );
        primitive.draw_background = !transparent;
        primitive
    }

    /// 分子図（3D Viewport）のウィジェット。2つの分子図は、視点だけが違う。
    fn view_molecule_viewport(&self, id: viewport::ViewportId) -> Element<'_, Message> {
        let viewport_widget = iced::widget::shader(viewport::ViewportProgram {
            id,
            camera: self.camera_of(id),
            molecule: &self.state.molecule,
            atom_group: self.state.atom_group.as_ref(),
            secondary_structure_map: &self.state.secondary_structure_map,
            representations: &self.state.representations,
            resolved_interactions: &self.resolved_interactions,
            active_model: self.state.active_model.as_deref(),
            model_atom_ranges: &self.model_atom_ranges,
            // 再生中（とスライダーを動かしている間）は、モデルとずれるので分子表面を描かない
            surface_batches: if self.playback.hides_surface() {
                crate::surface_render::SurfaceBatches::empty_ref()
            } else {
                &self.surface_batches
            },
            background_color: self.state.scene.background_color,
            sizes: &self.viewport_sizes,
            selection: &self.selection,
            highlight: self.highlight,
            selection_color: {
                let c = crate::ui_style::palette().selection;
                [c.r, c.g, c.b, c.a]
            },
            on_click: Some(Box::new(Message::ViewportClicked)),
            camera_fit_token: self.camera_fit_token,
            fit_pending: self.fit_pending.contains(&id),
            fit_selection: self.fit_selection,
            on_camera_changed: Some(Box::new(|id, camera, fitted| {
                Message::ViewportCameraChanged { id, camera, fitted }
            })),
            on_right_click: Some(Box::new(|screen_pos, target| {
                Message::ViewportRightClicked { screen_pos, target }
            })),
        })
        .width(Fill)
        .height(Fill);

        if has_structure(&self.state) {
            container(viewport_widget).width(Fill).height(Fill).into()
        } else {
            // 何も読み込まれていないときは、開き方の案内を分子図の上に重ねる
            let hint = container(
                text(NO_STRUCTURE_HINT)
                    .size(14)
                    .color(crate::ui_style::palette().text_subtle),
            )
            .center(Fill);
            iced::widget::stack![viewport_widget, hint]
                .width(Fill)
                .height(Fill)
                .into()
        }
    }

    /// 分子図の下端の、再生の操作のバー（フレームが 2 つ以上あるときだけ）
    fn view_playback_bar(&self) -> Option<Element<'_, Message>> {
        let current = self.current_frame();
        let label = match &self.trajectory {
            Some(t) => Some(format!("step {}", t.step)),
            None => current
                .and_then(|i| self.model_atom_ranges.get(i))
                .map(|r| r.model_key.clone()),
        };
        crate::playback_bar::view_playback_bar(
            &crate::playback_bar::PlaybackBarInput {
                total: self.frame_count(),
                current,
                label,
                all_label: self.trajectory.is_none(),
                playing: self.playback.playing,
                fps: self.playback.fps,
                loop_mode: self.playback.loop_mode,
            },
            Message::PlaybackControl,
        )
    }

    fn render_view_content<'a>(&'a self, view: ViewId) -> Element<'a, Message> {
        match view {
            ViewId::Viewport => {
                let viewport = self.view_molecule_viewport(viewport::ViewportId::Primary);
                match self.view_playback_bar() {
                    Some(bar) => column![viewport, bar].width(Fill).height(Fill).into(),
                    None => viewport,
                }
            }
            ViewId::Viewport2 => self.view_molecule_viewport(viewport::ViewportId::Secondary),
            ViewId::StructureTree => view_structure_tree(
                &self.structure_tree,
                self.state.atom_group.as_ref(),
                &self.state.secondary_structure_map,
                self.state.active_model.as_deref(),
                Message::StructureTreeAction,
            ),
            ViewId::NamedSelections => view_named_selections(
                &self.state.named_selections,
                self.renaming_selection.as_ref(),
                Message::NamedSelectionAction,
            ),
            ViewId::RepresentationInspector => {
                let inspector = view_representation_inspector(
                    &self.state.representations,
                    Message::RepresentationAction,
                );
                match &self.cube {
                    Some(cube) => column![
                        crate::cube_view::view_cube_card(
                            cube,
                            &self.state.cube_view,
                            Message::CubeAction,
                        ),
                        inspector
                    ]
                    .spacing(6)
                    .into(),
                    None => inspector,
                }
            }
            ViewId::LogViewer => self.log_viewer.view().map(Message::LogViewerMsg),
            ViewId::PythonConsole => container(text(
                "Python Console (Phase 3 Native / WASM-ready Console - Coming soon)",
            ))
            .padding(10)
            .into(),
            ViewId::Terminal => self.terminal_pane.view().map(Message::TerminalPaneMsg),
            ViewId::AiChat => container(text("AI Chat (Phase 6b Assistant - Coming soon)"))
                .padding(10)
                .into(),
            ViewId::Document(id) => match self.documents.get(id) {
                Some(doc) => text_editor_pane::view_document(doc).map(Message::Editor),
                None => text("This file has been closed.").size(12).into(),
            },
            // 古いワークスペースの 1 つだけのエディタのタブ。読み込み時に取り除くので、通常は表示されない。
            ViewId::LegacyTextEditor => text("").into(),
            ViewId::ProjectTree => self.project_tree.view().map(Message::ProjectTreePaneMsg),
            ViewId::Plot(kind) => self.plot_view(kind),
        }
    }

    /// ペインの本体に、タブを落とす層を重ねる。
    ///
    /// タブのドラッグ中だけ、本体の上に透明な `mouse_area` を置く。エディタのグループには 5 つ（中央と上下左右の端。
    /// 端は 4 分の 1）、ドック（左・右・下）には中央の 1 つだけ（ドックの中は分割しない）。
    /// マウスが入った場所を落とす先にする。落とすと配置が変わる場所なら、そこに入るタブの範囲を色で示す
    /// （中央はグループ全体、端はその側の半分）。ドラッグしていないときは、何もしない空白を置く。
    /// どちらのときも同じ形の `stack` にして、本体（分子図やエディタ）のウィジェットの状態が作り直されないようにする。
    fn with_tab_drop_layer<'a>(
        &self,
        pane: PaneKind,
        body: Element<'a, Message>,
    ) -> Element<'a, Message> {
        use crate::pane_view::DropZone;
        use iced::Length::FillPortion;
        use iced::widget::{Space, row};
        // ドックへは、落とす場所は中央だけ（ドックの中は分割しない）
        let is_group = matches!(pane, PaneKind::Group(_));
        let (highlight, zones): (Element<'a, Message>, Element<'a, Message>) = if self
            .tab_drag
            .is_some()
        {
            let palette = crate::ui_style::palette();
            let zone = |zone: DropZone, w: iced::Length, h: iced::Length| -> Element<'a, Message> {
                mouse_area(Space::new().width(w).height(h))
                    .on_enter(Message::TabDropTargetEntered(pane, zone))
                    .on_exit(Message::TabDropTargetLeft(pane, zone))
                    .into()
            };
            let zones = if !is_group {
                zone(DropZone::Center, Fill, Fill)
            } else {
                column![
                    zone(DropZone::Top, Fill, FillPortion(1)),
                    row![
                        zone(DropZone::Left, FillPortion(1), Fill),
                        zone(DropZone::Center, FillPortion(2), Fill),
                        zone(DropZone::Right, FillPortion(1), Fill),
                    ]
                    .height(FillPortion(2)),
                    zone(DropZone::Bottom, Fill, FillPortion(1)),
                ]
                .into()
            };
            let shown = match self.tab_drop_target {
                Some((p, z)) if p == pane && self.drop_changes_layout(p, z) => Some(z),
                _ => None,
            };
            let tint = move || -> Element<'a, Message> {
                container(Space::new())
                    .width(Fill)
                    .height(Fill)
                    .style(move |_theme| crate::ui_style::tab_drop_style(&palette, true))
                    .into()
            };
            let empty = || -> Element<'a, Message> { Space::new().width(Fill).height(Fill).into() };
            let highlight = match shown {
                None => empty(),
                Some(DropZone::Center) => tint(),
                Some(DropZone::Left) => row![tint(), empty()].into(),
                Some(DropZone::Right) => row![empty(), tint()].into(),
                Some(DropZone::Top) => column![tint(), empty()].into(),
                Some(DropZone::Bottom) => column![empty(), tint()].into(),
            };
            (highlight, zones)
        } else {
            (Space::new().into(), Space::new().into())
        };
        iced::widget::stack![body, highlight, zones]
            .width(Fill)
            .height(Fill)
            .into()
    }

    pub fn view(&self) -> Element<'_, Message> {
        // スタイル関数が今のテーマのパレットを参照できるようにする
        crate::ui_style::set_theme(self.theme);
        let grid = PaneGrid::new(&self.panes, |_pane, kind, _is_maximized| {
            let default_state = PaneViewState::new(vec![ViewId::Viewport], 0);
            let pane_state = self.pane_views.get(kind).unwrap_or(&default_state);

            let is_group = matches!(kind, PaneKind::Group(_));
            // タブのないペインには、案内を出す（ビューポートを、ここでもう 1 つ描画しないこと）。
            // エディタのグループ（タブをほかへ移した）と、ドック（タブをすべてほかへ移した）。
            if pane_state.views.is_empty() {
                let message = if is_group {
                    "No file open. Use File > Open Text File... or open a file from the Project tab."
                } else {
                    "Drag a tab here."
                };
                let hint = container(text(message).size(13)).center(Fill);
                let palette = crate::ui_style::palette();
                let title_bar = pane_grid::TitleBar::new(text(kind.title()).size(12))
                    .padding([4, 8])
                    .style(move |_theme| crate::ui_style::pane_title_style(&palette));
                return pane_grid::Content::new(self.with_tab_drop_layer(*kind, hint.into()))
                    .title_bar(title_bar)
                    .style(move |_theme| crate::ui_style::pane_body_style(&palette));
            }

            let active = pane_state.active_view().unwrap_or(ViewId::Viewport);
            let view_elem = self.with_tab_drop_layer(*kind, self.render_view_content(active));

            // タブの帯は、タブが 1 つでもいつも出す（どのタブも、ほかのペインへドラッグできるように。VSCode と同じ）
            let tab_bar = view_tab_bar(
                *kind,
                pane_state,
                |v| self.view_label(v),
                |p, v| Message::PaneTabSelected { pane: p, view: v },
                |p, v| Message::PaneTabHovered { pane: p, view: v },
                Message::CloseTabRequested,
                |p, dx| Message::TabBarWheel { pane: p, dx },
            );
            let content: Element<'_, Message> = column![tab_bar, view_elem]
                .spacing(4)
                .width(Fill)
                .height(Fill)
                .into();

            // 中央の領域は、表示しているビューの名前を見出しにする（タブで 3D Viewport / Text Editor を切り替えるため）
            let title = match kind {
                PaneKind::Group(_) => self.view_label(active),
                _ => kind.title().to_string(),
            };
            let palette = crate::ui_style::palette();
            let title_bar = pane_grid::TitleBar::new(text(title).size(12))
                .padding([4, 8])
                .style(move |_theme| crate::ui_style::pane_title_style(&palette));

            pane_grid::Content::new(content)
                .title_bar(title_bar)
                .style(move |_theme| crate::ui_style::pane_body_style(&palette))
        })
        .width(Fill)
        .height(Fill)
        .on_resize(10, Message::PaneResized)
        .on_drag(Message::PaneDragged);

        let menu_bar_elem = menu_bar::view_menu_bar(self.active_menu, Message::MenuAction);

        let mut layers = Vec::new();
        layers.push(container(grid).width(Fill).height(Fill).into());

        if let Some(cat) = self.active_menu {
            let items = self
                .menus
                .build(menu_location(cat), &self.menu_context(None));
            layers.push(menu_bar::view_dropdown(
                cat,
                &items,
                self.window_size,
                Message::RunCommand,
                Message::MenuAction(MenuAction::CloseMenu),
            ));
        }

        if let Some((pos, ref target)) = self.context_menu {
            let items = self.menus.build(
                menu::location::CONTEXT_VIEWPORT,
                &self.menu_context(Some(MenuTarget::Atom(target.clone()))),
            );
            layers.push(context_menu::view_context_menu(
                pos,
                &items,
                self.window_size,
                Message::RunCommand,
                Message::ContextMenuAction(context_menu::ContextMenuAction::Close),
            ));
        }

        if let Some((pos, ref target)) = self.tree_context_menu {
            let items = self.menus.build(
                menu::location::CONTEXT_STRUCTURE_TREE,
                &self.menu_context(Some(MenuTarget::TreeNode(target.clone()))),
            );
            layers.push(view_tree_context_menu(
                pos,
                &items,
                self.window_size,
                Message::RunCommand,
                Message::TreeContextMenuAction(TreeContextMenuAction::Close),
            ));
        }

        if let Some((pos, ref target)) = self.rep_context_menu {
            let items = self.menus.build(
                menu::location::CONTEXT_REPRESENTATION,
                &self.menu_context(Some(MenuTarget::Representation(target.clone()))),
            );
            layers.push(view_rep_context_menu(
                pos,
                &items,
                self.window_size,
                Message::RunCommand,
                Message::RepContextMenuAction(RepContextMenuAction::Close),
            ));
        }

        if let Some((pos, ref target)) = self.file_context_menu {
            let items = self.menus.build(
                menu::location::CONTEXT_FILE,
                &self.menu_context(Some(MenuTarget::File(target.clone()))),
            );
            layers.push(menu::popup_menu(
                &items,
                menu::Surface::Card,
                200.0,
                menu::Anchor::Cursor(pos),
                self.window_size,
                Message::RunCommand,
                Message::FileContextMenuClosed,
            ));
        }

        if let Some(id) = self.pending_close
            && let Some(doc) = self.documents.get(id)
        {
            layers.push(dim_modal(text_editor_pane::view_close_dialog(
                &doc.file_name,
                Message::CloseDialogChoice,
            )));
        }

        if self.quit_dialog {
            layers.push(dim_modal(text_editor_pane::view_quit_dialog(
                &self.unsaved_file_names(),
                Message::QuitDialogChoice,
            )));
        }

        if self.svg_dialog {
            layers.push(dim_modal(crate::svg_export::view_svg_dialog(
                self.svg_options,
                Message::SvgDialog,
            )));
        }

        if let Some(share) = &self.share_dialog {
            layers.push(dim_modal(crate::share_dialog::view_share_dialog(
                share,
                &self.backend_http_base(),
                Message::Share,
            )));
        }

        if self.png_dialog {
            layers.push(dim_modal(crate::png_export::view_png_dialog(
                self.png_options,
                Message::PngDialog,
            )));
        }

        if self.movie_dialog {
            layers.push(dim_modal(crate::movie_export::view_movie_dialog(
                self.movie_options,
                self.frame_count(),
                Message::MovieDialog,
            )));
        }

        if self.show_about {
            layers.push(menu_bar::view_about_modal(Message::MenuAction(
                MenuAction::CloseAbout,
            )));
        }

        #[cfg(not(target_arch = "wasm32"))]
        if self.show_plugins {
            layers.push(crate::plugin_list::view_plugins_modal(
                &self.plugin_rows,
                crate::plugins::plugins_dir().as_deref(),
                self.plugin_notice.as_deref(),
                Message::PluginsDialogClose,
                Message::PluginToggled,
            ));
        }

        if self.show_controls {
            layers.push(menu_bar::view_controls_modal(Message::MenuAction(
                MenuAction::CloseControls,
            )));
        }

        let main_stack = iced::widget::stack(layers).width(Fill).height(Fill);

        let mut top_column = column![menu_bar_elem];

        if let Some((ref path, ref candidates)) = self.pending_open_choice {
            let filename = std::path::Path::new(path)
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or(path.as_str());

            let prompt_text = text(format!("Open '{filename}' with:")).size(13);
            let mut btn_row = iced::widget::row![prompt_text]
                .spacing(8)
                .align_y(iced::Alignment::Center);

            for handler in candidates {
                let label = handler.label();
                let btn = iced::widget::button(text(label).size(12))
                    .on_press(Message::OpenHandlerChosen(*handler))
                    .padding([2, 8]);
                btn_row = btn_row.push(btn);
            }
            btn_row = btn_row.push(
                iced::widget::button(text("Cancel").size(12))
                    .on_press(Message::OpenHandlerCancelled)
                    .padding([2, 8]),
            );

            let bar = container(btn_row)
                .padding([4, 12])
                .style(|_theme| container::Style {
                    background: Some(crate::ui_style::palette().banner_bg.into()),
                    ..Default::default()
                })
                .width(Fill);

            top_column = top_column.push(bar);
        }

        if let Some(job) = &self.movie {
            let progress = text(format!(
                "Exporting the animation: {}/{} frames",
                job.next.min(job.total),
                job.total
            ))
            .size(13);
            let cancel = iced::widget::button(text("Cancel").size(12))
                .on_press(Message::MovieCancel)
                .padding([2, 8]);
            let bar = container(
                iced::widget::row![progress, cancel]
                    .spacing(12)
                    .align_y(iced::Alignment::Center),
            )
            .padding([4, 12])
            .style(|_theme| container::Style {
                background: Some(crate::ui_style::palette().banner_bg.into()),
                ..Default::default()
            })
            .width(Fill);
            top_column = top_column.push(bar);
        }

        let root = top_column
            .push(main_stack)
            .push(self.view_status_bar())
            .width(Fill)
            .height(Fill);
        // いちばん上に、ドラッグ中のタブの四角形の層を重ねる（いつも 2 層にして、画面の状態が作り直されないようにする）
        iced::widget::stack![root, self.view_tab_drag_ghost()]
            .width(Fill)
            .height(Fill)
            .into()
    }

    /// ドラッグ中のタブの四角形（VSCode と同じく、マウスと一緒に動く）。
    ///
    /// 押した位置から `TAB_DRAG_GHOST_THRESHOLD` より動かしたときだけ出す（クリックで、一瞬だけ出ないように）。
    /// マウスの真下ではなく、少し右下に置く。真下に置くと、下のタブの帯や、落とす先の判定にマウスが届かなくなる。
    fn view_tab_drag_ghost(&self) -> Element<'_, Message> {
        let Some(label) = self.tab_drag_ghost_label() else {
            return iced::widget::Space::new().into();
        };
        let [x, y] = self.cursor_position;
        let palette = crate::ui_style::palette();
        let ui = crate::ui_style::ui_settings();
        let ghost = container(text(label).size(ui.tab_font_size))
            .padding([4, 10])
            .style(move |_theme| crate::ui_style::tab_drag_ghost_style(&palette));
        iced::widget::pin(ghost)
            .position(iced::Point::new(
                x + TAB_DRAG_GHOST_OFFSET[0],
                y + TAB_DRAG_GHOST_OFFSET[1],
            ))
            .into()
    }
}

impl Yui {
    /// ドラッグ中のタブの四角形に出す名前。出さないとき（ドラッグしていない、まだ動かしていない）は `None`
    pub(super) fn tab_drag_ghost_label(&self) -> Option<String> {
        let (_, view) = self.tab_drag?;
        let [x, y] = self.cursor_position;
        let [x0, y0] = self.tab_drag_start;
        ((x - x0).hypot(y - y0) >= TAB_DRAG_GHOST_THRESHOLD).then(|| self.view_label(view))
    }
}

/// ドラッグ中のタブの四角形を出すまでに、押した位置から動かす距離（ピクセル）
const TAB_DRAG_GHOST_THRESHOLD: f32 = 4.0;
/// ドラッグ中のタブの四角形を置く位置（マウスからのずれ。ピクセル）
const TAB_DRAG_GHOST_OFFSET: [f32; 2] = [12.0, 10.0];

/// 起動時の設定の警告を、ステータスバーに出す 1 つのメッセージにする（なければ `None`）
fn startup_settings_message() -> Option<crate::status_bar::StatusMessage> {
    let warnings = crate::user_settings::startup_warnings();
    if warnings.is_empty() {
        None
    } else {
        Some(crate::status_bar::StatusMessage::warning(
            warnings.join("  |  "),
        ))
    }
}

/// 確認のダイアログを、背面を暗くして重ねる（背面の操作は受け付けない）
fn dim_modal(content: Element<'_, Message>) -> Element<'_, Message> {
    iced::widget::opaque(
        container(content)
            .width(Fill)
            .height(Fill)
            .style(|_| container::Style {
                background: Some(crate::ui_style::palette().scrim.into()),
                ..Default::default()
            }),
    )
}

/// 何も読み込まれていないときに、中央の分子図に表示する案内
pub(super) const NO_STRUCTURE_HINT: &str =
    "No structure loaded. Open a file from the Project tab or File > Open.";

/// 表示する構造があるか。
///
/// backend のデモ構造のように `atom_group` がなく平坦化キャッシュ（`molecule`）だけが
/// 届いている場合も、構造があるとみなす。
pub(super) fn has_structure(state: &AppState) -> bool {
    state.atom_group.is_some() || !state.molecule.atoms.is_empty()
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod gallery;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod ui_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use yui_core::representation::default_representations;
    use yui_core::structure_loader::load_structure_into_state;

    fn read_1hls() -> Vec<u8> {
        let pdb_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/tests/data/1hls.pdb");
        std::fs::read(pdb_path).expect("failed to read 1hls.pdb")
    }

    #[test]
    fn test_startup_state_is_blank() {
        // 起動時の状態（File → New と同じ）には、構造も表示設定もない
        let state = menu_bar::create_empty_state();
        assert!(state.atom_group.is_none());
        assert!(state.molecule.atoms.is_empty());
        assert!(state.representations.is_empty());
        assert!(!has_structure(&state));
    }

    #[test]
    fn test_load_after_new_adds_default_representations() {
        // File → New の後に構造を読み込むと、既定の2つの表示設定が追加される
        let mut state = menu_bar::create_empty_state();
        load_structure_into_state(&mut state, &read_1hls(), "1hls.pdb").expect("load 1hls.pdb");
        assert!(has_structure(&state));
        assert_eq!(state.representations, default_representations());
    }

    #[test]
    fn test_camera_fit_requested_on_structure_change_only() {
        use crate::file_open_dispatch::FileOpenHandler;
        use crate::menu_bar::MenuAction;
        use crate::structure_tree::StructureTreeAction;

        let cif_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/tests/data/1HLS.cif");
        let bytes = std::fs::read(cif_path).expect("failed to read 1HLS.cif");

        let mut app = Yui::default();
        let t0 = app.camera_fit_token;

        // 構造を開くと、カメラを合わせる
        let _ = app.update(Message::FileContentFetched {
            path: "1HLS.cif".to_string(),
            handler: FileOpenHandler::StructureViewer,
            result: Ok(bytes),
        });
        assert!(app.state.atom_group.is_some());
        let t1 = app.camera_fit_token;
        assert_ne!(t1, t0);

        // 表示中のモデルを切り替えても、カメラは合わせない
        let _ = app.update(Message::StructureTreeAction(
            StructureTreeAction::SelectModel(Some("model_2".to_string())),
        ));
        assert_eq!(app.state.active_model.as_deref(), Some("model_2"));
        assert_eq!(app.camera_fit_token, t1);

        // Reset Camera と File → New では合わせる
        let _ = app.update(Message::MenuAction(MenuAction::ViewResetCamera));
        let t2 = app.camera_fit_token;
        assert_ne!(t2, t1);
        let _ = app.update(Message::MenuAction(MenuAction::FileNew));
        assert_ne!(app.camera_fit_token, t2);
    }

    #[test]
    fn test_fit_target_positions_follow_active_model() {
        use crate::viewport::fit_target_positions;
        use yui_core::structure_loader::load_structure_into_state;

        let cif_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/tests/data/1HLS.cif");
        let bytes = std::fs::read(cif_path).expect("failed to read 1HLS.cif");
        let mut state = menu_bar::create_empty_state();
        load_structure_into_state(&mut state, &bytes, "1HLS.cif").expect("load 1HLS.cif");
        let ranges = yui_core::calculate_model_atom_ranges(state.atom_group.as_ref().unwrap());

        // 表示中のモデル（model_1）の原子だけ
        let one = fit_target_positions(&state.molecule, Some("model_1"), &ranges);
        assert_eq!(one.len(), 782);
        // 全モデル表示なら全原子
        let all = fit_target_positions(&state.molecule, None, &ranges);
        assert_eq!(all.len(), state.molecule.atoms.len());
        // 存在しないモデルなら空（既定のカメラになる）
        assert!(fit_target_positions(&state.molecule, Some("model_99"), &ranges).is_empty());
    }

    #[test]
    fn test_has_structure_with_flat_molecule_only() {
        // backend のデモ構造のように、平坦化キャッシュだけがある場合も構造ありとみなす
        let mut state = menu_bar::create_empty_state();
        state.molecule.atoms.push(yui_core::Atom {
            id: 1,
            element: "C".to_string(),
            position: [0.0, 0.0, 0.0],
            radius: 1.7,
            color: [0.5, 0.5, 0.5, 1.0],
            residue_name: "UNL".to_string(),
            residue_index: 1,
            chain_id: "A".to_string(),
        });
        assert!(state.atom_group.is_none());
        assert!(has_structure(&state));
    }

    /// 原点に球対称の exp(-r^2) を持つ小さな CUBE（Bohr）
    fn sphere_cube() -> String {
        let (n, h) = (11usize, 0.5f64);
        let o = -(n as f64 - 1.0) * h / 2.0;
        let mut s = format!("t\nt\n    1 {o:.6} {o:.6} {o:.6}\n");
        s.push_str(&format!(
            "{n:5} {h} 0.0 0.0\n{n:5} 0.0 {h} 0.0\n{n:5} 0.0 0.0 {h}\n"
        ));
        s.push_str("    1 1.0 0.0 0.0 0.0\n");
        for ix in 0..n {
            for iy in 0..n {
                for iz in 0..n {
                    let (x, y, z) = (o + ix as f64 * h, o + iy as f64 * h, o + iz as f64 * h);
                    s.push_str(&format!(" {:.8e}\n", (-(x * x + y * y + z * z)).exp()));
                }
            }
        }
        s
    }

    /// 開いている CUBE の「表示したいもの」で、計算の結果を作って、アプリへ渡す
    fn finish_cube_job(app: &mut Yui) -> Task<Message> {
        let cube = app.cube.as_ref().unwrap();
        let job = cube.job(&app.state.cube_view);
        let result = crate::cube_view::run_job(job);
        app.update(Message::CubeJobDone(result))
    }

    #[test]
    fn test_opening_and_closing_a_cube_changes_the_translucent_batch() {
        use crate::cube_view::CubeAction;
        let mut app = Yui::default();
        assert!(app.surface_batches.transparent_vertices.is_empty());
        let set = std::sync::Arc::new(yui_core::cube_iso::CubeSet::parse(&sphere_cube()).unwrap());
        assert!(app.state.molecule.atoms.is_empty());
        let _ = app.update(Message::CubeOpened(Ok(Some((
            "s.cube".to_string(),
            set,
            None,
        )))));
        assert!(app.cube.as_ref().unwrap().busy, "the first job is running");
        assert_eq!(
            app.state.molecule.atoms.len(),
            1,
            "the CUBE's atom is shown"
        );
        assert!(app.surface_batches.transparent_vertices.is_empty());

        let _ = finish_cube_job(&mut app);
        assert!(!app.cube.as_ref().unwrap().busy);
        assert!(!app.surface_batches.transparent_vertices.is_empty());
        assert!(app.surface_batches.opaque_vertices.is_empty());

        // 分子表面を作り直しても（表面がなくなっても）、CUBE は残る
        let _ = app.trigger_surface_rebuild();
        assert!(!app.surface_batches.transparent_vertices.is_empty());

        // 等値レベルを変えると、計算が始まる。計算中の変更は重ねず、終わったときに最新で計算し直す
        let before = app.cube_entries[0].mesh.positions.len();
        let _ = app.update(Message::CubeAction(CubeAction::SetIsolevel(0.5)));
        assert!(app.cube.as_ref().unwrap().busy);
        let _ = app.update(Message::CubeAction(CubeAction::SetIsolevel(0.6)));
        let job = crate::cube_view::CubeJob {
            set: app.cube.as_ref().unwrap().set.clone(),
            dataset: 0,
            grid: None,
            isolevel: Some(0.5),
        };
        let _ = app.update(Message::CubeJobDone(crate::cube_view::run_job(job)));
        assert!(
            app.cube.as_ref().unwrap().busy,
            "0.6 was asked while 0.5 was running, so it runs next"
        );
        let _ = finish_cube_job(&mut app);
        let cube = app.cube.as_ref().unwrap();
        assert!(!cube.busy);
        assert_eq!(cube.shown.as_ref().unwrap().isolevel, 0.6);
        assert_ne!(app.cube_entries[0].mesh.positions.len(), before);

        app.close_cube();
        assert!(app.cube.is_none());
        assert!(app.surface_batches.transparent_vertices.is_empty());

        // 色だけの変更は計算せずに、面の色を替える
        let set = std::sync::Arc::new(yui_core::cube_iso::CubeSet::parse(&sphere_cube()).unwrap());
        let _ = app.update(Message::CubeOpened(Ok(Some((
            "s.cube".to_string(),
            set,
            None,
        )))));
        let _ = finish_cube_job(&mut app);
        let _ = app.update(Message::CubeAction(CubeAction::SetPositiveColor(2)));
        assert!(!app.cube.as_ref().unwrap().busy);
        assert_eq!(
            app.cube_entries[0].vertices[0].color[..3],
            crate::cube_view::COLOR_PRESETS[2].1[..3]
        );
        // ファイルのパスは、ワークスペースに保存する。閉じる、File → New で消える
        app.close_cube();
        let set = std::sync::Arc::new(yui_core::cube_iso::CubeSet::parse(&sphere_cube()).unwrap());
        let _ = app.update(Message::CubeOpened(Ok(Some((
            "p.cube".to_string(),
            set,
            Some("/data/p.cube".to_string()),
        )))));
        assert_eq!(
            app.state.data_paths.cube_path.as_deref(),
            Some("/data/p.cube")
        );
        let yaml = crate::pane_view::export_workspace_to_yaml_str(
            &app.state,
            &crate::pane_view::PaneLayout::from(std::collections::BTreeMap::new()),
        )
        .unwrap();
        assert!(yaml.contains("/data/p.cube"));
        let _ = app.update(Message::MenuAction(crate::menu_bar::MenuAction::FileNew));
        assert!(app.cube.is_none());
        assert_eq!(app.state.data_paths.cube_path, None);
        assert!(app.surface_batches.transparent_vertices.is_empty());

        // 構造を開いているときは、CUBE の原子で構造を置き換えない
        let set = std::sync::Arc::new(yui_core::cube_iso::CubeSet::parse(&sphere_cube()).unwrap());
        let _ = app.update(Message::CubeOpened(Ok(Some((
            "s.cube".to_string(),
            set,
            None,
        )))));
        let _ = finish_cube_job(&mut app);
        let atoms_before = app.state.molecule.atoms.len();
        let set = std::sync::Arc::new(yui_core::cube_iso::CubeSet::parse(&sphere_cube()).unwrap());
        let _ = app.update(Message::CubeOpened(Ok(Some((
            "t.cube".to_string(),
            set,
            None,
        )))));
        assert_eq!(app.state.molecule.atoms.len(), atoms_before);
        app.close_cube();

        // 閉じたあとに届いた計算の結果と、読めないファイルは、何も変えない
        let _ = app.update(Message::CubeJobDone(Err("late".to_string())));
        let _ = app.update(Message::CubeOpened(Err("bad".to_string())));
        assert!(app.cube.is_none());
    }

    #[test]
    fn test_cube_opacity_is_part_of_the_workspace_state() {
        use crate::cube_view::CubeAction;
        let open = |app: &mut Yui, path: Option<&str>| {
            let set =
                std::sync::Arc::new(yui_core::cube_iso::CubeSet::parse(&sphere_cube()).unwrap());
            let _ = app.update(Message::CubeOpened(Ok(Some((
                "s.cube".to_string(),
                set,
                path.map(str::to_string),
            )))));
            let _ = finish_cube_job(app);
        };
        let mut app = Yui::default();
        open(&mut app, Some("/data/a.cube"));
        assert_eq!(app.state.cube_view.opacity, 0.6);
        assert_eq!(app.cube_entries[0].opacity, 0.6);

        // 不透明度を変えると、計算はせずに、面の不透明度と頂点の色が替わり、状態に入る
        let _ = app.update(Message::CubeAction(CubeAction::SetOpacity(0.3)));
        assert!(!app.cube.as_ref().unwrap().busy);
        assert_eq!(app.state.cube_view.opacity, 0.3);
        assert_eq!(app.cube_entries[0].opacity, 0.3);
        assert_eq!(app.cube_entries[0].vertices[0].color[3], 0.3);
        let _ = app.update(Message::CubeAction(CubeAction::SetOpacity(f32::NAN)));
        assert_eq!(app.state.cube_view.opacity, 0.6, "NaN gives the default");

        // ワークスペースの YAML に保存され、読み込み直すと同じになる
        let _ = app.update(Message::CubeAction(CubeAction::SetOpacity(0.3)));
        let _ = app.update(Message::CubeAction(CubeAction::SetNegativeColor(3)));
        let yaml = crate::pane_view::export_workspace_to_yaml_str(
            &app.state,
            &crate::pane_view::PaneLayout::from(std::collections::BTreeMap::new()),
        )
        .unwrap();
        assert!(yaml.contains("cube_view"), "{yaml}");
        let (loaded, _) = crate::pane_view::import_workspace_from_yaml_str(&yaml).unwrap();
        assert_eq!(loaded.cube_view, app.state.cube_view);

        // 同じファイルをワークスペースから開き直したときは、保存された見た目を使う。
        // 別のファイルを開いたときは、既定に戻す
        open(&mut app, Some("/data/a.cube"));
        assert_eq!(app.state.cube_view.opacity, 0.3);
        assert_eq!(app.state.cube_view.negative_color, 3);
        open(&mut app, Some("/data/b.cube"));
        assert_eq!(app.state.cube_view, Default::default());
        let _ = app.update(Message::CubeAction(CubeAction::SetOpacity(0.9)));
        app.close_cube();
        assert_eq!(app.state.cube_view, Default::default());
    }

    #[test]
    fn test_cube_dataset_and_isolevel_are_saved_and_restored() {
        use crate::cube_view::CubeAction;
        let orbitals = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../core/tests/data/orbitals.cube"
        ))
        .unwrap();
        // 計算中の依頼を、同期的に終わらせる（続けて頼まれたものも）
        let drain = |app: &mut Yui| {
            for _ in 0..4 {
                if !app.cube.as_ref().is_some_and(|c| c.busy) {
                    break;
                }
                let _ = finish_cube_job(app);
            }
        };
        let open = |app: &mut Yui, path: &str| {
            let set = std::sync::Arc::new(yui_core::cube_iso::CubeSet::parse(&orbitals).unwrap());
            let _ = app.update(Message::CubeOpened(Ok(Some((
                "orbitals.cube".to_string(),
                set,
                Some(path.to_string()),
            )))));
            drain(app);
        };
        let mut app = Yui::default();
        open(&mut app, "/data/o.cube");
        let _ = app.update(Message::CubeAction(CubeAction::SetDataset(1)));
        drain(&mut app);
        let _ = app.update(Message::CubeAction(CubeAction::SetIsolevel(0.3)));
        drain(&mut app);
        let view = app.state.cube_view;
        assert_eq!((view.dataset, view.isolevel), (1, 0.3));
        assert_eq!(
            app.cube.as_ref().unwrap().shown.as_ref().unwrap().isolevel,
            0.3
        );

        // 保存して読み込み直し、同じファイルを開き直すと、同じ軌道と等値レベルで表示する
        let yaml = crate::pane_view::export_workspace_to_yaml_str(
            &app.state,
            &crate::pane_view::PaneLayout::from(std::collections::BTreeMap::new()),
        )
        .unwrap();
        assert!(yaml.contains("isolevel"), "{yaml}");
        let (loaded, _) = crate::pane_view::import_workspace_from_yaml_str(&yaml).unwrap();
        assert_eq!(loaded.cube_view, view);
        app.state = loaded;
        app.cube = None;
        open(&mut app, "/data/o.cube");
        let shown = app.cube.as_ref().unwrap().shown.as_ref().unwrap();
        assert_eq!((shown.dataset, shown.isolevel), (1, 0.3));

        // 軌道を替えると、等値レベルはその軌道の既定値に戻る
        let _ = app.update(Message::CubeAction(CubeAction::SetDataset(0)));
        assert_eq!(app.state.cube_view.isolevel, 0.0);
        drain(&mut app);

        // 別のファイルを開くと既定に戻る。保存された軌道の番号が、このファイルにない値でも止まらない
        open(&mut app, "/data/other.cube");
        assert_eq!(app.state.cube_view, Default::default());
        app.state.cube_view.dataset = 9;
        app.state.cube_view.isolevel = f32::NAN;
        app.state.data_paths.cube_path = Some("/data/o.cube".to_string());
        open(&mut app, "/data/o.cube");
        assert_eq!(
            app.state.cube_view.dataset, 1,
            "clamped to the last data set"
        );
        assert_eq!(app.state.cube_view.isolevel, 0.0, "NaN gives the default");
        assert!(app.cube.as_ref().unwrap().shown.is_some());
    }

    #[test]
    fn test_png_export_renders_the_structure_from_the_current_camera() {
        let mut app = Yui::default();
        let mut state = app.state.clone();
        load_structure_into_state(&mut state, &read_1hls(), "1hls.pdb").unwrap();
        app.state = state;
        app.update_model_atom_ranges();
        let ((w, h), _) = app
            .export_canvas(&crate::canvas::CanvasOptions::default(), 2000)
            .unwrap();
        assert_eq!(w, 2000);
        let aspect = app.window_size.width / app.window_size.height;
        assert!((h as f32 - 2000.0 / aspect).abs() <= 1.0);
        // 小さく描いて確かめる（GPU のアダプターがなければ、読み飛ばす）
        let primitive = app.offscreen_primitive(256, 192, false);
        // 本番では iced の tokio の中で動く（読み戻しの待ちを spawn_blocking で行う）
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let result = runtime.block_on(crate::viewport::offscreen::render_png(
            primitive, 256, 192, 2,
        ));
        let (bytes, pw, ph) = match result {
            Ok(r) => r,
            Err(e) if e.contains("No graphics adapter") => {
                eprintln!("png export: skipped ({e})");
                return;
            }
            Err(e) => panic!("{e}"),
        };
        assert_eq!((pw, ph), (256, 192));
        let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buf).unwrap();
        assert_eq!((info.width, info.height), (256, 192));
        // 背景だけではない（角の画素と違う画素がたくさんある）
        let corner = [buf[0], buf[1], buf[2]];
        let (pixels, _) = buf[..info.buffer_size()].as_chunks::<4>();
        let differing = pixels
            .iter()
            .filter(|p| [p[0], p[1], p[2]] != corner)
            .count();
        assert!(
            differing > 2000,
            "{differing} pixels differ from the corner"
        );
    }

    #[test]
    fn test_png_dialog_opens_keeps_options_and_closes() {
        use crate::png_export::{PngDialogAction as A, PngOptions};
        let mut app = Yui::default();
        // 構造がないと、開かない
        let _ = app.update(Message::MenuAction(MenuAction::FileExportImagePng));
        assert!(!app.png_dialog);

        let mut state = app.state.clone();
        load_structure_into_state(&mut state, &read_1hls(), "1hls.pdb").unwrap();
        app.state = state;
        let _ = app.update(Message::MenuAction(MenuAction::FileExportImagePng));
        assert!(app.png_dialog);
        let _ = app.update(Message::PngDialog(A::SetWidth(3000)));
        let _ = app.update(Message::PngDialog(A::SetTransparent(true)));
        let _ = app.update(Message::PngDialog(A::Cancel));
        assert!(!app.png_dialog);
        // 取り消しても、選んだ設定は次の回に残る
        assert_eq!(
            app.png_options,
            PngOptions {
                width: 3000,
                transparent: true,
                ..PngOptions::default()
            }
        );
        let _ = app.update(Message::MenuAction(MenuAction::FileExportImagePng));
        assert!(app.png_dialog);
        // 書き出しに進むと、ダイアログは閉じて、描画を始める
        let _ = app.update(Message::PngDialog(A::Export));
        assert!(!app.png_dialog);
        assert!(
            app.status_message
                .as_ref()
                .is_some_and(|m| m.text.contains("Choose a file"))
        );
    }

    #[test]
    fn test_png_export_with_a_transparent_background() {
        let mut app = Yui::default();
        let mut state = app.state.clone();
        load_structure_into_state(&mut state, &read_1hls(), "1hls.pdb").unwrap();
        app.state = state;
        app.update_model_atom_ranges();
        let primitive = app.offscreen_primitive(256, 192, true);
        assert!(!primitive.draw_background);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let (bytes, w, h) = match runtime.block_on(crate::viewport::offscreen::render_png(
            primitive, 256, 192, 2,
        )) {
            Ok(r) => r,
            Err(e) if e.contains("No graphics adapter") => {
                eprintln!("png export: skipped ({e})");
                return;
            }
            Err(e) => panic!("{e}"),
        };
        assert_eq!((w, h), (256, 192));
        let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buf).unwrap();
        let (pixels, _) = buf[..info.buffer_size()].as_chunks::<4>();
        // 背景は透明で、分子のところは不透明
        assert_eq!(pixels[0][3], 0, "the corner is transparent");
        let opaque = pixels.iter().filter(|p| p[3] == 255).count();
        let transparent = pixels.iter().filter(|p| p[3] == 0).count();
        assert!(opaque > 1000, "{opaque} opaque pixels");
        assert!(transparent > 20000, "{transparent} transparent pixels");
    }

    #[test]
    fn test_export_canvas_3d_pane_uses_the_drawn_pane_not_the_window() {
        let app = Yui {
            window_size: iced::Size::new(1000.0, 500.0), // 2:1
            ..Default::default()
        };
        let canvas = crate::canvas::CanvasOptions::default();
        assert_eq!(canvas.shape, crate::canvas::CanvasShape::Pane);
        // まだ描かれていないときは、ウィンドウの縦横比
        assert_eq!(app.export_canvas(&canvas, 1000).unwrap().0, (1000, 500));
        // 3D ペインが 600 x 600 で描かれたあとは、その縦横比（ウィンドウは別の形のまま）
        app.viewport_sizes
            .set(viewport::ViewportId::Primary, 600.0, 600.0);
        assert_eq!(app.export_canvas(&canvas, 1000).unwrap().0, (1000, 1000));
        app.viewport_sizes
            .set(viewport::ViewportId::Primary, 800.0, 400.0);
        assert_eq!(app.export_canvas(&canvas, 1000).unwrap().0, (1000, 500));
        // 固定の比率と Custom は、ペインに依らない
        let mut square = canvas;
        square.shape = crate::canvas::CanvasShape::Square;
        assert_eq!(app.export_canvas(&square, 1000).unwrap().0, (1000, 1000));
    }

    #[test]
    fn test_export_canvas_uses_the_shape_the_custom_size_and_the_framing() {
        use crate::canvas::{CanvasAction as C, CanvasOptions, CanvasShape};
        let mut app = Yui::default();
        let mut state = app.state.clone();
        load_structure_into_state(&mut state, &read_1hls(), "1hls.pdb").unwrap();
        app.state = state;
        app.update_model_atom_ranges();
        app.window_size = iced::Size::new(1000.0, 500.0);
        let mut canvas = CanvasOptions::default();

        // ウィンドウの形（2:1）
        assert_eq!(app.export_canvas(&canvas, 1000).unwrap().0, (1000, 500));
        // 決まった形と、Custom（幅の候補は使わない）
        canvas.apply(C::Shape(CanvasShape::Square));
        assert_eq!(app.export_canvas(&canvas, 1000).unwrap().0, (1000, 1000));
        canvas.apply(C::Shape(CanvasShape::Custom));
        canvas.apply(C::CustomWidth(640));
        canvas.apply(C::CustomHeight(1136));
        assert_eq!(app.export_canvas(&canvas, 1000).unwrap().0, (640, 1136));
        canvas.apply(C::CustomHeight(0));
        assert!(app.export_canvas(&canvas, 1000).is_err(), "an empty field");

        // 構図: 画面のまま（視点は同じ）と、全体に合わせる（縦長なので、横に収まる距離になる）
        canvas.apply(C::CustomHeight(1136));
        let (_, same) = app.export_canvas(&canvas, 1000).unwrap();
        assert_eq!(same, app.state.camera);
        canvas.apply(C::FitAll(true));
        let (_, fitted) = app.export_canvas(&canvas, 1000).unwrap();
        assert_ne!(fitted, app.state.camera);
        let dist = |c: &yui_core::state::CameraState| {
            let v = [
                c.eye[0] - c.target[0],
                c.eye[1] - c.target[1],
                c.eye[2] - c.target[2],
            ];
            (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
        };
        // 同じ構造を、横長のキャンバスに合わせるより、縦長に合わせるほうが遠い
        canvas.apply(C::CustomWidth(1136));
        canvas.apply(C::CustomHeight(640));
        let (_, wide) = app.export_canvas(&canvas, 1000).unwrap();
        assert!(dist(&fitted) > dist(&wide));
    }

    #[test]
    fn test_svg_dialog_flow_and_a_custom_size_svg() {
        use crate::canvas::{CanvasAction as C, CanvasShape};
        use crate::svg_export::SvgDialogAction as A;
        let mut app = Yui::default();
        let _ = app.update(Message::MenuAction(MenuAction::FileExportImageSvg));
        assert!(!app.svg_dialog, "no structure");

        let mut state = app.state.clone();
        load_structure_into_state(&mut state, &read_1hls(), "1hls.pdb").unwrap();
        app.state = state;
        let _ = app.update(Message::MenuAction(MenuAction::FileExportImageSvg));
        assert!(app.svg_dialog);
        let _ = app.update(Message::SvgDialog(A::Canvas(C::Shape(CanvasShape::Custom))));
        let _ = app.update(Message::SvgDialog(A::Canvas(C::CustomWidth(720))));
        let _ = app.update(Message::SvgDialog(A::Canvas(C::CustomHeight(1280))));
        let _ = app.update(Message::SvgDialog(A::Cancel));
        assert!(!app.svg_dialog);
        assert_eq!(app.svg_options.canvas.custom_width, 720, "remembered");
        // 指定の大きさの SVG ができる
        let ((w, h), camera) = app
            .export_canvas(&app.svg_options.canvas, app.svg_options.width)
            .unwrap();
        let svg = crate::svg_export::render_view_svg(
            &app.state,
            &app.model_atom_ranges,
            &app.resolved_interactions,
            &app.surface_batches,
            &camera,
            (w, h),
        )
        .unwrap();
        assert!(svg.contains("viewBox=\"0 0 720 1280\""), "{}", &svg[..200]);
    }

    #[test]
    fn test_svg_includes_the_ribbon_and_the_surfaces_that_the_screen_shows() {
        use yui_core::representation::{ColorScheme, Representation, RepresentationKind};
        let mut app = Yui::default();
        let mut state = app.state.clone();
        load_structure_into_state(&mut state, &read_1hls(), "1hls.pdb").unwrap();
        app.state = state;
        app.update_model_atom_ranges();
        let rep = |id: &str, kind, color| {
            Representation::new(id, yui_core::selector::Selector::All, kind, color)
        };
        let svg_of = |app: &Yui| {
            let ((w, h), camera) = app
                .export_canvas(&crate::canvas::CanvasOptions::default(), 800)
                .unwrap();
            crate::svg_export::render_view_svg(
                &app.state,
                &app.model_atom_ranges,
                &app.resolved_interactions,
                &app.surface_batches,
                &camera,
                (w, h),
            )
            .unwrap()
        };
        let polygons = |svg: &str| svg.matches("<polygon").count();

        // 球とスティックだけなら、多角形はない
        app.state.representations = vec![rep(
            "bb",
            RepresentationKind::BallAndStick,
            ColorScheme::Element,
        )];
        assert_eq!(polygons(&svg_of(&app)), 0);

        // カートゥーンが入る。目のアイコンで消すと、また 0
        app.state.representations = vec![rep(
            "c",
            RepresentationKind::Cartoon,
            ColorScheme::SecondaryStructure,
        )];
        assert!(polygons(&svg_of(&app)) > 1000);
        app.state.representations[0].visible = false;
        assert_eq!(polygons(&svg_of(&app)), 0);

        // 分子表面（不透明）。画面と同じバッチから作るので、バッチを空にすると消える
        app.state.representations = vec![rep(
            "s",
            RepresentationKind::Surface,
            ColorScheme::Uniform([0.3, 0.6, 1.0, 1.0]),
        )];
        let requests = app.surface_build_requests();
        let ctx = crate::surface_render::SurfaceColorContext::new(
            &app.state.molecule.atoms,
            app.state.atom_group.as_ref(),
            &app.state.secondary_structure_map,
        );
        app.surface_entries = ctx.entries_from_results(
            crate::surface_render::process_surface_build_requests(&requests),
        );
        app.refresh_surface_batches();
        let opaque = svg_of(&app);
        assert!(polygons(&opaque) > 1000);
        assert!(
            !opaque.contains("fill-opacity=\"0."),
            "an opaque surface has no see-through polygons"
        );
        // 不透明な面は、背面を省くので、全部（表と裏）の半分ほどになる
        let all = app.surface_batches.opaque_indices.len() / 3;
        assert!(
            polygons(&opaque) < all * 3 / 4,
            "{} of {all}",
            polygons(&opaque)
        );
        // 半透明にすると、背面も出て、多角形は半透明になる
        app.state.representations[0].opacity = 0.5;
        let requests = app.surface_build_requests();
        app.surface_entries = ctx.entries_from_results(
            crate::surface_render::process_surface_build_requests(&requests),
        );
        app.refresh_surface_batches();
        let glass = svg_of(&app);
        assert!(glass.contains("fill-opacity=\"0.50\""));
        assert!(polygons(&glass) > polygons(&opaque), "both sides are drawn");
        app.surface_batches = Default::default();
        assert_eq!(polygons(&svg_of(&app)), 0);
    }

    #[test]
    fn test_png_export_with_a_custom_portrait_canvas() {
        use crate::canvas::{CanvasAction as C, CanvasShape};
        let mut app = Yui::default();
        let mut state = app.state.clone();
        load_structure_into_state(&mut state, &read_1hls(), "1hls.pdb").unwrap();
        app.state = state;
        app.update_model_atom_ranges();
        let mut canvas = crate::canvas::CanvasOptions::default();
        canvas.apply(C::Shape(CanvasShape::Custom));
        canvas.apply(C::CustomWidth(120));
        canvas.apply(C::CustomHeight(200));
        canvas.apply(C::FitAll(true));
        let ((w, h), camera) = app.export_canvas(&canvas, 2000).unwrap();
        assert_eq!((w, h), (120, 200));
        let primitive = app.offscreen_primitive_with(
            w,
            h,
            false,
            &camera,
            &crate::surface_render::SurfaceBatches::default(),
        );
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let (bytes, pw, ph) =
            match runtime.block_on(crate::viewport::offscreen::render_png(primitive, w, h, 2)) {
                Ok(r) => r,
                Err(e) if e.contains("No graphics adapter") => {
                    eprintln!("png export: skipped ({e})");
                    return;
                }
                Err(e) => panic!("{e}"),
            };
        assert_eq!((pw, ph), (120, 200));
        let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buf).unwrap();
        assert_eq!((info.width, info.height), (120, 200));
        // 「全体に合わせる」ので、構造が画像の左右の端に切れずに収まる（端の列は背景だけ）
        let (pixels, _) = buf[..info.buffer_size()].as_chunks::<4>();
        let corner = pixels[0];
        let edge_differs =
            (0..200).any(|y| pixels[y * 120] != corner || pixels[y * 120 + 119] != corner);
        assert!(
            !edge_differs,
            "the structure touches the left or right edge"
        );
        let center_differs = pixels.iter().any(|p| *p != corner);
        assert!(center_differs, "something is drawn");
    }
}
