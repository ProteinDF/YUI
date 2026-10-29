//! UI の操作の自動テスト（`iced_test` で、画面なしにクリックを再現する）。
//!
//! 実際の画面（`Yui::view`）を作り、文字でウィジェットを探してクリックし、出てきたメッセージをアプリに渡す。
//! メッセージを直接送る単体テストと違い、「ボタンが画面にあり、押せて、押したらダイアログが出る」までを確かめる。
//! 3D の分子図（shader ウィジェット）は、このテスト用の描画器では描かれないが、操作の確認には要らない。

use super::*;
use crate::file_open_dispatch::FileOpenHandler;

fn app_with_structure() -> Yui {
    let mut app = Yui::default();
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/tests/data/1hls.pdb");
    let bytes = std::fs::read(path).expect("read the test structure");
    let _ = app.update(Message::FileContentFetched {
        path: "1hls.pdb".to_string(),
        handler: FileOpenHandler::StructureViewer,
        result: Ok(bytes),
    });
    app
}

/// 画面にある、`text` を含むウィジェットをクリックして、出てきたメッセージをアプリに渡す。
/// 見つからない、または押せないときは `false`。
fn click(app: &mut Yui, text: &str) -> bool {
    let _gpu = crate::test_gpu::lock();
    let mut ui = iced_test::simulator(app.view());
    if ui.click(text).is_err() {
        return false;
    }
    let messages: Vec<Message> = ui.into_messages().collect();
    for m in messages {
        let _ = app.update(m);
    }
    true
}

/// 文字が `text` のウィジェットのうち、深さ優先の順で `n` 番目（1 始まり）を選ぶ
fn nth_text(
    text: &str,
    n: usize,
) -> impl FnMut(iced_test::selector::Candidate<'_>) -> Option<iced_test::selector::Target> + Send {
    let wanted = text.to_string();
    let mut seen = 0usize;
    move |candidate| {
        if let iced_test::selector::Candidate::Text { content, .. } = &candidate
            && *content == wanted
        {
            seen += 1;
            if seen == n {
                return Some(iced_test::selector::Target::from(candidate));
            }
        }
        None
    }
}

/// 画面にある、文字が `text` のウィジェットのうち、`n` 番目をクリックする。
/// 「▶」のように、同じ文字のボタンが画面のあちこちにあるときに使う。
fn click_nth(app: &mut Yui, text: &str, n: usize) -> bool {
    let _gpu = crate::test_gpu::lock();
    let mut ui = iced_test::simulator(app.view());
    if ui.click(nth_text(text, n)).is_err() {
        return false;
    }
    let messages: Vec<Message> = ui.into_messages().collect();
    for m in messages {
        let _ = app.update(m);
    }
    true
}

/// 画面にある、文字が `text` のウィジェットの数
fn count(app: &Yui, text: &str) -> usize {
    let _gpu = crate::test_gpu::lock();
    let mut ui = iced_test::simulator(app.view());
    (1..=200)
        .take_while(|&n| ui.find(nth_text(text, n)).is_ok())
        .count()
}

/// 同じ文字のウィジェットのうち、画面の最後のもの（分子図の下の操作のバー、Right ペインのカードなど）
fn click_last(app: &mut Yui, text: &str) -> bool {
    let n = count(app, text);
    n > 0 && click_nth(app, text, n)
}

/// 画面に、`text` を含むウィジェットがあるか
fn visible(app: &Yui, text: &str) -> bool {
    let _gpu = crate::test_gpu::lock();
    // edition 2024 では、末尾の式の一時値（描画器）が、ロックより先に壊れる
    iced_test::simulator(app.view()).find(text).is_ok()
}

/// 画面（今の `Yui::view`）を、環境変数 `YUI_UI_SHOTS_DIR` があれば、そこに PNG で書く
/// （目で確かめるため。比べはしない。描画器は、テスト用のソフトウェア描画で、3D の分子図は描かれない）。
fn shot(app: &Yui, name: &str) {
    let Ok(dir) = std::env::var("YUI_UI_SHOTS_DIR") else {
        return;
    };
    let _gpu = crate::test_gpu::lock();
    let mut ui = iced_test::simulator(app.view());
    if let Ok(snapshot) = ui.snapshot(&app.theme()) {
        // 既にあれば比べるだけで書き換えないので、実行ごとに消す
        let base = std::path::Path::new(&dir).join(name);
        let _ = std::fs::create_dir_all(&dir);
        let written = base.with_file_name(format!("{name}-{}.png", "tiny-skia"));
        let _ = std::fs::remove_file(&written);
        let _ = snapshot.matches_image(&base);
    }
}

