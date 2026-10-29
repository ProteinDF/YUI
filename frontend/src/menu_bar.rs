//! トップメニューバー（File / View / Help）および関連ロジック
//!
//! architecture.md 8.2節「メニューバー」および 8.4節「アクションパイプラインの原則」に準拠。
//! UIから独立した純粋関数としてメニュー操作（New / Open / Save / Reset Camera / ペイン表示切替）を定義し、
//! 単体テスト可能とする。

use iced::widget::pane_grid::{Axis, Configuration};
use yui_core::state::{AppState, CameraState};

use crate::PaneKind;

/// メニューのトップレベルカテゴリ
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuCategory {
    File,
    View,
    Help,
}

impl MenuCategory {
    pub fn label(&self) -> &'static str {
        match self {
            MenuCategory::File => "File",
            MenuCategory::View => "View",
            MenuCategory::Help => "Help",
        }
    }
}

/// メニューバーから発行されるアクション（8.4節）
#[derive(Debug, Clone, PartialEq)]
pub enum MenuAction {
    /// メニューカテゴリの開閉トグル
    ToggleCategory(MenuCategory),
    /// メニューを閉じる
    CloseMenu,

    // --- File メニュー ---
    /// 既定の空 AppState にリセット
    FileNew,
    /// YAML 状態ファイルのインポート
    FileOpen,
    /// YAML 状態ファイルへのエクスポート
    FileSave,
    /// 名前を付けて YAML 保存
    FileSaveAs,
    /// 構造ファイル（PDB / mmCIF）のエクスポート
    FileExportStructure,
    /// 表示中のビューを SVG の画像として書き出す
    FileExportImageSvg,
    /// 表示中のビューを PNG の画像として書き出す
    FileExportImagePng,
    /// アニメーションを連番の PNG（と MP4）に書き出す
    FileExportAnimation,
    /// 新しい空のテキストファイルをタブで開く
    FileNewTextFile,
    /// テキストファイルをダイアログで選んでタブで開く
    FileOpenTextFile,
    /// 軌跡ファイル（複数フレームの XYZ）を、表示中の構造の軌跡として開く
    FileOpenTrajectory,
    /// 開いている軌跡を閉じ、構造を軌跡を開く前の座標に戻す
    FileCloseTrajectory,
    /// pytools が書いたグラフの JSON を選んで、下のパネルのタブに出す
    FileOpenPlot,
    /// Gaussian CUBE ファイルを選んで、等値面を表示する
    FileOpenCube,
    /// 開いている CUBE の等値面を閉じる
    FileCloseCube,
    /// 利用者ごとの設定ファイルをタブで開く（ネイティブ版。なければ雛形を作る）
    FileOpenSettings,
    /// 共有（招待の作成・取り消し）のダイアログを開く
    FileShareSession,
    /// アプリケーション終了
    FileQuit,

    // --- View メニュー ---
    /// カメラ視点を既定値にリセット
    ViewResetCamera,
    /// 3Dビューポートの背景色を設定
    ViewSetBackgroundColor(yui_core::state::Color4),
    /// 特定ペインの表示/非表示トグル
    ViewTogglePane(PaneKind),
    /// テキストエディタを中央の右側に分けて表示する／中央ペインのタブに戻す
    ViewToggleEditorSplit,
    /// 2つの分子図の視点の連動のオン・オフ
    ViewToggleCameraLink,
    /// 散布図のタブを、下のパネルで開く（開いていれば選ぶ）
    ViewOpenPlot(crate::pane_view::PlotKind),
    /// UI のテーマ（暗い / 明るい）を選ぶ。利用者ごとの設定に保存する
    ViewSetTheme(crate::ui_style::ThemeKind),
    /// 選択の強調の方法（グロー / ハロー / なし）を選ぶ。利用者ごとの設定に保存する
    ViewSetSelectionHighlight(crate::selection::HighlightStyle),

    // --- Help メニュー ---
    /// About ダイアログの表示切替
    HelpAbout,
    /// About ダイアログを閉じる
    CloseAbout,
    /// 操作の一覧（マウスとキーボード）の表示切替
    HelpControls,
    /// 操作の一覧を閉じる
    CloseControls,
}