/// 複数のモデルを持つ構造（1HLS.cif）を開いたアプリ
fn app_with_models() -> Yui {
    let mut app = Yui::default();
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/tests/data/1HLS.cif");
    let bytes = std::fs::read(path).expect("read the test structure");
    let _ = app.update(Message::FileContentFetched {
        path: "1HLS.cif".to_string(),
        handler: FileOpenHandler::StructureViewer,
        result: Ok(bytes),
    });
    app
}

fn open_file_menu_item(app: &mut Yui, item: &str) {
    assert!(click(app, "File"), "the File button is on the screen");
    assert!(visible(app, item), "'{item}' is in the open File menu");
    assert!(click(app, item), "'{item}' can be clicked");
}

#[test]
fn test_document_tabs_and_both_themes_render() {
    use crate::ui_style::ThemeKind;
    let mut app = app_with_structure();
    let _ = app.update(Message::MenuAction(MenuAction::FileNewTextFile));
    let _ = app.update(Message::MenuAction(MenuAction::FileNewTextFile));
    for (kind, name) in [
        (ThemeKind::Dark, "document_tabs_dark"),
        (ThemeKind::Light, "document_tabs_light"),
    ] {
        app.theme = kind;
        // ドキュメントのタブには、閉じるボタンがある
        assert!(
            visible(&app, "×"),
            "{name}: the close button of a document tab"
        );
        shot(&app, name);
    }
    // 明るいテーマで、メニューを開いた画面
    assert!(click(&mut app, "File"));
    shot(&app, "file_menu_light");
}

#[test]
fn test_editor_groups_and_the_drop_highlight_render_in_both_themes() {
    use crate::pane_view::{DropZone, ViewId};
    use crate::ui_style::ThemeKind;
    let mut app = app_with_structure();
    let _ = app.update(Message::MenuAction(MenuAction::FileNewTextFile));
    let _ = app.update(Message::MenuAction(MenuAction::FileNewTextFile));
    let docs = app.documents.ids();
    let (first, second) = (ViewId::Document(docs[0]), ViewId::Document(docs[1]));
    // 2 つ目のファイルを、下の端へ落として、上下に分割する
    let _ = app.update(Message::PaneTabSelected {
        pane: PaneKind::CENTER,
        view: second,
    });
    let _ = app.update(Message::TabDropTargetEntered(
        PaneKind::CENTER,
        DropZone::Bottom,
    ));
    let _ = app.update(Message::TabDragEnded);
    let lower = PaneKind::Group(crate::app::GroupId(1));
    assert_eq!(app.pane_views[&lower].views, vec![second]);
    // 1 つ目のファイルを、下のグループの右の端の上でドラッグしている途中（右半分が色付きになる）
    let _ = app.update(Message::PaneTabSelected {
        pane: PaneKind::CENTER,
        view: first,
    });
    let _ = app.update(Message::TabDropTargetEntered(lower, DropZone::Right));
    for (kind, name) in [
        (ThemeKind::Dark, "editor_groups_drag_dark"),
        (ThemeKind::Light, "editor_groups_drag_light"),
    ] {
        app.theme = kind;
        assert!(visible(&app, "Untitled-1"), "{name}");
        assert!(visible(&app, "Untitled-2"), "{name}");
        shot(&app, name);
    }
    app.theme = ThemeKind::Dark;
}

#[test]
fn test_a_dock_tab_moves_to_the_editor_and_the_empty_dock_shows_a_hint() {
    use crate::pane_view::{DropZone, ViewId};
    use crate::ui_style::ThemeKind;
    let mut app = app_with_structure();
    assert!(!visible(&app, "Drag a tab here."));
    // 右のパネルの唯一のタブ（Inspector）を、エディタの本体へ落とす
    let _ = app.update(Message::PaneTabSelected {
        pane: PaneKind::Right,
        view: ViewId::RepresentationInspector,
    });
    let _ = app.update(Message::TabDropTargetEntered(
        PaneKind::CENTER,
        DropZone::Center,
    ));
    let _ = app.update(Message::TabDragEnded);
    assert_eq!(
        app.pane_views[&PaneKind::CENTER].active_view(),
        Some(ViewId::RepresentationInspector)
    );
    assert!(app.pane_views[&PaneKind::Right].views.is_empty());
    assert!(
        visible(&app, "Drag a tab here."),
        "the empty dock shows a hint"
    );
    // 下のパネルの Terminal を、空になった右のパネルへ落とす
    let _ = app.update(Message::PaneTabSelected {
        pane: PaneKind::Bottom,
        view: ViewId::Terminal,
    });
    let _ = app.update(Message::TabDropTargetEntered(
        PaneKind::Right,
        DropZone::Center,
    ));
    let _ = app.update(Message::TabDragEnded);
    assert_eq!(
        app.pane_views[&PaneKind::Right].views,
        vec![ViewId::Terminal]
    );
    assert!(!visible(&app, "Drag a tab here."));
    // もう一度、右を空にして、両方のテーマで撮る
    let _ = app.update(Message::PaneTabSelected {
        pane: PaneKind::Right,
        view: ViewId::Terminal,
    });
    let _ = app.update(Message::TabDropTargetEntered(
        PaneKind::Bottom,
        DropZone::Center,
    ));
    let _ = app.update(Message::TabDragEnded);
    for (kind, name) in [
        (ThemeKind::Dark, "dock_empty_dark"),
        (ThemeKind::Light, "dock_empty_light"),
    ] {
        app.theme = kind;
        assert!(visible(&app, "Drag a tab here."), "{name}");
        shot(&app, name);
    }
    app.theme = ThemeKind::Dark;
}