/// 既定の空 AppState を生成する純粋関数（File -> New）
pub fn create_empty_state() -> AppState {
    let mut state = AppState {
        atom_group: None,
        ..Default::default()
    };
    state.sync_molecule_from_atom_group();
    state.representations.clear();
    // 空のときの視点は、これまでの固定の視点（分子図は、構造を開くと構造に合わせる）
    state.camera = default_camera_state();
    state
}

use crate::pane_view;

/// YAML 文字列から AppState およびペイン構成を復元する純粋関数（File -> Open）
#[allow(dead_code)]
pub fn import_workspace_from_yaml(
    yaml_str: &str,
) -> Result<(AppState, crate::pane_view::PaneLayout), String> {
    pane_view::import_workspace_from_yaml_str(yaml_str)
}

/// AppState およびペイン構成を YAML 文字列へシリアライズする純粋関数（File -> Save / Save As）
pub fn export_workspace_to_yaml(
    state: &AppState,
    pane_views: &crate::pane_view::PaneLayout,
) -> Result<String, String> {
    pane_view::export_workspace_to_yaml_str(state, pane_views)
}

/// YAML 文字列から AppState を復元する互換関数（旧形式フォールバック対応）
#[allow(dead_code)]
pub fn import_yaml_to_app_state(yaml_str: &str) -> Result<AppState, String> {
    let (state, _) = pane_view::import_workspace_from_yaml_str(yaml_str)?;
    Ok(state)
}

/// AppState を YAML 文字列へシリアライズする互換関数
#[allow(dead_code)]
pub fn export_app_state_to_yaml(state: &AppState) -> Result<String, String> {
    let default_views = pane_view::default_pane_views();
    pane_view::export_workspace_to_yaml_str(state, &default_views)
}

/// 既定の CameraState を取得する純粋関数（View -> Reset Camera）
pub fn default_camera_state() -> CameraState {
    CameraState {
        eye: [24.0, 12.0, 95.0],
        target: [24.0, 12.0, 20.0],
        up: [0.0, 1.0, 0.0],
        fov: 45.0,
    }
}

/// エディタのグループの木から、中央の領域の `pane_grid::Configuration` を作る
fn editor_configuration(node: &pane_view::GroupNode) -> Configuration<PaneKind> {
    match node {
        pane_view::GroupNode::Group(id) => Configuration::Pane(PaneKind::Group(*id)),
        pane_view::GroupNode::Split { axis, ratio, a, b } => Configuration::Split {
            axis: *axis,
            ratio: *ratio,
            a: Box::new(editor_configuration(a)),
            b: Box::new(editor_configuration(b)),
        },
    }
}

/// ペインの表示/非表示フラグと、エディタのグループの木から `pane_grid::Configuration` を構築する純粋関数（View -> TogglePane）
pub fn build_pane_configuration(
    show_left: bool,
    show_right: bool,
    show_bottom: bool,
    editor: &pane_view::GroupNode,
) -> Configuration<PaneKind> {
    // センター領域（エディタのグループ。3D Viewport を核とし、Left/Right を結合）。
    // テキストエディタを分割表示するときは、グループが左右に 2 つになる。
    let center_pane = editor_configuration(editor);

    let middle_row = match (show_left, show_right) {
        (true, true) => Configuration::Split {
            axis: Axis::Vertical,
            ratio: 0.2,
            a: Box::new(Configuration::Pane(PaneKind::Left)),
            b: Box::new(Configuration::Split {
                axis: Axis::Vertical,
                ratio: 0.75,
                a: Box::new(center_pane),
                b: Box::new(Configuration::Pane(PaneKind::Right)),
            }),
        },
        (true, false) => Configuration::Split {
            axis: Axis::Vertical,
            ratio: 0.2,
            a: Box::new(Configuration::Pane(PaneKind::Left)),
            b: Box::new(center_pane),
        },
        (false, true) => Configuration::Split {
            axis: Axis::Vertical,
            ratio: 0.8,
            a: Box::new(center_pane),
            b: Box::new(Configuration::Pane(PaneKind::Right)),
        },
        (false, false) => center_pane,
    };

    // Bottom ペインとの結合
    if show_bottom {
        Configuration::Split {
            axis: Axis::Horizontal,
            ratio: 0.75,
            a: Box::new(middle_row),
            b: Box::new(Configuration::Pane(PaneKind::Bottom)),
        }
    } else {
        middle_row
    }
}