#[test]
fn test_hide_empty_docks_hides_a_dock_left_without_tabs() {
    use crate::pane_view::{DropZone, ViewId};
    let drag_inspector_to_editor = |app: &mut Yui| {
        let _ = app.update(Message::PaneTabSelected {
            pane: PaneKind::Right,
            view: ViewId::RepresentationInspector,
        });
        let _ = app.update(Message::TabDropTargetEntered(
            PaneKind::CENTER,
            DropZone::Center,
        ));
        let _ = app.update(Message::TabDragEnded);
    };
    // 既定（オフ）: 空になっても残す
    let mut app = Yui::default();
    assert!(!app.hide_empty_docks);
    drag_inspector_to_editor(&mut app);
    assert!(app.show_right);
    assert!(app.panes.iter().any(|(_, k)| *k == PaneKind::Right));

    // オン: 空になったら隠す。View で表示すると、空のパネルと案内が出る
    let mut app = Yui {
        hide_empty_docks: true,
        ..Yui::default()
    };
    drag_inspector_to_editor(&mut app);
    assert!(!app.show_right, "the empty right dock is hidden");
    assert!(!app.panes.iter().any(|(_, k)| *k == PaneKind::Right));
    assert!(app.show_left && app.show_bottom, "the other docks stay");
    let _ = app.update(Message::MenuAction(MenuAction::ViewTogglePane(
        PaneKind::Right,
    )));
    assert!(app.show_right);
    assert!(visible(&app, "Drag a tab here."));
    // タブを戻すと、そのまま表示される
    let _ = app.update(Message::PaneTabSelected {
        pane: PaneKind::CENTER,
        view: ViewId::RepresentationInspector,
    });
    let _ = app.update(Message::TabDropTargetEntered(
        PaneKind::Right,
        DropZone::Center,
    ));
    let _ = app.update(Message::TabDragEnded);
    assert!(app.show_right);
    assert_eq!(
        app.pane_views[&PaneKind::Right].views,
        vec![ViewId::RepresentationInspector]
    );
}

#[test]
fn test_a_dragged_tab_follows_the_mouse() {
    use crate::pane_view::ViewId;
    use crate::ui_style::ThemeKind;
    let mut app = app_with_structure();
    let _ = app.update(Message::CursorMoved([300.0, 64.0]));
    let _ = app.update(Message::PaneTabSelected {
        pane: PaneKind::Bottom,
        view: ViewId::Terminal,
    });
    // 押しただけ、少しだけ動かしただけでは、出さない（クリックで、一瞬だけ出ないように）
    assert_eq!(app.tab_drag_ghost_label(), None);
    let _ = app.update(Message::CursorMoved([302.0, 66.0]));
    assert_eq!(app.tab_drag_ghost_label(), None);
    // 動かすと、マウスと一緒に動く四角形に、タブの名前を出す
    let _ = app.update(Message::CursorMoved([520.0, 300.0]));
    assert_eq!(app.tab_drag_ghost_label().as_deref(), Some("Terminal"));
    for (kind, name) in [
        (ThemeKind::Dark, "tab_drag_ghost_dark"),
        (ThemeKind::Light, "tab_drag_ghost_light"),
    ] {
        app.theme = kind;
        shot(&app, name);
    }
    app.theme = ThemeKind::Dark;
    // 離すと消える
    let _ = app.update(Message::TabDragEnded);
    assert_eq!(app.tab_drag_ghost_label(), None);
}