/// 分割の比率。分割の両側（左・上と、右・下）にあるペインの組で、どの分割かを表す（`split_key`）。
pub type SplitRatio = (Vec<PaneKind>, Vec<PaneKind>, f32);

/// 分割を表す、両側のペインの組。ペインの順は並べ替える。
///
/// ドック（Left / Right / Bottom）を含む分割では、エディタのグループをまとめて `CENTER` の 1 つとして扱う。
/// エディタを分割したり、グループを消したりしても、ドックの境界は同じ分割として見つかるようにするため。
/// エディタの中の分割（両側がグループだけ）は、グループの組そのままで表す。
pub fn split_key(mut a: Vec<PaneKind>, mut b: Vec<PaneKind>) -> (Vec<PaneKind>, Vec<PaneKind>) {
    let has_dock = a
        .iter()
        .chain(b.iter())
        .any(|k| !matches!(k, PaneKind::Group(_)));
    for side in [&mut a, &mut b] {
        if has_dock {
            for k in side.iter_mut() {
                if matches!(k, PaneKind::Group(_)) {
                    *k = PaneKind::CENTER;
                }
            }
        }
        side.sort();
        side.dedup();
    }
    (a, b)
}

/// 今の `pane_grid` の分割の比率を集める（利用者が境界をドラッグした大きさを、作り直しのあとへ引き継ぐため）
pub fn grid_split_ratios(
    node: &iced::widget::pane_grid::Node,
    kind_of: &impl Fn(iced::widget::pane_grid::Pane) -> Option<PaneKind>,
) -> Vec<SplitRatio> {
    use iced::widget::pane_grid::Node;
    fn kinds(
        node: &Node,
        kind_of: &impl Fn(iced::widget::pane_grid::Pane) -> Option<PaneKind>,
    ) -> Vec<PaneKind> {
        match node {
            Node::Pane(pane) => kind_of(*pane).into_iter().collect(),
            Node::Split { a, b, .. } => {
                let mut v = kinds(a, kind_of);
                v.extend(kinds(b, kind_of));
                v
            }
        }
    }
    match node {
        Node::Pane(_) => Vec::new(),
        Node::Split { ratio, a, b, .. } => {
            let (ka, kb) = split_key(kinds(a, kind_of), kinds(b, kind_of));
            let mut out = vec![(ka, kb, *ratio)];
            out.extend(grid_split_ratios(a, kind_of));
            out.extend(grid_split_ratios(b, kind_of));
            out
        }
    }
}

/// 両側のペインの組が同じ分割の比率を引く（組は `split_key` で揃える）
pub fn find_split_ratio(ratios: &[SplitRatio], a: &[PaneKind], b: &[PaneKind]) -> Option<f32> {
    let (a, b) = split_key(a.to_vec(), b.to_vec());
    ratios
        .iter()
        .find(|(ra, rb, _)| *ra == a && *rb == b)
        .map(|(_, _, r)| *r)
}

/// 構成の分割のうち、両側のペインの組が `ratios` にあるものは、その比率にする（ほかは、構成の比率のまま）
pub fn with_split_ratios(
    config: Configuration<PaneKind>,
    ratios: &[SplitRatio],
) -> Configuration<PaneKind> {
    fn kinds(config: &Configuration<PaneKind>, out: &mut Vec<PaneKind>) {
        match config {
            Configuration::Pane(k) => out.push(*k),
            Configuration::Split { a, b, .. } => {
                kinds(a, out);
                kinds(b, out);
            }
        }
    }
    match config {
        Configuration::Pane(k) => Configuration::Pane(k),
        Configuration::Split { axis, ratio, a, b } => {
            let (mut ka, mut kb) = (Vec::new(), Vec::new());
            kinds(&a, &mut ka);
            kinds(&b, &mut kb);
            Configuration::Split {
                axis,
                ratio: find_split_ratio(ratios, &ka, &kb).unwrap_or(ratio),
                a: Box::new(with_split_ratios(*a, ratios)),
                b: Box::new(with_split_ratios(*b, ratios)),
            }
        }
    }
}

use crate::ui_style;
use iced::widget::{button, column, container, mouse_area, row, text};
use iced::{Alignment, Element, Fill, Size};

/// メニューバーのボタン（File / View / Help）の幅。幅を固定するので、ドロップダウンの位置が計算で求まる。
const MENU_BUTTON_WIDTH: f32 = 52.0;
/// メニューバーのボタンの間隔
const MENU_BUTTON_SPACING: f32 = 2.0;
/// メニューバーの帯の左右の余白
const MENU_BAR_PADDING_X: f32 = 4.0;
/// ドロップダウンの幅
const DROPDOWN_WIDTH: f32 = 220.0;

const CATEGORIES: [MenuCategory; 3] = [MenuCategory::File, MenuCategory::View, MenuCategory::Help];

/// ドロップダウンの左端の位置（そのカテゴリのボタンの左端に合わせる）
pub fn dropdown_left(category: MenuCategory) -> f32 {
    let index = CATEGORIES
        .iter()
        .position(|c| *c == category)
        .unwrap_or_default();
    MENU_BAR_PADDING_X + index as f32 * (MENU_BUTTON_WIDTH + MENU_BUTTON_SPACING)
}

/// メニューバー上部ウィジェットの描画（File/View/Helpボタン）
pub fn view_menu_bar<'a, Message: 'a + Clone>(
    active_menu: Option<MenuCategory>,
    on_action: impl Fn(MenuAction) -> Message + 'a + Copy,
) -> Element<'a, Message> {
    let palette = crate::ui_style::palette();
    let mut menu_buttons = row![]
        .spacing(MENU_BUTTON_SPACING)
        .align_y(Alignment::Center);

    for cat in CATEGORIES {
        let is_active = active_menu == Some(cat);
        // 開いているメニューは、括弧ではなくボタンの背景の色で示す
        let btn = button(text(cat.label()).size(12).center().width(Fill))
            .on_press(on_action(MenuAction::ToggleCategory(cat)))
            .width(MENU_BUTTON_WIDTH)
            .padding([3, 0])
            .style(move |_theme, status| {
                ui_style::menu_bar_button_style(&palette, is_active, status)
            });

        menu_buttons = menu_buttons.push(btn);
    }

    let bar = container(menu_buttons)
        .width(Fill)
        .padding([2.0, MENU_BAR_PADDING_X])
        .style(move |_theme| ui_style::menu_bar_style(&palette));
    // 帯の下に、細い区切り線（高さは `placement::MENU_BAR_HEIGHT` に含める）
    column![bar, ui_style::menu_separator(palette)].into()
}

/// ドロップダウンメニュー（アクティブなカテゴリの項目を、メニューモデルから描く）
///
/// ボタンの真下に開き、ウィンドウが低いときは高さを制限してスクロールにする（`menu::placement`）。
pub fn view_dropdown<'a, Message: 'a + Clone>(
    category: MenuCategory,
    items: &[crate::menu::MenuItem],
    window: Size,
    on_command: impl Fn(crate::menu::CommandInvocation) -> Message + 'a + Copy,
    on_close: Message,
) -> Element<'a, Message> {
    crate::menu::popup_menu(
        items,
        crate::menu::Surface::Dropdown,
        DROPDOWN_WIDTH,
        crate::menu::Anchor::Dropdown {
            left: dropdown_left(category),
        },
        window,
        on_command,
        on_close,
    )
}

/// ビルドの識別子（短いハッシュ。作業ツリーに変更があれば `(modified)` を付ける）。git がないビルドは `unknown`。
pub fn build_id_text(hash: &str, dirty: bool) -> String {
    match (hash, dirty) {
        ("unknown", _) => "unknown".to_string(),
        (hash, true) => format!("{hash} (modified)"),
        (hash, false) => hash.to_string(),
    }
}