#[test]
fn test_file_menu_lists_the_export_items_and_disables_them_without_a_structure() {
    let mut app = Yui::default();
    assert!(click(&mut app, "File"));
    for item in [
        "Export Structure... (No structure)",
        "Export Image (SVG)... (No structure)",
        "Export Image (PNG)... (No structure)",
        "Export Animation... (No structure)",
        "Open CUBE (Isosurface)...",
    ] {
        assert!(visible(&app, item), "'{item}' is in the File menu");
    }
    shot(&app, "file_menu_without_structure");
    // 構造がなければ、押しても、ダイアログは開かない
    let _ = click(&mut app, "Export Image (PNG)... (No structure)");
    assert!(!app.png_dialog);
    let _ = click(&mut app, "Export Animation... (No structure)");
    assert!(!app.movie_dialog);

    let mut app = app_with_structure();
    assert!(click(&mut app, "File"));
    shot(&app, "file_menu_with_structure");
    for item in [
        "Export Image (SVG)...",
        "Export Image (PNG)...",
        "Export Animation...",
    ] {
        assert!(visible(&app, item));
    }
}

#[test]
fn test_png_dialog_choices_cancel_and_export_by_clicking() {
    let mut app = app_with_structure();
    open_file_menu_item(&mut app, "Export Image (PNG)...");
    assert!(app.png_dialog);
    shot(&app, "png_dialog_default");
    for label in [
        "Width",
        "Smoothing",
        "Background",
        "Shape",
        "Framing",
        "Export...",
        "Cancel",
    ] {
        assert!(visible(&app, label), "'{label}' is in the PNG dialog");
    }
    // 選択肢をクリックすると、設定が変わる
    assert!(click(&mut app, "4000 px"));
    assert!(click(&mut app, "3x"));
    assert!(click(&mut app, "Transparent"));
    assert!(click(&mut app, "16:9"));
    assert!(click(&mut app, "Fit all"));
    let o = app.png_options;
    assert_eq!((o.width, o.smoothing, o.transparent), (4000, 3, true));
    assert_eq!(o.canvas.shape, crate::canvas::CanvasShape::R16x9);
    assert!(o.canvas.fit_all);
    shot(&app, "png_dialog_changed");
    // Custom にすると、大きさの入力欄が出て、幅の候補は消える
    assert!(click(&mut app, "Custom"));
    assert!(visible(&app, "Size"));
    assert!(
        !visible(&app, "4000 px"),
        "the width choices are hidden for Custom"
    );
    shot(&app, "png_dialog_custom");
    // 取り消すと閉じる。選んだ設定は残る
    assert!(click(&mut app, "Cancel"));
    assert!(!app.png_dialog);
    assert!(!visible(&app, "Transparent"));
    assert_eq!(app.png_options.width, 4000);
    // 開き直すと、前回の選択のまま。「Export...」で閉じて、描画を始める
    open_file_menu_item(&mut app, "Export Image (PNG)...");
    assert!(click(&mut app, "Scene color"));
    assert!(click(&mut app, "3D pane"));
    assert!(click(&mut app, "Export..."));
    assert!(!app.png_dialog);
    assert!(
        app.status_message
            .as_ref()
            .is_some_and(|m| m.text.contains("Choose a file"))
    );
}

#[test]
fn test_svg_dialog_by_clicking() {
    let mut app = app_with_structure();
    open_file_menu_item(&mut app, "Export Image (SVG)...");
    assert!(app.svg_dialog);
    shot(&app, "svg_dialog");
    assert!(click(&mut app, "3200 px"));
    assert!(click(&mut app, "4:3"));
    assert!(click(&mut app, "Fit all"));
    assert_eq!(app.svg_options.width, 3200);
    assert_eq!(
        app.svg_options.canvas.shape,
        crate::canvas::CanvasShape::R4x3
    );
    assert!(click(&mut app, "Cancel"));
    assert!(!app.svg_dialog);
}

#[test]
fn test_share_dialog_creates_lists_and_revokes_invitations() {
    use crate::share_dialog::{CreatedInvite, InviteInfo, InviteRole, ShareAction as A};
    let mut app = Yui::default();
    open_file_menu_item(&mut app, "Share Session...");
    assert!(app.share_dialog.is_some());
    assert!(visible(&app, "Create invitation"));
    // ロールと有効な長さを選ぶ
    assert!(click(&mut app, "Editor"));
    assert!(click(&mut app, "1 day"));
    let st = app.share_dialog.as_ref().unwrap();
    assert_eq!(st.role, InviteRole::Editor);
    assert_eq!(st.lifetime.secs(), 86_400);
    // 作れた（結果は、要求の応答として戻る）。秘密とアドレスが出る
    let _ = app.update(Message::Share(A::Created(Ok(CreatedInvite {
        token: "f".repeat(64),
        id: "abc123def456".into(),
        role: "editor".into(),
        expires_in_secs: 86_400,
    }))));
    let _ = app.update(Message::Share(A::Listed(Ok(vec![InviteInfo {
        id: "abc123def456".into(),
        role: "editor".into(),
        expires_in_secs: 86_390,
    }]))));
    assert!(visible(&app, "Copy link"));
    assert!(visible(&app, "Revoke"));
    assert!(visible(&app, "Active invitations"));
    shot(&app, "share_dialog");
    // 取り消すと、一覧と、いま作った招待の表示が消える
    let _ = app.update(Message::Share(A::Revoked(Ok("abc123def456".into()))));
    assert!(!visible(&app, "Copy link"));
    assert!(!visible(&app, "Revoke"));
    // 失敗（Host の秘密でないとき）は、文が出る
    let _ = app.update(Message::Share(A::Created(Err(
        "403 Forbidden: this action requires the Host role".into(),
    ))));
    assert!(visible(
        &app,
        "403 Forbidden: this action requires the Host role"
    ));
    // 閉じる。閉じたあとに届いた結果は、何も起こさない
    assert!(click(&mut app, "Close"));
    assert!(app.share_dialog.is_none());
    let _ = app.update(Message::Share(A::Listed(Ok(vec![]))));
    assert!(app.share_dialog.is_none());
}

#[test]
fn test_animation_dialog_offers_frames_only_for_a_multi_model_structure() {
    // モデルが 1 つ: 「Frames (1)」は押しても選べない
    let mut app = app_with_structure();
    open_file_menu_item(&mut app, "Export Animation...");
    assert!(app.movie_dialog);
    shot(&app, "animation_dialog_single_model");
    let _ = click(&mut app, "Frames (1)");
    assert_ne!(
        app.movie_options.source,
        crate::movie_export::MovieSource::Playback
    );
    assert!(click(&mut app, "Rock"));
    assert!(click(&mut app, "30 fps"));
    assert!(click(&mut app, "12 s"));
    assert!(click(&mut app, "Frames only"));
    let o = app.movie_options;
    assert_eq!((o.fps, o.seconds, o.make_mp4), (30, 12, false));
    assert!(click(&mut app, "Cancel"));
    assert!(!app.movie_dialog);

    // 20 モデル: 「Frames (20)」を選べて、長さの候補は押せなくなる
    let mut app = app_with_models();
    open_file_menu_item(&mut app, "Export Animation...");
    assert!(click(&mut app, "Frames (20)"));
    assert_eq!(
        app.movie_options.source,
        crate::movie_export::MovieSource::Playback
    );
    shot(&app, "animation_dialog_frames");
    let before = app.movie_options.seconds;
    let _ = click(&mut app, "4 s");
    assert_eq!(
        app.movie_options.seconds, before,
        "the length does not apply to Frames"
    );
    // 「Choose folder...」でフォルダの選択に進む（ダイアログは閉じ、書き出しの仕事ができる）
    assert!(click(&mut app, "Choose folder..."));
    assert!(!app.movie_dialog);
    assert_eq!(
        app.movie
            .as_ref()
            .expect("a job waits for the folder")
            .total,
        20
    );
}

#[test]
fn test_representation_card_controls() {
    let mut app = app_with_structure();
    let before = app.state.representations.len();
    assert!(visible(&app, "+ Add"));
    assert!(click(&mut app, "+ Add"));
    assert_eq!(app.state.representations.len(), before + 1);
    let last = app.state.representations.len() - 1;
    // 種類のボタンで、最後のカードの種類が替わる。Surface と Interaction のときだけ、Opacity が出る
    assert!(!visible(&app, "Opacity:"));
    shot(&app, "representation_cards");
    let find_button = |app: &Yui, label: &str| visible(app, label);
    assert!(find_button(&app, "Surface"));
    // 同じ名前のボタンが複数のカードにあるので、最後のカードだけが対象になる状態にしてから確かめる
    app.state.representations = vec![app.state.representations[last].clone()];
    assert!(click(&mut app, "Surface"));
    assert_eq!(
        app.state.representations[0].kind,
        yui_core::representation::RepresentationKind::Surface
    );
    assert!(
        visible(&app, "Opacity:"),
        "the opacity slider appears for a Surface card"
    );
    shot(&app, "representation_surface_card_with_opacity");
    assert!(click(&mut app, "Cartoon"));
    assert!(!visible(&app, "Opacity:"));
    // 目のアイコン: そのカードだけが、非表示になる
    assert!(click(&mut app, "👁"));
    assert!(!app.state.representations[0].visible);
    assert!(visible(&app, "🚫"));
    assert!(click(&mut app, "🚫"));
    assert!(app.state.representations[0].visible);
    // 削除
    assert!(click(&mut app, "✕"));
    assert!(app.state.representations.is_empty());
}