/// About ダイアログ（Help -> About）
pub fn view_about_modal<'a, Message: 'a + Clone>(on_close: Message) -> Element<'a, Message> {
    let title = text("YUI").size(22);
    let version = text(format!("Version: {}", env!("CARGO_PKG_VERSION"))).size(13);
    let build = text(format!(
        "Build: {}",
        build_id_text(env!("YUI_GIT_HASH"), env!("YUI_GIT_DIRTY") == "1")
    ))
    .size(12);
    let calver = text("Release Scheme: CalVer (YYYY.M.PATCH)").size(12);
    let license = text("License: GPLv3 (GNU General Public License v3)").size(12);
    let desc =
        text("Integrated visualization platform for canonical molecular orbital calculations.")
            .size(12);
    let palette = crate::ui_style::palette();
    let ok_btn = button(text("  OK  ").size(13))
        .on_press(on_close)
        .padding([4, 16])
        .style(move |_theme, status| ui_style::accent_button_style(&palette, status));

    let content = column![
        title,
        iced::widget::Space::new().height(6),
        version,
        build,
        calver,
        license,
        iced::widget::Space::new().height(6),
        desc,
        iced::widget::Space::new().height(14),
        ok_btn,
    ]
    .align_x(Alignment::Center)
    .spacing(4);

    let modal_card = container(content)
        .width(420)
        .padding(20)
        .style(move |_theme| ui_style::menu_card_style(&palette));

    // 背景を少し暗くして、モーダルらしく見せる（背面のクリックは受け取るだけで何もしない）
    let backdrop = mouse_area(
        container(iced::widget::Space::new().width(Fill).height(Fill))
            .width(Fill)
            .height(Fill)
            .style(move |_theme| container::Style {
                background: Some(palette.scrim.into()),
                ..container::Style::default()
            }),
    );

    let centered = container(modal_card)
        .width(Fill)
        .height(Fill)
        .center_x(Fill)
        .center_y(Fill);

    iced::widget::stack![backdrop, centered].into()
}

/// 操作の一覧（Help -> Mouse & Keyboard）。README の「操作」の節と同じ内容に保つ。
pub const CONTROL_SECTIONS: &[(&str, &[(&str, &str)])] = &[
    (
        "3D Viewport: mouse",
        &[
            ("Left drag", "Orbit around the center"),
            ("Shift + left drag", "Pan"),
            (
                "Ctrl (Cmd on macOS) + left drag",
                "Zoom (drag down to zoom out)",
            ),
            ("Mouse wheel", "Zoom"),
            ("Left click on an atom", "Select only that atom"),
            ("Shift + click", "Add the atom to the selection"),
            (
                "Ctrl (Cmd on macOS) + click",
                "Toggle the atom in the selection",
            ),
            ("Double-click an atom", "Select its whole residue"),
            (
                "Click empty space",
                "Clear the selection (Shift / Ctrl keep it)",
            ),
            ("Right click", "Context menu"),
        ],
    ),
    (
        "Keyboard",
        &[
            ("Space", "Play / pause"),
            ("Left / Right arrow", "Previous / next frame"),
            ("Esc", "Clear the selection"),
            ("Ctrl+S (Cmd+S on macOS)", "Save the open text file"),
        ],
    ),
    (
        "Terminal",
        &[
            ("Click the screen", "Type into the terminal (cyan border)"),
            ("Ctrl+C", "Interrupt the running program"),
            ("Ctrl+Shift+C / V (Cmd+C / V on macOS)", "Copy / paste"),
            ("Shift+Insert", "Paste"),
            ("Mouse wheel, Shift+PageUp / PageDown", "Scroll the history"),
        ],
    ),
];