#[test]
fn test_playback_bar_buttons_move_through_the_models() {
    let mut app = app_with_models();
    assert!(
        visible(&app, "Play"),
        "the playback bar is shown for 20 models"
    );
    shot(&app, "playback_bar");
    assert_eq!(app.state.active_model.as_deref(), Some("model_1"));
    assert!(click_last(&mut app, "▶"));
    assert_eq!(app.state.active_model.as_deref(), Some("model_2"));
    assert!(click(&mut app, "▶|"));
    assert_eq!(app.state.active_model.as_deref(), Some("model_20"));
    assert!(click(&mut app, "|◀"));
    assert_eq!(app.state.active_model.as_deref(), Some("model_1"));
    // 再生と一時停止
    assert!(click(&mut app, "Play"));
    assert!(app.playback.playing);
    assert!(visible(&app, "Pause"));
    assert!(click(&mut app, "Pause"));
    assert!(!app.playback.playing);
}

#[test]
fn test_cube_card_buttons() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../core/tests/data/orbitals.cube"
    ))
    .unwrap();
    let set = std::sync::Arc::new(yui_core::cube_iso::CubeSet::parse(&text).unwrap());
    let mut app = Yui::default();
    let _ = app.update(Message::CubeOpened(Ok(Some((
        "orbitals.cube".into(),
        set,
        None,
    )))));
    // 計算の結果を、同期的に渡す（画面では、非同期で届く）
    let cube = app.cube.as_ref().unwrap();
    let job = crate::cube_view::CubeJob {
        set: cube.set.clone(),
        dataset: 0,
        grid: None,
        isolevel: None,
    };
    let _ = app.update(Message::CubeJobDone(crate::cube_view::run_job(job)));
    assert!(visible(&app, "CUBE"));
    assert!(visible(&app, "Isolevel:"));
    assert!(
        visible(&app, "Opacity:"),
        "the CUBE card has its own opacity slider"
    );
    assert!(visible(&app, "Orbital 1 (1/2)"));
    shot(&app, "cube_card");
    // 次の軌道へ（◀ は、先頭では押せない）
    let _ = click_last(&mut app, "◀");
    assert_eq!(app.state.cube_view.dataset, 0);
    assert!(click_last(&mut app, "▶"));
    assert_eq!(app.state.cube_view.dataset, 1);
    assert!(
        app.cube.as_ref().unwrap().busy,
        "the new orbital is being computed"
    );
    // 閉じる
    assert!(click(&mut app, "✕"));
    assert!(app.cube.is_none());
    assert!(!visible(&app, "Isolevel:"));
}

#[test]
fn test_view_menu_commands_work_by_clicking() {
    let mut app = app_with_structure();
    assert!(click(&mut app, "View"));
    shot(&app, "view_menu");
    // カメラのアニメーションを入れる（View のメニューの項目）
    // チェックの印のある項目は、表示名の先頭に印（なければ空白）が付く
    assert!(visible(&app, "  Spin"), "the View menu has the Spin item");
    assert!(click(&mut app, "  Spin"));
    assert!(app.is_spinning(), "Spin starts");
    // 項目を選んだら、メニューは閉じる（Spin などのコマンドでも）
    assert!(
        app.active_menu.is_none(),
        "the menu closes after choosing an item"
    );
    assert!(!visible(&app, "✓ Spin"));
    // もう一度開くと、印が付いている。押すと止まる
    assert!(click(&mut app, "View"));
    assert!(visible(&app, "✓ Spin"));
    assert!(click(&mut app, "✓ Spin"));
    assert!(!app.is_spinning(), "Spin stops");
}

#[test]
fn test_every_pane_tab_can_be_clicked_at_the_default_window_size() {
    // 左のペインが細くても、タブが折り返して、すべて押せる（以前は「Project」が隠れて押せなかった）
    let mut app = Yui::default();
    for tab in ["Explorer / Playbook", "Named Selections", "Project"] {
        assert!(
            click(&mut app, tab),
            "the left pane tab '{tab}' can be clicked"
        );
    }
    // 選んだタブが、そのペインの表示中のビューになる（選択中は、下線と文字の色で示す）
    assert_eq!(
        app.pane_views
            .get(&PaneKind::Left)
            .and_then(|s| s.active_view()),
        Some(ViewId::ProjectTree),
        "the Project tab is now the active one"
    );
    for tab in ["Python Console", "Terminal", "AI Chat", "Log Viewer"] {
        assert!(
            click(&mut app, tab),
            "the bottom pane tab '{tab}' can be clicked"
        );
    }
    shot(&app, "pane_tabs_at_the_default_size");
}

#[test]
fn test_cancelling_the_save_dialog_does_not_leave_a_progress_message() {
    let mut app = app_with_structure();
    open_file_menu_item(&mut app, "Export Image (PNG)...");
    assert!(click(&mut app, "Export..."));
    assert!(
        app.status_message
            .as_ref()
            .is_some_and(|m| m.text.contains("Choose a file"))
    );
    // 保存のダイアログで取り消した（`Ok(None)`）
    let _ = app.update(Message::ImageExported(Ok(None)));
    assert_eq!(
        app.status_message.as_ref().unwrap().text,
        "Export cancelled."
    );
}

#[test]
fn test_editor_with_scrollbar_shows_and_follows_the_cursor() {
    let mut app = Yui::default();
    let text = (0..300).map(|i| format!("line {i}\n")).collect::<String>();
    let _ = app.update(Message::FileContentFetched {
        path: "long.txt".to_string(),
        handler: FileOpenHandler::TextEditor,
        result: Ok(text.into_bytes()),
    });
    shot(&app, "editor-scrollbar");
    assert!(visible(&app, "long.txt"));
}

#[test]
fn test_open_with_menu_opens_at_the_cursor_and_runs_a_choice() {
    use crate::project_tree_pane::ProjectTreePaneMessage as T;
    let mut app = Yui::default();
    // 左クリックは、構造ファイルをたずねずに開く
    let _ = app.update(Message::ProjectTreePaneMsg(T::FileSelected(
        "a/p.pdb".into(),
    )));
    assert!(app.pending_open_choice.is_none());
    assert!(app.file_context_menu.is_none());
    // 右クリックは、マウスのそばにメニューを出し、開き方を並べる
    let _ = app.update(Message::CursorMoved([320.0, 210.0]));
    let _ = app.update(Message::ProjectTreePaneMsg(T::FileOpenWithRequested(
        "a/p.pdb".into(),
    )));
    assert!(app.pending_open_choice.is_none());
    let (pos, target) = app.file_context_menu.clone().expect("menu");
    assert_eq!(pos, [320.0, 210.0]);
    assert_eq!(target.path, "a/p.pdb");
    assert_eq!(
        target.handlers,
        vec![
            FileOpenHandler::StructureViewer,
            FileOpenHandler::TextEditor
        ]
    );
    assert!(visible(&app, "Open with"));
    assert!(visible(&app, "Structure Viewer"));
    // メニューの外をクリックすると、開かずに閉じる
    let _ = app.update(Message::FileContextMenuClosed);
    assert!(app.file_context_menu.is_none());
    // 候補を選ぶと、メニューが閉じる
    let _ = app.update(Message::ProjectTreePaneMsg(T::FileOpenWithRequested(
        "a/p.pdb".into(),
    )));
    assert!(click(&mut app, "Text Editor"));
    assert!(app.file_context_menu.is_none());
}

#[test]
fn test_terminal_keys_go_to_the_terminal_only_while_it_has_the_keyboard() {
    use iced::keyboard::{Key, Modifiers, key::Named};
    let mut app = Yui::default();
    let enter = || Message::TerminalKey {
        key: Key::Named(Named::Enter),
        modifiers: Modifiers::empty(),
        text: None,
    };
    // 接続していない・画面を選んでいない間は、キーを受け取らない
    assert!(!app.terminal_has_keyboard());
    app.terminal_pane.focused = true;
    app.terminal_pane.connected = true;
    assert!(
        !app.terminal_has_keyboard(),
        "the Terminal tab is not shown"
    );
    let _ = app.update(Message::PaneTabSelected {
        pane: PaneKind::Bottom,
        view: ViewId::Terminal,
    });
    assert!(app.terminal_has_keyboard());
    app.terminal_pane.process_output(
        b"\x1b[32muser@host\x1b[0m:~$ ls\r\nfile1  file2\r\n\x1b[32muser@host\x1b[0m:~$ ",
    );
    shot(&app, "terminal-focused");
    // キーは、端末への入力になる
    let _ = app.update(enter());
    let _ = app.update(Message::TerminalKey {
        key: Key::Character("l".into()),
        modifiers: Modifiers::CTRL,
        text: None,
    });
    assert_eq!(
        app.terminal_pane.drain_pending_inputs(),
        vec![b"\r".to_vec(), vec![0x0c]]
    );
    // ほかのタブにすると、受け取らない
    let _ = app.update(Message::PaneTabSelected {
        pane: PaneKind::Bottom,
        view: ViewId::LogViewer,
    });
    assert!(!app.terminal_has_keyboard());
    // 画面のクリックのあと、ほかをクリックすると、受け取らなくなる
    app.terminal_pane.focused = true;
    let _ = app.update(Message::MousePressed);
    assert!(!app.terminal_pane.focused);
}