/// 操作の一覧のダイアログ（Help -> Mouse & Keyboard）
pub fn view_controls_modal<'a, Message: 'a + Clone>(on_close: Message) -> Element<'a, Message> {
    let palette = crate::ui_style::palette();
    let mut sections = column![].spacing(10);
    for (heading, rows) in CONTROL_SECTIONS {
        let mut lines = column![text(*heading).size(13).color(palette.text_secondary)].spacing(4);
        for (keys, action) in rows.iter() {
            lines = lines.push(
                row![
                    text(*keys)
                        .size(12)
                        .color(palette.text)
                        .width(iced::Length::Fixed(250.0)),
                    text(*action).size(12).color(palette.text_secondary),
                ]
                .spacing(10),
            );
        }
        sections = sections.push(lines);
    }
    let ok_btn = button(text("  OK  ").size(13))
        .on_press(on_close)
        .padding([4, 16])
        .style(move |_theme, status| ui_style::accent_button_style(&palette, status));

    let content = column![
        text("Mouse & Keyboard").size(20),
        iced::widget::Space::new().height(10),
        iced::widget::scrollable(container(sections).padding([0, 14]))
            .height(iced::Length::Fixed(440.0)),
        iced::widget::Space::new().height(14),
        container(ok_btn).width(Fill).center_x(Fill),
    ]
    .spacing(4);

    let modal_card = container(content)
        .width(560)
        .padding(20)
        .style(move |_theme| ui_style::menu_card_style(&palette));

    let backdrop = mouse_area(
        container(iced::widget::Space::new().width(Fill).height(Fill))
            .width(Fill)
            .height(Fill)
            .style(move |_theme| container::Style {
                background: Some(palette.scrim.into()),
                ..container::Style::default()
            }),
    );

    let centered = container(modal_card)
        .width(Fill)
        .height(Fill)
        .center_x(Fill)
        .center_y(Fill);

    iced::widget::stack![backdrop, centered].into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use yui_core::AtomGroupExt;

    #[test]
    fn test_build_id_shows_the_hash_and_marks_local_changes() {
        assert_eq!(build_id_text("1a2b3c4", false), "1a2b3c4");
        assert_eq!(build_id_text("1a2b3c4", true), "1a2b3c4 (modified)");
        assert_eq!(build_id_text("unknown", true), "unknown");
    }

    #[test]
    fn dropdown_is_placed_under_its_own_button() {
        let lefts: Vec<f32> = CATEGORIES.iter().map(|c| dropdown_left(*c)).collect();
        assert_eq!(lefts[0], MENU_BAR_PADDING_X);
        // 隣のボタンとは、ボタンの幅と間隔だけ離れる
        assert_eq!(lefts[1] - lefts[0], MENU_BUTTON_WIDTH + MENU_BUTTON_SPACING);
        assert_eq!(lefts[2] - lefts[1], MENU_BUTTON_WIDTH + MENU_BUTTON_SPACING);
    }

    #[test]
    fn test_create_empty_state() {
        let state = create_empty_state();
        assert!(state.atom_group.is_none());
        assert!(state.molecule.atoms.is_empty());
        assert!(state.molecule.bonds.is_empty());
        assert!(state.representations.is_empty());
    }

    #[test]
    fn test_yaml_export_and_import_round_trip() {
        let mut original = AppState::default();
        let (peptide, sec_map) = yui_core::AtomGroup::create_test_peptide();
        original.atom_group = Some(peptide);
        original.secondary_structure_map = sec_map;
        original.sync_molecule_from_atom_group();
        original.scene.background_color = [0.2, 0.3, 0.4, 1.0];

        let yaml = export_app_state_to_yaml(&original).expect("Export should succeed");
        assert!(yaml.contains("atom_group"));
        assert!(yaml.contains("scene:"));
        assert!(yaml.contains("background_color:"));

        let imported = import_yaml_to_app_state(&yaml).expect("Import should succeed");
        assert!(imported.atom_group.is_some());
        assert_eq!(imported.molecule.atoms.len(), original.molecule.atoms.len());
        assert_eq!(imported.scene.background_color, [0.2, 0.3, 0.4, 1.0]);
    }

    #[test]
    fn test_default_camera_state() {
        let cam = default_camera_state();
        assert_eq!(cam.fov, 45.0);
        assert_eq!(cam.up, [0.0, 1.0, 0.0]);
        assert_eq!(cam.eye, [24.0, 12.0, 95.0]);
        assert_eq!(cam.target, [24.0, 12.0, 20.0]);
    }

    /// エディタのグループの木（分割なし、または左右分割）
    fn editor(split: bool) -> crate::pane_view::GroupNode {
        let mut layout = crate::pane_view::default_pane_views();
        crate::pane_view::set_editor_split(&mut layout, split);
        layout.editor().clone()
    }

    #[test]
    fn test_split_ratios_carry_over_to_a_new_configuration() {
        use iced::widget::pane_grid;
        // 利用者が、下のペインの境界と、左のペインの境界を動かした
        let mut state = pane_grid::State::with_configuration(build_pane_configuration(
            true,
            true,
            true,
            &editor(false),
        ));
        let splits: Vec<_> = state.layout().splits().copied().collect();
        for split in &splits {
            state.resize(*split, 0.4);
        }
        let ratios = grid_split_ratios(state.layout(), &|p| state.get(p).copied());
        assert_eq!(ratios.len(), 3);
        // エディタを分割した構成でも、ドックの境界の大きさは保つ（エディタの新しい分割は、構成の比率）
        let config = with_split_ratios(
            build_pane_configuration(true, true, true, &editor(true)),
            &ratios,
        );
        let fresh = pane_grid::State::with_configuration(config);
        let new_ratios = grid_split_ratios(fresh.layout(), &|p| fresh.get(p).copied());
        let docks_and_editor = [
            PaneKind::Left,
            PaneKind::CENTER,
            PaneKind::Right,
            PaneKind::CENTER_RIGHT,
        ];
        assert_eq!(
            find_split_ratio(&new_ratios, &docks_and_editor, &[PaneKind::Bottom]),
            Some(0.4)
        );
        assert_eq!(
            find_split_ratio(&new_ratios, &[PaneKind::Left], &docks_and_editor[1..]),
            Some(0.4)
        );
        assert_eq!(
            find_split_ratio(
                &new_ratios,
                &[PaneKind::CENTER, PaneKind::CENTER_RIGHT],
                &[PaneKind::Right]
            ),
            Some(0.4)
        );
        // エディタの新しい分割は、構成の比率
        assert_eq!(
            find_split_ratio(&new_ratios, &[PaneKind::CENTER], &[PaneKind::CENTER_RIGHT]),
            Some(0.5)
        );
    }

    #[test]
    fn test_build_pane_configuration_variations() {
        // 4ペイン全表示
        let cfg_all = build_pane_configuration(true, true, true, &editor(false));
        match cfg_all {
            Configuration::Split { axis, .. } => assert_eq!(axis, Axis::Horizontal),
            _ => panic!("Expected Split"),
        }

        // Bottom非表示
        let cfg_no_bottom = build_pane_configuration(true, true, false, &editor(false));
        match cfg_no_bottom {
            Configuration::Split { axis, .. } => assert_eq!(axis, Axis::Vertical),
            _ => panic!("Expected Split"),
        }

        // Centerのみ（全サイドペイン非表示）
        let cfg_center_only = build_pane_configuration(false, false, false, &editor(false));
        match cfg_center_only {
            Configuration::Pane(kind) => assert_eq!(kind, PaneKind::CENTER),
            _ => panic!("Expected single Pane"),
        }
    }

    /// 配置に含まれるペインを、左から右・上から下の順に集める
    fn collect_panes(cfg: &Configuration<PaneKind>, out: &mut Vec<PaneKind>) {
        match cfg {
            Configuration::Pane(kind) => out.push(*kind),
            Configuration::Split { a, b, .. } => {
                collect_panes(a, out);
                collect_panes(b, out);
            }
        }
    }

    #[test]
    fn test_build_pane_configuration_with_editor_split() {
        // すべての表示・非表示と分割のオン・オフの組み合わせ
        for show_left in [true, false] {
            for show_right in [true, false] {
                for show_bottom in [true, false] {
                    for split in [true, false] {
                        let cfg = build_pane_configuration(
                            show_left,
                            show_right,
                            show_bottom,
                            &editor(split),
                        );
                        let mut panes = Vec::new();
                        collect_panes(&cfg, &mut panes);

                        let mut expected = Vec::new();
                        if show_left {
                            expected.push(PaneKind::Left);
                        }
                        expected.push(PaneKind::CENTER);
                        if split {
                            expected.push(PaneKind::CENTER_RIGHT);
                        }
                        if show_right {
                            expected.push(PaneKind::Right);
                        }
                        if show_bottom {
                            expected.push(PaneKind::Bottom);
                        }
                        assert_eq!(
                            panes, expected,
                            "left={show_left} right={show_right} bottom={show_bottom} split={split}"
                        );
                    }
                }
            }
        }

        // 分割ありで中央だけのとき、中央の領域が左右（縦の境界）に分かれる
        match build_pane_configuration(false, false, false, &editor(true)) {
            Configuration::Split { axis, a, b, .. } => {
                assert_eq!(axis, Axis::Vertical);
                assert!(matches!(*a, Configuration::Pane(PaneKind::CENTER)));
                assert!(matches!(*b, Configuration::Pane(PaneKind::CENTER_RIGHT)));
            }
            _ => panic!("Expected Split"),
        }
    }
}