#[test]
fn test_terminal_copy_paste_keys_are_not_sent_to_the_shell() {
    use iced::keyboard::{Key, Modifiers};
    let mut app = Yui::default();
    app.terminal_pane.process_output(b"hello world");
    let at = |col: f32| iced::Point::new(6.0 + (col + 0.5) * 7.2, 10.0);
    use crate::terminal_pane::TerminalPaneMessage as T;
    let _ = app.update(Message::TerminalPaneMsg(T::Moved(at(0.0))));
    let _ = app.update(Message::TerminalPaneMsg(T::Clicked));
    let _ = app.update(Message::TerminalPaneMsg(T::Moved(at(4.0))));
    let _ = app.update(Message::TerminalPaneMsg(T::Released));
    assert_eq!(app.terminal_pane.selected_text().as_deref(), Some("hello"));
    shot(&app, "terminal-selection");
    // Ctrl+Shift+C / V は、クリップボードの操作で、端末への入力にはならない
    for c in ["C", "V"] {
        let _ = app.update(Message::TerminalKey {
            key: Key::Character(c.into()),
            modifiers: Modifiers::CTRL | Modifiers::SHIFT,
            text: None,
        });
    }
    assert!(app.terminal_pane.drain_pending_inputs().is_empty());
    // 貼り付ける文字が届くと、端末への入力になる
    let _ = app.update(Message::TerminalPasteText(Some("pwd\n".into())));
    let _ = app.update(Message::TerminalPasteText(None));
    assert_eq!(
        app.terminal_pane.drain_pending_inputs(),
        vec![b"pwd\r".to_vec()]
    );
}

#[test]
fn test_help_mouse_and_keyboard_dialog_lists_the_controls() {
    use crate::ui_style::ThemeKind;
    let mut app = app_with_structure();
    assert!(click(&mut app, "Help"), "the Help button is on the screen");
    assert!(visible(&app, "Mouse & Keyboard..."));
    assert!(
        click(&mut app, "Mouse & Keyboard..."),
        "the item can be clicked"
    );
    assert!(app.show_controls);
    for line in [
        "Mouse & Keyboard",
        "Shift + click",
        "Ctrl (Cmd on macOS) + click",
        "Ctrl+Shift+C / V (Cmd+C / V on macOS)",
        "Mouse wheel, Shift+PageUp / PageDown",
    ] {
        assert!(visible(&app, line), "'{line}' is in the dialog");
    }
    shot(&app, "controls_dialog_dark");
    app.theme = ThemeKind::Light;
    shot(&app, "controls_dialog_light");
    assert!(click(&mut app, "  OK  "), "OK closes the dialog");
    assert!(!app.show_controls);
}

#[test]
fn test_terminal_input_line_can_be_hidden_and_paste_stays() {
    use crate::terminal_pane::TerminalPaneMessage;
    let mut app = Yui::default();
    let _ = app.update(Message::PaneTabSelected {
        pane: PaneKind::Bottom,
        view: ViewId::Terminal,
    });
    app.terminal_pane.process_output(b"user@host:~$ ");
    assert!(visible(&app, "Hide input line"));
    assert!(
        visible(&app, "Send ⏎"),
        "the input line is shown by default"
    );
    let _ = app.update(Message::TerminalPaneMsg(
        TerminalPaneMessage::ToggleInputLine,
    ));
    assert!(visible(&app, "Show input line"));
    assert!(!visible(&app, "Send ⏎"), "the input line is hidden");
    assert!(visible(&app, "Paste"), "the quick keys stay");
    shot(&app, "terminal-input-hidden");
}

#[test]
fn test_terminal_shows_the_composing_text_at_the_cursor() {
    use crate::terminal_pane::TerminalPaneMessage;
    use crate::terminal_widget::ImeEvent;
    let mut app = Yui::default();
    let _ = app.update(Message::PaneTabSelected {
        pane: PaneKind::Bottom,
        view: ViewId::Terminal,
    });
    app.terminal_pane.process_output(b"user@host:~$ ");
    let _ = app.update(Message::TerminalPaneMsg(TerminalPaneMessage::Ime(
        ImeEvent::Preedit("にほんご".into()),
    )));
    assert!(app.terminal_pane.is_composing());
    shot(&app, "terminal-preedit");
}
