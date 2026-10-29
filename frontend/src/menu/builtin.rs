//! 組み込みのコマンドと、組み込みのメニュー（File / View / Help、右クリックのメニュー 3 つ）。
//!
//! コマンドは既存の操作（`MenuAction` / `ContextMenuAction` / `TreeContextMenuAction` / `RepContextMenuAction`）に
//! 橋渡しするだけで、処理の中身（`app/update.rs` の `handle_*`）は変えない。

use serde::Deserialize;
use serde_json::json;
use yui_core::{ColorScheme, RepresentationKind};

use super::command::{CommandInvocation, CommandRegistry, parse_args};
use super::model::*;
use crate::PaneKind;
use crate::app::Message;
use crate::context_menu::ContextMenuAction;
use crate::menu_bar::MenuAction;
use crate::representation_inspector::RepContextMenuAction;
use crate::structure_tree::TreeContextMenuAction;

#[derive(Deserialize)]
struct BackgroundArgs {
    color: [f32; 4],
}
#[derive(Deserialize)]
struct PlotArgs {
    kind: String,
}
#[derive(Deserialize)]
struct ThemeArgs {
    theme: String,
}
#[derive(Deserialize)]
struct HighlightArgs {
    style: String,
}
#[derive(Deserialize)]
struct LoopArgs {
    mode: String,
}
#[derive(Deserialize)]
struct PaneArgs {
    pane: PaneKind,
}
#[derive(Deserialize)]
struct AtomArgs {
    atom_id: usize,
}
#[derive(Deserialize)]
struct ResidueArgs {
    chain_id: String,
    residue_id: i32,
}
#[derive(Deserialize)]
struct AtomKindArgs {
    atom_id: usize,
    kind: RepresentationKind,
}
#[derive(Deserialize)]
struct AtomSchemeArgs {
    atom_id: usize,
    scheme: ColorScheme,
}
#[derive(Deserialize)]
struct KindArgs {
    kind: RepresentationKind,
}
#[derive(Deserialize)]
struct SchemeArgs {
    scheme: ColorScheme,
}
#[derive(Deserialize)]
struct OpenWithArgs {
    path: String,
    handler: String,
}
#[derive(Deserialize)]
struct IndexArgs {
    index: usize,
}
#[derive(Deserialize)]
struct IndexKindArgs {
    index: usize,
    kind: RepresentationKind,
}
#[derive(Deserialize)]
struct IndexSchemeArgs {
    index: usize,
    scheme: ColorScheme,
}

fn menu(action: MenuAction) -> impl Fn() -> Message {
    move || Message::MenuAction(action.clone())
}

/// 組み込みのコマンドを登録する
///
/// | 識別子 | 引数 | 橋渡し先 |
/// | --- | --- | --- |
/// | `file.new` `file.open` `file.save` `file.save_as` `file.export_structure` `file.export_image_svg` `file.export_image_png` `file.export_animation` | なし | `MenuAction::FileNew` など |
/// | `file.new_text_file` `file.open_text_file` `file.open_settings` `file.share_session` `file.quit` | なし | `MenuAction::FileNewTextFile` など |
/// | `file.open_trajectory` `file.close_trajectory` | なし | `MenuAction::FileOpenTrajectory` / `FileCloseTrajectory` |
/// | `file.open_cube` `file.close_cube` | なし | `MenuAction::FileOpenCube` / `FileCloseCube` |
/// | `view.reset_camera` `view.toggle_editor_split` `view.toggle_camera_link` | なし | `MenuAction::ViewResetCamera` など |
/// | `view.set_background_color` | `color: [f32; 4]` | `MenuAction::ViewSetBackgroundColor` |
/// | `view.set_theme` | `theme: "dark" \| "light"` | `MenuAction::ViewSetTheme` |
/// | `view.set_selection_highlight` | `style: "glow" \| "halo" \| "off"` | `MenuAction::ViewSetSelectionHighlight` |
/// | `playback.play_pause` `playback.first` `playback.prev` `playback.next` `playback.last` | なし | `Message::PlaybackControl(PlaybackAction::*)` |
/// | `playback.set_loop` | `mode: "once" \| "repeat" \| "ping_pong"` | `Message::PlaybackControl(PlaybackAction::SetLoopMode)` |
/// | `camera.toggle_spin` `camera.toggle_rock` `camera.cycle_spin_speed` `camera.zoom_to_selection` | なし | `Message::CameraAction(CameraAction::*)` |
/// | `selection.clear` `selection.save_as_named` | なし | `Message::ClearSelection` / `Message::SaveSelectionAsNamed` |
/// | `view.toggle_pane` | `pane: PaneKind` | `MenuAction::ViewTogglePane` |
/// | `view.open_plot` | `kind: "atom_positions"` | `MenuAction::ViewOpenPlot` |
/// | `help.about` | なし | `MenuAction::HelpAbout` |
/// | `viewport.select_atom` `viewport.hide_atom` `viewport.save_atom_as_named_selection` | `atom_id` | `ContextMenuAction::SelectAtom` など |
/// | `viewport.select_residue` `viewport.save_residue_as_named_selection` | `chain_id`, `residue_id` | `ContextMenuAction::SelectResidue` など |
/// | `viewport.show_as` / `viewport.color_by` | `atom_id`, `kind` / `scheme` | `ContextMenuAction::ShowAs` / `ColorBy` |
/// | `tree.select` `tree.hide` `tree.save_as_named_selection` | なし | `TreeContextMenuAction::*` |
/// | `tree.show_as` / `tree.color_by` | `kind` / `scheme` | `TreeContextMenuAction::ShowAs` / `ColorBy` |
/// | `representation.toggle_visibility` `representation.delete` | `index` | `RepContextMenuAction::*` |
/// | `file.open_with` | `path`, `handler: "text_editor" \| "structure_viewer" \| "workspace_state" \| "trajectory"` | `Message::OpenFileWith` |
/// | `representation.show_as` / `representation.color_by` | `index`, `kind` / `scheme` | `RepContextMenuAction::ShowAs` / `ColorBy` |
pub fn register_menu_commands(reg: &mut CommandRegistry) {
    for (id, action) in [
        ("file.new", MenuAction::FileNew),
        ("file.open", MenuAction::FileOpen),
        ("file.save", MenuAction::FileSave),
        ("file.save_as", MenuAction::FileSaveAs),
        ("file.export_structure", MenuAction::FileExportStructure),
        ("file.export_image_svg", MenuAction::FileExportImageSvg),
        ("file.export_image_png", MenuAction::FileExportImagePng),
        ("file.export_animation", MenuAction::FileExportAnimation),
        ("file.new_text_file", MenuAction::FileNewTextFile),
        ("file.open_text_file", MenuAction::FileOpenTextFile),
        ("file.open_trajectory", MenuAction::FileOpenTrajectory),
        ("file.close_trajectory", MenuAction::FileCloseTrajectory),
        ("file.open_cube", MenuAction::FileOpenCube),
        ("file.close_cube", MenuAction::FileCloseCube),
        ("file.open_settings", MenuAction::FileOpenSettings),
        ("file.share_session", MenuAction::FileShareSession),
        ("file.quit", MenuAction::FileQuit),
        ("view.reset_camera", MenuAction::ViewResetCamera),
        (
            "view.toggle_editor_split",
            MenuAction::ViewToggleEditorSplit,
        ),
        ("view.toggle_camera_link", MenuAction::ViewToggleCameraLink),
        ("help.about", MenuAction::HelpAbout),
        ("help.controls", MenuAction::HelpControls),
    ] {
        reg.register_plain(id, menu(action));
    }
    reg.register("view.set_background_color", |args| {
        let a: BackgroundArgs = parse_args(args)?;
        Ok(Message::MenuAction(MenuAction::ViewSetBackgroundColor(
            a.color,
        )))
    });
    reg.register("view.open_plot", |args| {
        let a: PlotArgs = parse_args(args)?;
        let kind = crate::pane_view::PlotKind::parse(&a.kind)
            .ok_or_else(|| format!("unknown plot '{}'", a.kind))?;
        Ok(Message::MenuAction(MenuAction::ViewOpenPlot(kind)))
    });
    reg.register("view.set_theme", |args| {
        let a: ThemeArgs = parse_args(args)?;
        let kind = crate::ui_style::ThemeKind::parse(&a.theme)
            .ok_or_else(|| format!("unknown theme '{}'", a.theme))?;
        Ok(Message::MenuAction(MenuAction::ViewSetTheme(kind)))
    });
    reg.register("view.set_selection_highlight", |args| {
        let a: HighlightArgs = parse_args(args)?;
        let style = crate::selection::HighlightStyle::parse(&a.style)
            .ok_or_else(|| format!("unknown highlight style '{}'", a.style))?;
        Ok(Message::MenuAction(MenuAction::ViewSetSelectionHighlight(
            style,
        )))
    });
    {
        use crate::playback::PlaybackAction as P;
        let mut play = |id: &str, action: P| {
            reg.register_plain(id, move || Message::PlaybackControl(action));
        };
        play("playback.play_pause", P::PlayPause);
        play("playback.first", P::First);
        play("playback.prev", P::Prev);
        play("playback.next", P::Next);
        play("playback.last", P::Last);
    }
    {
        use crate::camera_anim::CameraAction as C;
        let mut camera = |id: &str, action: C| {
            reg.register_plain(id, move || Message::CameraAction(action));
        };
        camera("camera.toggle_spin", C::ToggleSpin);
        camera("camera.toggle_rock", C::ToggleRock);
        camera("camera.cycle_spin_speed", C::CycleSpinSpeed);
        camera("camera.zoom_to_selection", C::ZoomToSelection);
    }
    reg.register("playback.set_loop", |args| {
        let a: LoopArgs = parse_args(args)?;
        let mode = crate::playback::LoopMode::parse(&a.mode)
            .ok_or_else(|| format!("unknown loop mode '{}'", a.mode))?;
        Ok(Message::PlaybackControl(
            crate::playback::PlaybackAction::SetLoopMode(mode),
        ))
    });
    reg.register_plain("selection.clear", || Message::ClearSelection);
    reg.register_plain("selection.save_as_named", || Message::SaveSelectionAsNamed);
    reg.register("view.toggle_pane", |args| {
        let a: PaneArgs = parse_args(args)?;
        Ok(Message::MenuAction(MenuAction::ViewTogglePane(a.pane)))
    });

    let ctx = |a: ContextMenuAction| Message::ContextMenuAction(a);
    reg.register("viewport.select_atom", move |args| {
        let a: AtomArgs = parse_args(args)?;
        Ok(ctx(ContextMenuAction::SelectAtom(a.atom_id)))
    });
    reg.register("viewport.select_residue", move |args| {
        let a: ResidueArgs = parse_args(args)?;
        Ok(ctx(ContextMenuAction::SelectResidue {
            chain_id: a.chain_id,
            residue_id: a.residue_id,
        }))
    });
    reg.register("viewport.hide_atom", move |args| {
        let a: AtomArgs = parse_args(args)?;
        Ok(ctx(ContextMenuAction::HideAtom(a.atom_id)))
    });
    reg.register("viewport.show_as", move |args| {
        let a: AtomKindArgs = parse_args(args)?;
        Ok(ctx(ContextMenuAction::ShowAs {
            atom_id: a.atom_id,
            kind: a.kind,
        }))
    });
    reg.register("viewport.color_by", move |args| {
        let a: AtomSchemeArgs = parse_args(args)?;
        Ok(ctx(ContextMenuAction::ColorBy {
            atom_id: a.atom_id,
            scheme: a.scheme,
        }))
    });
    reg.register("viewport.save_atom_as_named_selection", move |args| {
        let a: AtomArgs = parse_args(args)?;
        Ok(ctx(ContextMenuAction::SaveAtomAsNamedSelection(a.atom_id)))
    });
    reg.register("viewport.save_residue_as_named_selection", move |args| {
        let a: ResidueArgs = parse_args(args)?;
        Ok(ctx(ContextMenuAction::SaveResidueAsNamedSelection {
            chain_id: a.chain_id,
            residue_id: a.residue_id,
        }))
    });

    // 構造ツリーのメニューの対象は、メニューを開いたときにアプリが覚えている（`Yui.tree_context_menu`）ので、引数は要らない
    let tree = |a: TreeContextMenuAction| Message::TreeContextMenuAction(a);
    reg.register_plain("tree.select", move || tree(TreeContextMenuAction::Select));
    reg.register_plain("tree.hide", move || tree(TreeContextMenuAction::Hide));
    reg.register_plain("tree.save_as_named_selection", move || {
        tree(TreeContextMenuAction::SaveAsNamedSelection)
    });
    reg.register("tree.show_as", move |args| {
        let a: KindArgs = parse_args(args)?;
        Ok(tree(TreeContextMenuAction::ShowAs(a.kind)))
    });
    reg.register("tree.color_by", move |args| {
        let a: SchemeArgs = parse_args(args)?;
        Ok(tree(TreeContextMenuAction::ColorBy(a.scheme)))
    });

    reg.register("file.open_with", |args| {
        let a: OpenWithArgs = parse_args(args)?;
        let handler = crate::file_open_dispatch::FileOpenHandler::from_id(&a.handler)
            .ok_or_else(|| format!("unknown handler '{}'", a.handler))?;
        Ok(Message::OpenFileWith {
            path: a.path,
            handler,
        })
    });

    let rep = |a: RepContextMenuAction| Message::RepContextMenuAction(a);
    reg.register("representation.toggle_visibility", move |args| {
        let a: IndexArgs = parse_args(args)?;
        Ok(rep(RepContextMenuAction::ToggleVisibility(a.index)))
    });
    reg.register("representation.delete", move |args| {
        let a: IndexArgs = parse_args(args)?;
        Ok(rep(RepContextMenuAction::Delete(a.index)))
    });
    reg.register("representation.show_as", move |args| {
        let a: IndexKindArgs = parse_args(args)?;
        Ok(rep(RepContextMenuAction::ShowAs {
            index: a.index,
            kind: a.kind,
        }))
    });
    reg.register("representation.color_by", move |args| {
        let a: IndexSchemeArgs = parse_args(args)?;
        Ok(rep(RepContextMenuAction::ColorBy {
            index: a.index,
            scheme: a.scheme,
        }))
    });
}

// ---------------------------------------------------------------------------
// 組み込みのメニュー
// ---------------------------------------------------------------------------

fn dropdown_item(label: &str, command: &str) -> MenuItem {
    MenuItem::Command(CommandItem::new(
        label,
        CommandInvocation::plain(command),
        ItemStyle::DROPDOWN,
    ))
}

fn chip(label: &str, command: &str, args: serde_json::Value) -> MenuItem {
    MenuItem::Command(CommandItem::new(
        label,
        CommandInvocation::new(command, args),
        ItemStyle::CHIP,
    ))
}

fn action(label: &str, command: CommandInvocation, style: ItemStyle) -> MenuItem {
    MenuItem::Command(CommandItem::new(label, command, style))
}

fn row(spacing: u16, items: Vec<MenuItem>) -> MenuItem {
    MenuItem::Group {
        layout: GroupLayout::Row { spacing },
        items,
    }
}

fn column(spacing: u16, items: Vec<MenuItem>) -> MenuItem {
    MenuItem::Group {
        layout: GroupLayout::Column { spacing },
        items,
    }
}

/// 「Show As:」「Color By:」のように、見出しと、小さなボタンの行を縦に並べたもの
fn section(heading: &str, rows: Vec<Vec<MenuItem>>) -> MenuItem {
    let mut items = vec![MenuItem::Heading(heading.to_string())];
    items.extend(rows.into_iter().map(|r| row(4, r)));
    column(3, items)
}

fn show_as_chip(
    label: &str,
    command: &str,
    base: serde_json::Value,
    kind: RepresentationKind,
) -> MenuItem {
    let mut args = base;
    args["kind"] = serde_json::to_value(kind).expect("RepresentationKind serializes");
    chip(label, command, args)
}

fn color_by_chip(
    label: &str,
    command: &str,
    base: serde_json::Value,
    scheme: ColorScheme,
) -> MenuItem {
    let mut args = base;
    args["scheme"] = serde_json::to_value(scheme).expect("ColorScheme serializes");
    chip(label, command, args)
}

fn header(title: &str, subtitle: String) -> MenuItem {
    MenuItem::Header {
        title: title.to_string(),
        subtitle,
    }
}

const YELLOW: [f32; 4] = [1.0, 0.85, 0.1, 1.0];

/// 組み込みのメニューを、受け口に登録する（組み込みの並びは 100 刻み）
pub fn register_builtin_menus(menus: &mut MenuRegistry) {
    register_file_menu(menus);
    register_view_menu(menus);
    menus.contribute_item(
        location::HELP,
        90,
        dropdown_item("Mouse & Keyboard...", "help.controls"),
    );
    menus.contribute_item(
        location::HELP,
        100,
        dropdown_item("About YUI...", "help.about"),
    );
    register_viewport_context_menu(menus);
    register_tree_context_menu(menus);
    register_representation_context_menu(menus);
    register_file_context_menu(menus);
}

fn register_file_menu(menus: &mut MenuRegistry) {
    use location::FILE;
    menus.contribute(FILE, 100, |ctx| {
        let export_label = if ctx.has_structure {
            "Export Structure..."
        } else {
            "Export Structure... (No structure)"
        };
        let image_label = if ctx.has_structure {
            "Export Image (SVG)..."
        } else {
            "Export Image (SVG)... (No structure)"
        };
        let animation_label = if ctx.has_structure {
            "Export Animation..."
        } else {
            "Export Animation... (No structure)"
        };
        let png_label = if ctx.has_structure {
            "Export Image (PNG)..."
        } else {
            "Export Image (PNG)... (No structure)"
        };
        vec![
            dropdown_item("New  (Empty AppState)", "file.new"),
            dropdown_item("Open YAML State...", "file.open"),
            dropdown_item("Save YAML State", "file.save"),
            dropdown_item("Save As YAML...", "file.save_as"),
            MenuItem::Command(
                CommandItem::new(
                    export_label,
                    CommandInvocation::plain("file.export_structure"),
                    ItemStyle::DROPDOWN,
                )
                .enabled(ctx.has_structure),
            ),
            MenuItem::Command(
                CommandItem::new(
                    image_label,
                    CommandInvocation::plain("file.export_image_svg"),
                    ItemStyle::DROPDOWN,
                )
                .enabled(ctx.has_structure),
            ),
            MenuItem::Command(
                CommandItem::new(
                    png_label,
                    CommandInvocation::plain("file.export_image_png"),
                    ItemStyle::DROPDOWN,
                )
                .enabled(ctx.has_structure),
            ),
            MenuItem::Command(
                CommandItem::new(
                    animation_label,
                    CommandInvocation::plain("file.export_animation"),
                    ItemStyle::DROPDOWN,
                )
                .enabled(ctx.has_structure && cfg!(not(target_arch = "wasm32"))),
            ),
        ]
    });
    menus.contribute(FILE, 150, |ctx| {
        let open_label = if ctx.has_structure {
            "Open Trajectory (XYZ)..."
        } else {
            "Open Trajectory (XYZ)... (No structure)"
        };
        vec![
            MenuItem::Separator,
            MenuItem::Command(
                CommandItem::new(
                    open_label,
                    CommandInvocation::plain("file.open_trajectory"),
                    ItemStyle::DROPDOWN,
                )
                .enabled(ctx.has_structure),
            ),
            MenuItem::Command(
                CommandItem::new(
                    "Close Trajectory",
                    CommandInvocation::plain("file.close_trajectory"),
                    ItemStyle::DROPDOWN,
                )
                .enabled(ctx.has_trajectory),
            ),
            MenuItem::Separator,
            dropdown_item("Open CUBE (Isosurface)...", "file.open_cube"),
            MenuItem::Command(
                CommandItem::new(
                    "Close CUBE",
                    CommandInvocation::plain("file.close_cube"),
                    ItemStyle::DROPDOWN,
                )
                .enabled(ctx.has_cube),
            ),
        ]
    });
    menus.contribute(FILE, 200, |_| {
        #[cfg_attr(target_arch = "wasm32", allow(unused_mut))]
        // 設定ファイルの項目はネイティブ版だけ
        let mut items = vec![
            MenuItem::Separator,
            dropdown_item("New Text File", "file.new_text_file"),
            dropdown_item("Open Text File...", "file.open_text_file"),
        ];
        // ブラウザ版の設定は localStorage にあり、ファイルとして開けないため、ネイティブ版だけ
        #[cfg(not(target_arch = "wasm32"))]
        items.push(dropdown_item("Open Settings File", "file.open_settings"));
        items.push(dropdown_item("Share Session...", "file.share_session"));
        items
    });
    menus.contribute(FILE, 300, |_| {
        vec![MenuItem::Separator, dropdown_item("Quit", "file.quit")]
    });
}

fn register_view_menu(menus: &mut MenuRegistry) {
    use location::VIEW;
    menus.contribute_item(
        VIEW,
        100,
        dropdown_item("Reset Camera", "view.reset_camera"),
    );
    menus.contribute(VIEW, 200, |_| {
        let preset = |label: &str, color: [f32; 4]| {
            MenuItem::Command(CommandItem::new(
                label,
                CommandInvocation::new("view.set_background_color", json!({ "color": color })),
                ItemStyle::DROPDOWN,
            ))
        };
        vec![
            MenuItem::Separator,
            MenuItem::Heading("Background Color:".to_string()),
            preset(
                "  Dark (Default)",
                yui_core::state::DEFAULT_BACKGROUND_COLOR,
            ),
            preset("  Black", [0.0, 0.0, 0.0, 1.0]),
            preset("  White", [1.0, 1.0, 1.0, 1.0]),
            preset("  Light Gray", [0.85, 0.85, 0.85, 1.0]),
        ]
    });
    menus.contribute(VIEW, 250, |ctx| {
        let theme = |kind: crate::ui_style::ThemeKind| {
            MenuItem::Command(
                CommandItem::new(
                    format!("  {}", kind.label()),
                    CommandInvocation::new("view.set_theme", json!({ "theme": kind.as_str() })),
                    ItemStyle::DROPDOWN,
                )
                .checked(ctx.theme == kind),
            )
        };
        vec![
            MenuItem::Separator,
            MenuItem::Heading("Theme:".to_string()),
            theme(crate::ui_style::ThemeKind::Dark),
            theme(crate::ui_style::ThemeKind::Light),
        ]
    });
    menus.contribute(VIEW, 260, |ctx| {
        use crate::selection::HighlightStyle;
        let style = |s: HighlightStyle| {
            MenuItem::Command(
                CommandItem::new(
                    format!("  {}", s.label()),
                    CommandInvocation::new(
                        "view.set_selection_highlight",
                        json!({ "style": s.as_str() }),
                    ),
                    ItemStyle::DROPDOWN,
                )
                .checked(ctx.highlight == s),
            )
        };
        let selection_item = |label: &str, command: &str| {
            MenuItem::Command(
                CommandItem::new(
                    label,
                    CommandInvocation::plain(command),
                    ItemStyle::DROPDOWN,
                )
                .enabled(ctx.has_selection),
            )
        };
        vec![
            MenuItem::Separator,
            MenuItem::Heading("Selection Highlight:".to_string()),
            style(HighlightStyle::Glow),
            style(HighlightStyle::Halo),
            style(HighlightStyle::Off),
            selection_item("Clear Selection", "selection.clear"),
            selection_item(
                "Save Selection as Named Selection",
                "selection.save_as_named",
            ),
        ]
    });
    menus.contribute(VIEW, 270, |ctx| {
        let command = |label: &str, id: &str| {
            MenuItem::Command(
                CommandItem::new(label, CommandInvocation::plain(id), ItemStyle::DROPDOWN)
                    .enabled(ctx.has_playback),
            )
        };
        // ループの方法は、押すたびに次の方法へ切り替える（メニューを長くしないため）
        let next_loop = ctx.loop_mode.next();
        vec![
            MenuItem::Separator,
            MenuItem::Heading("Playback:".to_string()),
            command(
                if ctx.playing { "Pause" } else { "Play" },
                "playback.play_pause",
            ),
            command("Previous Frame", "playback.prev"),
            command("Next Frame", "playback.next"),
            MenuItem::Command(
                CommandItem::new(
                    format!("Loop: {}", ctx.loop_mode.label()),
                    CommandInvocation::new(
                        "playback.set_loop",
                        json!({ "mode": next_loop.as_str() }),
                    ),
                    ItemStyle::DROPDOWN,
                )
                .enabled(ctx.has_playback),
            ),
        ]
    });
    menus.contribute(VIEW, 280, |ctx| {
        let toggle = |label: &str, id: &str, on: bool| {
            MenuItem::Command(
                CommandItem::new(label, CommandInvocation::plain(id), ItemStyle::DROPDOWN)
                    .checked(on),
            )
        };
        vec![
            MenuItem::Separator,
            MenuItem::Heading("Camera:".to_string()),
            MenuItem::Command(
                CommandItem::new(
                    "Zoom to Selection",
                    CommandInvocation::plain("camera.zoom_to_selection"),
                    ItemStyle::DROPDOWN,
                )
                .enabled(ctx.has_selection),
            ),
            toggle("Spin", "camera.toggle_spin", ctx.spinning),
            toggle("Rock", "camera.toggle_rock", ctx.rocking),
            MenuItem::Command(CommandItem::new(
                format!("Spin Speed: {} deg/s", ctx.spin_speed),
                CommandInvocation::plain("camera.cycle_spin_speed"),
                ItemStyle::DROPDOWN,
            )),
        ]
    });
    menus.contribute(VIEW, 300, |ctx| {
        let pane = |label: &str, pane: PaneKind, shown: bool| {
            MenuItem::Command(
                CommandItem::new(
                    label,
                    CommandInvocation::new("view.toggle_pane", json!({ "pane": pane })),
                    ItemStyle::DROPDOWN,
                )
                .checked(shown),
            )
        };
        let toggle = |label: &str, command: &str, on: bool| {
            MenuItem::Command(
                CommandItem::new(
                    label,
                    CommandInvocation::plain(command),
                    ItemStyle::DROPDOWN,
                )
                .checked(on),
            )
        };
        vec![
            MenuItem::Separator,
            pane("Explorer / Playbook (Left)", PaneKind::Left, ctx.show_left),
            pane("Inspector (Right)", PaneKind::Right, ctx.show_right),
            pane(
                "Bottom Panels (Terminal / Logs)",
                PaneKind::Bottom,
                ctx.show_bottom,
            ),
            toggle(
                "Split Editor Right",
                "view.toggle_editor_split",
                ctx.editor_split,
            ),
            toggle(
                "Link Viewport Cameras",
                "view.toggle_camera_link",
                ctx.camera_link,
            ),
            MenuItem::Separator,
        ]
        .into_iter()
        .chain(crate::pane_view::PlotKind::ALL.map(|kind| {
            MenuItem::Command(CommandItem::new(
                kind.label(),
                CommandInvocation::new("view.open_plot", json!({ "kind": kind.as_str() })),
                ItemStyle::DROPDOWN,
            ))
        }))
        .collect()
    });
}

fn register_viewport_context_menu(menus: &mut MenuRegistry) {
    menus.contribute(location::CONTEXT_VIEWPORT, 100, |ctx| {
        let Some(MenuTarget::Atom(t)) = &ctx.target else {
            return Vec::new();
        };
        let atom = json!({ "atom_id": t.atom_id });
        let residue = t
            .residue_id
            .map(|rid| json!({ "chain_id": t.chain_id, "residue_id": rid }));

        let mut select = vec![action(
            "Select Atom",
            CommandInvocation::new("viewport.select_atom", atom.clone()),
            ItemStyle::ACTION_12,
        )];
        let mut named = vec![action(
            "Save Atom as Named Selection",
            CommandInvocation::new("viewport.save_atom_as_named_selection", atom.clone()),
            ItemStyle::WIDE_11,
        )];
        if let Some(res) = residue {
            select.push(action(
                "Select Residue",
                CommandInvocation::new("viewport.select_residue", res.clone()),
                ItemStyle::ACTION_12,
            ));
            named.push(action(
                "Save Residue as Named Selection",
                CommandInvocation::new("viewport.save_residue_as_named_selection", res),
                ItemStyle::WIDE_11,
            ));
        }

        vec![
            header("Object Menu", t.summary_label()),
            row(6, select),
            column(4, named),
            action(
                "Hide Atom",
                CommandInvocation::new("viewport.hide_atom", atom.clone()),
                ItemStyle::ACTION_12,
            ),
            section(
                "Show As:",
                vec![vec![
                    show_as_chip(
                        "Ball & Stick",
                        "viewport.show_as",
                        atom.clone(),
                        RepresentationKind::BallAndStick,
                    ),
                    show_as_chip(
                        "Spacefill",
                        "viewport.show_as",
                        atom.clone(),
                        RepresentationKind::Cpk,
                    ),
                    show_as_chip(
                        "Cartoon",
                        "viewport.show_as",
                        atom.clone(),
                        RepresentationKind::Cartoon,
                    ),
                ]],
            ),
            section(
                "Color By:",
                vec![vec![
                    color_by_chip(
                        "Element",
                        "viewport.color_by",
                        atom.clone(),
                        ColorScheme::Element,
                    ),
                    color_by_chip(
                        "Chain",
                        "viewport.color_by",
                        atom.clone(),
                        ColorScheme::Chain,
                    ),
                    color_by_chip(
                        "Residue",
                        "viewport.color_by",
                        atom.clone(),
                        ColorScheme::Residue,
                    ),
                    color_by_chip(
                        "Uniform (Yellow)",
                        "viewport.color_by",
                        atom.clone(),
                        ColorScheme::Uniform(YELLOW),
                    ),
                ]],
            ),
        ]
    });
}

fn register_tree_context_menu(menus: &mut MenuRegistry) {
    menus.contribute(location::CONTEXT_STRUCTURE_TREE, 100, |ctx| {
        let Some(MenuTarget::TreeNode(t)) = &ctx.target else {
            return Vec::new();
        };
        let none = json!({});
        vec![
            header("Tree Node Menu", t.label.clone()),
            row(
                6,
                vec![
                    action(
                        "Select / Highlight",
                        CommandInvocation::plain("tree.select"),
                        ItemStyle::ACTION_11,
                    ),
                    action(
                        "Hide",
                        CommandInvocation::plain("tree.hide"),
                        ItemStyle::ACTION_11,
                    ),
                ],
            ),
            action(
                "Save as Named Selection",
                CommandInvocation::plain("tree.save_as_named_selection"),
                ItemStyle::WIDE_11,
            ),
            section(
                "Show As:",
                vec![vec![
                    show_as_chip(
                        "Ball & Stick",
                        "tree.show_as",
                        none.clone(),
                        RepresentationKind::BallAndStick,
                    ),
                    show_as_chip(
                        "Spacefill",
                        "tree.show_as",
                        none.clone(),
                        RepresentationKind::Cpk,
                    ),
                    show_as_chip(
                        "Cartoon",
                        "tree.show_as",
                        none.clone(),
                        RepresentationKind::Cartoon,
                    ),
                ]],
            ),
            section(
                "Color By:",
                vec![vec![
                    color_by_chip(
                        "Element",
                        "tree.color_by",
                        none.clone(),
                        ColorScheme::Element,
                    ),
                    color_by_chip("Chain", "tree.color_by", none.clone(), ColorScheme::Chain),
                    color_by_chip(
                        "Residue",
                        "tree.color_by",
                        none.clone(),
                        ColorScheme::Residue,
                    ),
                    color_by_chip(
                        "Uniform",
                        "tree.color_by",
                        none.clone(),
                        ColorScheme::Uniform(YELLOW),
                    ),
                ]],
            ),
        ]
    });
}

fn register_file_context_menu(menus: &mut MenuRegistry) {
    menus.contribute(location::CONTEXT_FILE, 100, |ctx| {
        let Some(MenuTarget::File(t)) = &ctx.target else {
            return Vec::new();
        };
        let name = std::path::Path::new(&t.path)
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or(&t.path)
            .to_string();
        let mut items = vec![header("Open with", name)];
        items.extend(t.handlers.iter().map(|h| {
            action(
                h.label(),
                CommandInvocation::new(
                    "file.open_with",
                    json!({ "path": t.path, "handler": h.id() }),
                ),
                ItemStyle::WIDE_11,
            )
        }));
        items
    });
}

fn register_representation_context_menu(menus: &mut MenuRegistry) {
    menus.contribute(location::CONTEXT_REPRESENTATION, 100, |ctx| {
        let Some(MenuTarget::Representation(t)) = &ctx.target else {
            return Vec::new();
        };
        let idx = json!({ "index": t.index });
        let vis_label = if t.visible {
            "Hide Representation"
        } else {
            "Show Representation"
        };
        vec![
            header("Representation Menu", t.id.clone()),
            row(
                6,
                vec![
                    action(
                        vis_label,
                        CommandInvocation::new("representation.toggle_visibility", idx.clone()),
                        ItemStyle::ACTION_11,
                    ),
                    action(
                        "Delete",
                        CommandInvocation::new("representation.delete", idx.clone()),
                        ItemStyle::DANGER_11,
                    ),
                ],
            ),
            section(
                "Show As:",
                vec![
                    vec![
                        show_as_chip(
                            "Ball & Stick",
                            "representation.show_as",
                            idx.clone(),
                            RepresentationKind::BallAndStick,
                        ),
                        show_as_chip(
                            "Cartoon",
                            "representation.show_as",
                            idx.clone(),
                            RepresentationKind::Cartoon,
                        ),
                        show_as_chip(
                            "Spacefill",
                            "representation.show_as",
                            idx.clone(),
                            RepresentationKind::Cpk,
                        ),
                    ],
                    vec![
                        show_as_chip(
                            "Licorice",
                            "representation.show_as",
                            idx.clone(),
                            RepresentationKind::Licorice,
                        ),
                        show_as_chip(
                            "Line",
                            "representation.show_as",
                            idx.clone(),
                            RepresentationKind::Line,
                        ),
                        show_as_chip(
                            "Surface",
                            "representation.show_as",
                            idx.clone(),
                            RepresentationKind::Surface,
                        ),
                    ],
                ],
            ),
            section(
                "Color By:",
                vec![
                    vec![
                        color_by_chip(
                            "Element",
                            "representation.color_by",
                            idx.clone(),
                            ColorScheme::Element,
                        ),
                        color_by_chip(
                            "Chain",
                            "representation.color_by",
                            idx.clone(),
                            ColorScheme::Chain,
                        ),
                        color_by_chip(
                            "Residue",
                            "representation.color_by",
                            idx.clone(),
                            ColorScheme::Residue,
                        ),
                    ],
                    vec![
                        color_by_chip(
                            "Secondary Structure",
                            "representation.color_by",
                            idx.clone(),
                            ColorScheme::SecondaryStructure,
                        ),
                        color_by_chip(
                            "Uniform (Yellow)",
                            "representation.color_by",
                            idx.clone(),
                            ColorScheme::Uniform(YELLOW),
                        ),
                    ],
                ],
            ),
        ]
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::{CommandRegistry, MenuContext, MenuItem, MenuRegistry, MenuTarget, location};

    fn registries() -> (CommandRegistry, MenuRegistry) {
        let mut commands = CommandRegistry::new();
        register_menu_commands(&mut commands);
        let mut menus = MenuRegistry::new();
        register_builtin_menus(&mut menus);
        (commands, menus)
    }

    fn all_shown() -> MenuContext {
        MenuContext {
            show_left: true,
            show_right: true,
            show_bottom: true,
            has_structure: true,
            ..MenuContext::default()
        }
    }

    /// 項目を、表示名・順番・押せるか・並べ方が分かる文字列にする
    fn describe(items: &[MenuItem]) -> Vec<String> {
        items
            .iter()
            .map(|item| match item {
                MenuItem::Command(c) => {
                    let off = if c.enabled { "" } else { " [disabled]" };
                    format!("{}{}", c.display_label(), off)
                }
                MenuItem::Separator => "---".to_string(),
                MenuItem::Heading(h) => format!("# {h}"),
                MenuItem::Header { title, subtitle } => format!("header: {title} / {subtitle}"),
                MenuItem::Group { layout, items } => {
                    let (kind, spacing) = match layout {
                        GroupLayout::Row { spacing } => ("row", spacing),
                        GroupLayout::Column { spacing } => ("col", spacing),
                    };
                    format!("{kind}{spacing}{{ {} }}", describe(items).join(" | "))
                }
                MenuItem::Submenu { label, items } => {
                    format!("submenu {label} {{ {} }}", describe(items).join(" | "))
                }
            })
            .collect()
    }

    /// 項目のコマンドを実行したときのメッセージ（Debug 表記）を、表示名の順に並べる
    fn resolved(commands: &CommandRegistry, items: &[MenuItem]) -> Vec<(String, String)> {
        items
            .iter()
            .flat_map(|i| i.command_items())
            .map(|c| {
                let msg = commands
                    .resolve(&c.command)
                    .unwrap_or_else(|e| panic!("{}: {e}", c.label));
                (c.label.clone(), format!("{msg:?}"))
            })
            .collect()
    }

    fn atom_target(residue: bool) -> MenuTarget {
        MenuTarget::Atom(crate::context_menu::ContextMenuTarget {
            atom_id: 42,
            element: "C".to_string(),
            atom_name: "CA".to_string(),
            residue_name: "ALA".to_string(),
            residue_id: residue.then_some(15),
            chain_id: "A".to_string(),
            position: [0.0; 3],
        })
    }

    #[test]
    fn test_file_menu_labels_and_order_match_the_previous_menu() {
        let (_, menus) = registries();
        let with = describe(&menus.build(location::FILE, &all_shown()));
        let mut expected = vec![
            "New  (Empty AppState)",
            "Open YAML State...",
            "Save YAML State",
            "Save As YAML...",
            "Export Structure...",
            "Export Image (SVG)...",
            "Export Image (PNG)...",
            "Export Animation...",
            "---",
            "Open Trajectory (XYZ)...",
            "Close Trajectory [disabled]",
            "---",
            "Open CUBE (Isosurface)...",
            "Close CUBE [disabled]",
            "---",
            "New Text File",
            "Open Text File...",
        ];
        if cfg!(not(target_arch = "wasm32")) {
            expected.push("Open Settings File");
        }
        expected.extend(["Share Session...", "---", "Quit"]);
        assert_eq!(with, expected);

        // 構造がないと、Export Structure は押せず、表示名が変わる
        let without = describe(&menus.build(
            location::FILE,
            &MenuContext {
                has_structure: false,
                ..all_shown()
            },
        ));
        assert_eq!(without[4], "Export Structure... (No structure) [disabled]");
        assert_eq!(
            without[5],
            "Export Image (SVG)... (No structure) [disabled]"
        );
        assert_eq!(
            without[6],
            "Export Image (PNG)... (No structure) [disabled]"
        );
        assert_eq!(without[7], "Export Animation... (No structure) [disabled]");
        assert_eq!(&without[..4], &with[..4]);
        // 軌跡も、構造がないと開けない
        assert_eq!(
            without[9],
            "Open Trajectory (XYZ)... (No structure) [disabled]"
        );
        // 軌跡を開いていると、閉じられる
        let open = describe(&menus.build(
            location::FILE,
            &MenuContext {
                has_trajectory: true,
                ..all_shown()
            },
        ));
        assert_eq!(open[10], "Close Trajectory");
    }

    #[test]
    fn test_view_menu_labels_order_and_checks_match_the_previous_menu() {
        let (_, menus) = registries();
        let ctx = MenuContext {
            show_left: true,
            show_right: false,
            show_bottom: true,
            editor_split: true,
            camera_link: false,
            ..all_shown()
        };
        assert_eq!(
            describe(&menus.build(location::VIEW, &ctx)),
            vec![
                "Reset Camera",
                "---",
                "# Background Color:",
                "  Dark (Default)",
                "  Black",
                "  White",
                "  Light Gray",
                "---",
                "# Theme:",
                "✓   Dark",
                "    Light",
                "---",
                "# Selection Highlight:",
                "✓   Glow",
                "    Halo",
                "    Off",
                "Clear Selection [disabled]",
                "Save Selection as Named Selection [disabled]",
                "---",
                "# Playback:",
                "Play [disabled]",
                "Previous Frame [disabled]",
                "Next Frame [disabled]",
                "Loop: Repeat [disabled]",
                "---",
                "# Camera:",
                "Zoom to Selection [disabled]",
                "  Spin",
                "  Rock",
                "Spin Speed: 0 deg/s",
                "---",
                "✓ Explorer / Playbook (Left)",
                "  Inspector (Right)",
                "✓ Bottom Panels (Terminal / Logs)",
                "✓ Split Editor Right",
                "  Link Viewport Cameras",
                "---",
                "Plot: Atom Positions",
            ]
        );
    }

    #[test]
    fn test_playback_items_are_enabled_with_frames_and_show_the_state() {
        let (commands, menus) = registries();
        let ctx = MenuContext {
            has_playback: true,
            playing: true,
            loop_mode: crate::playback::LoopMode::PingPong,
            ..all_shown()
        };
        let items = menus.build(location::VIEW, &ctx);
        let text = describe(&items);
        for expected in ["Pause", "Previous Frame", "Next Frame", "Loop: Ping-pong"] {
            assert!(text.iter().any(|t| t == expected), "{expected}: {text:?}");
        }
        // 押すと、ループの方法が次へ切り替わる
        let got = resolved(&commands, &items);
        assert!(
            got.iter()
                .any(|(l, m)| l == "Loop: Ping-pong" && m == "PlaybackControl(SetLoopMode(Once))"),
            "{got:?}"
        );
    }

    #[test]
    fn test_playback_commands_resolve_to_playback_actions() {
        let (commands, _) = registries();
        let run = |id: &str, args: serde_json::Value| {
            format!(
                "{:?}",
                commands
                    .resolve(&CommandInvocation::new(id, args))
                    .expect("registered")
            )
        };
        let none = serde_json::json!({});
        assert_eq!(
            run("playback.play_pause", none.clone()),
            "PlaybackControl(PlayPause)"
        );
        assert_eq!(
            run("playback.first", none.clone()),
            "PlaybackControl(First)"
        );
        assert_eq!(run("playback.prev", none.clone()), "PlaybackControl(Prev)");
        assert_eq!(run("playback.next", none.clone()), "PlaybackControl(Next)");
        assert_eq!(run("playback.last", none), "PlaybackControl(Last)");
        assert_eq!(
            run(
                "playback.set_loop",
                serde_json::json!({ "mode": "ping_pong" })
            ),
            "PlaybackControl(SetLoopMode(PingPong))"
        );
        assert!(
            commands
                .resolve(&CommandInvocation::new(
                    "playback.set_loop",
                    serde_json::json!({ "mode": "bounce" })
                ))
                .is_err()
        );
    }

    #[test]
    fn test_hiding_a_pane_removes_its_check() {
        let (_, menus) = registries();
        let find = |ctx: &MenuContext| {
            describe(&menus.build(location::VIEW, ctx))
                .into_iter()
                .find(|l| l.contains("Inspector (Right)"))
                .unwrap()
        };
        assert_eq!(find(&all_shown()), "✓ Inspector (Right)");
        assert_eq!(
            find(&MenuContext {
                show_right: false,
                ..all_shown()
            }),
            "  Inspector (Right)"
        );
    }

    #[test]
    fn test_help_menu() {
        let (_, menus) = registries();
        assert_eq!(
            describe(&menus.build(location::HELP, &all_shown())),
            vec!["Mouse & Keyboard...", "About YUI..."]
        );
    }

    #[test]
    fn test_dropdown_commands_resolve_to_the_same_actions_as_before() {
        let (commands, menus) = registries();
        let ctx = all_shown();
        let mut all = resolved(&commands, &menus.build(location::FILE, &ctx));
        all.extend(resolved(&commands, &menus.build(location::VIEW, &ctx)));
        all.extend(resolved(&commands, &menus.build(location::HELP, &ctx)));
        let m = |a: &str| format!("MenuAction({a})");
        let mut expected = vec![
            ("New  (Empty AppState)", m("FileNew")),
            ("Open YAML State...", m("FileOpen")),
            ("Save YAML State", m("FileSave")),
            ("Save As YAML...", m("FileSaveAs")),
            ("Export Structure...", m("FileExportStructure")),
            ("Export Image (SVG)...", m("FileExportImageSvg")),
            ("Export Image (PNG)...", m("FileExportImagePng")),
            ("Export Animation...", m("FileExportAnimation")),
            ("Open Trajectory (XYZ)...", m("FileOpenTrajectory")),
            ("Close Trajectory", m("FileCloseTrajectory")),
            ("Open CUBE (Isosurface)...", m("FileOpenCube")),
            ("Close CUBE", m("FileCloseCube")),
            ("New Text File", m("FileNewTextFile")),
            ("Open Text File...", m("FileOpenTextFile")),
        ];
        if cfg!(not(target_arch = "wasm32")) {
            expected.push(("Open Settings File", m("FileOpenSettings")));
        }
        expected.push(("Share Session...", m("FileShareSession")));
        expected.extend([
            ("Quit", m("FileQuit")),
            ("Reset Camera", m("ViewResetCamera")),
            (
                "  Dark (Default)",
                m(&format!(
                    "ViewSetBackgroundColor({:?})",
                    yui_core::state::DEFAULT_BACKGROUND_COLOR
                )),
            ),
            ("  Black", m("ViewSetBackgroundColor([0.0, 0.0, 0.0, 1.0])")),
            ("  White", m("ViewSetBackgroundColor([1.0, 1.0, 1.0, 1.0])")),
            (
                "  Light Gray",
                m("ViewSetBackgroundColor([0.85, 0.85, 0.85, 1.0])"),
            ),
            ("  Dark", m("ViewSetTheme(Dark)")),
            ("  Light", m("ViewSetTheme(Light)")),
            ("  Glow", m("ViewSetSelectionHighlight(Glow)")),
            ("  Halo", m("ViewSetSelectionHighlight(Halo)")),
            ("  Off", m("ViewSetSelectionHighlight(Off)")),
            ("Clear Selection", "ClearSelection".to_string()),
            (
                "Save Selection as Named Selection",
                "SaveSelectionAsNamed".to_string(),
            ),
            ("Play", "PlaybackControl(PlayPause)".to_string()),
            ("Previous Frame", "PlaybackControl(Prev)".to_string()),
            ("Next Frame", "PlaybackControl(Next)".to_string()),
            (
                "Loop: Repeat",
                "PlaybackControl(SetLoopMode(PingPong))".to_string(),
            ),
            (
                "Zoom to Selection",
                "CameraAction(ZoomToSelection)".to_string(),
            ),
            ("Spin", "CameraAction(ToggleSpin)".to_string()),
            ("Rock", "CameraAction(ToggleRock)".to_string()),
            (
                "Spin Speed: 0 deg/s",
                "CameraAction(CycleSpinSpeed)".to_string(),
            ),
            ("Explorer / Playbook (Left)", m("ViewTogglePane(Left)")),
            ("Inspector (Right)", m("ViewTogglePane(Right)")),
            (
                "Bottom Panels (Terminal / Logs)",
                m("ViewTogglePane(Bottom)"),
            ),
            ("Split Editor Right", m("ViewToggleEditorSplit")),
            ("Link Viewport Cameras", m("ViewToggleCameraLink")),
            ("Plot: Atom Positions", m("ViewOpenPlot(AtomPositions)")),
            ("Mouse & Keyboard...", m("HelpControls")),
            ("About YUI...", m("HelpAbout")),
        ]);
        let expected: Vec<(String, String)> = expected
            .into_iter()
            .map(|(l, a)| (l.to_string(), a))
            .collect();
        assert_eq!(all, expected);
    }

    #[test]
    fn test_viewport_context_menu_labels_and_actions() {
        let (commands, menus) = registries();
        let ctx = |residue| MenuContext {
            target: Some(atom_target(residue)),
            ..all_shown()
        };
        let items = menus.build(location::CONTEXT_VIEWPORT, &ctx(true));
        assert_eq!(
            describe(&items),
            vec![
                "header: Object Menu / A/ALA:15.CA (#42)",
                "row6{ Select Atom | Select Residue }",
                "col4{ Save Atom as Named Selection | Save Residue as Named Selection }",
                "Hide Atom",
                "col3{ # Show As: | row4{ Ball & Stick | Spacefill | Cartoon } }",
                "col3{ # Color By: | row4{ Element | Chain | Residue | Uniform (Yellow) } }",
            ]
        );
        let got: Vec<String> = resolved(&commands, &items)
            .into_iter()
            .map(|(_, m)| m)
            .collect();
        let c = |a: &str| format!("ContextMenuAction({a})");
        assert_eq!(
            got,
            vec![
                c("SelectAtom(42)"),
                c("SelectResidue { chain_id: \"A\", residue_id: 15 }"),
                c("SaveAtomAsNamedSelection(42)"),
                c("SaveResidueAsNamedSelection { chain_id: \"A\", residue_id: 15 }"),
                c("HideAtom(42)"),
                c("ShowAs { atom_id: 42, kind: BallAndStick }"),
                c("ShowAs { atom_id: 42, kind: Cpk }"),
                c("ShowAs { atom_id: 42, kind: Cartoon }"),
                c("ColorBy { atom_id: 42, scheme: Element }"),
                c("ColorBy { atom_id: 42, scheme: Chain }"),
                c("ColorBy { atom_id: 42, scheme: Residue }"),
                c("ColorBy { atom_id: 42, scheme: Uniform([1.0, 0.85, 0.1, 1.0]) }"),
            ]
        );

        // 残基のない原子では、残基のボタンが出ない
        let no_residue = describe(&menus.build(location::CONTEXT_VIEWPORT, &ctx(false)));
        assert_eq!(no_residue[1], "row6{ Select Atom }");
        assert_eq!(no_residue[2], "col4{ Save Atom as Named Selection }");
    }

    #[test]
    fn test_tree_context_menu_labels_and_actions() {
        let (commands, menus) = registries();
        let target = MenuTarget::TreeNode(crate::structure_tree::TreeContextTarget {
            key: "c:1:A".to_string(),
            label: "Chain A".to_string(),
            selector: yui_core::selector::Selector::All,
        });
        let items = menus.build(
            location::CONTEXT_STRUCTURE_TREE,
            &MenuContext {
                target: Some(target),
                ..all_shown()
            },
        );
        assert_eq!(
            describe(&items),
            vec![
                "header: Tree Node Menu / Chain A",
                "row6{ Select / Highlight | Hide }",
                "Save as Named Selection",
                "col3{ # Show As: | row4{ Ball & Stick | Spacefill | Cartoon } }",
                "col3{ # Color By: | row4{ Element | Chain | Residue | Uniform } }",
            ]
        );
        let got: Vec<String> = resolved(&commands, &items)
            .into_iter()
            .map(|(_, m)| m)
            .collect();
        let t = |a: &str| format!("TreeContextMenuAction({a})");
        assert_eq!(
            got,
            vec![
                t("Select"),
                t("Hide"),
                t("SaveAsNamedSelection"),
                t("ShowAs(BallAndStick)"),
                t("ShowAs(Cpk)"),
                t("ShowAs(Cartoon)"),
                t("ColorBy(Element)"),
                t("ColorBy(Chain)"),
                t("ColorBy(Residue)"),
                t("ColorBy(Uniform([1.0, 0.85, 0.1, 1.0]))"),
            ]
        );
    }

    #[test]
    fn test_representation_context_menu_labels_and_actions() {
        let (commands, menus) = registries();
        let ctx = |visible| MenuContext {
            target: Some(MenuTarget::Representation(
                crate::representation_inspector::RepContextTarget {
                    index: 3,
                    id: "rep_3".to_string(),
                    kind: yui_core::RepresentationKind::Cartoon,
                    visible,
                },
            )),
            ..all_shown()
        };
        let items = menus.build(location::CONTEXT_REPRESENTATION, &ctx(true));
        assert_eq!(
            describe(&items),
            vec![
                "header: Representation Menu / rep_3",
                "row6{ Hide Representation | Delete }",
                "col3{ # Show As: | row4{ Ball & Stick | Cartoon | Spacefill } | row4{ Licorice | Line | Surface } }",
                "col3{ # Color By: | row4{ Element | Chain | Residue } | row4{ Secondary Structure | Uniform (Yellow) } }",
            ]
        );
        // 非表示の Representation では、表示名が変わる
        let hidden = describe(&menus.build(location::CONTEXT_REPRESENTATION, &ctx(false)));
        assert_eq!(hidden[1], "row6{ Show Representation | Delete }");

        let got: Vec<String> = resolved(&commands, &items)
            .into_iter()
            .map(|(_, m)| m)
            .collect();
        let r = |a: &str| format!("RepContextMenuAction({a})");
        assert_eq!(got[0], r("ToggleVisibility(3)"));
        assert_eq!(got[1], r("Delete(3)"));
        assert_eq!(got[2], r("ShowAs { index: 3, kind: BallAndStick }"));
        assert_eq!(got[7], r("ShowAs { index: 3, kind: Surface }"));
        assert_eq!(got[8], r("ColorBy { index: 3, scheme: Element }"));
        assert_eq!(got.len(), 2 + 6 + 5);
    }

    #[test]
    fn test_file_context_menu_lists_the_handlers_and_opens_with_the_choice() {
        use crate::file_open_dispatch::{FileOpenHandler, OpenWithTarget};
        let (commands, menus) = registries();
        let ctx = MenuContext {
            target: Some(MenuTarget::File(OpenWithTarget {
                path: "x/p.pdb".to_string(),
                handlers: vec![
                    FileOpenHandler::StructureViewer,
                    FileOpenHandler::TextEditor,
                ],
            })),
            ..all_shown()
        };
        let items = menus.build(location::CONTEXT_FILE, &ctx);
        assert_eq!(
            describe(&items),
            vec![
                "header: Open with / p.pdb",
                "Structure Viewer",
                "Text Editor"
            ]
        );
        let got: Vec<String> = resolved(&commands, &items)
            .into_iter()
            .map(|(_, m)| m)
            .collect();
        assert_eq!(
            got[1],
            "OpenFileWith { path: \"x/p.pdb\", handler: TextEditor }"
        );
    }

    #[test]
    fn test_every_builtin_menu_command_is_registered() {
        let (commands, menus) = registries();
        let targets = [
            (location::CONTEXT_VIEWPORT, atom_target(true)),
            (
                location::CONTEXT_STRUCTURE_TREE,
                MenuTarget::TreeNode(crate::structure_tree::TreeContextTarget {
                    key: "k".to_string(),
                    label: "l".to_string(),
                    selector: yui_core::selector::Selector::All,
                }),
            ),
            (
                location::CONTEXT_REPRESENTATION,
                MenuTarget::Representation(crate::representation_inspector::RepContextTarget {
                    index: 0,
                    id: "r".to_string(),
                    kind: yui_core::RepresentationKind::Cartoon,
                    visible: true,
                }),
            ),
        ];
        let mut all: Vec<MenuItem> = [location::FILE, location::VIEW, location::HELP]
            .iter()
            .flat_map(|l| menus.build(l, &all_shown()))
            .collect();
        for (loc, target) in targets {
            all.extend(menus.build(
                loc,
                &MenuContext {
                    target: Some(target),
                    ..all_shown()
                },
            ));
        }
        for item in all.iter().flat_map(|i| i.command_items()) {
            assert!(
                commands.contains(&item.command.id),
                "unregistered: {}",
                item.command.id
            );
        }
    }

    #[test]
    fn test_contributed_item_appears_in_order_and_runs_through_the_registry() {
        let (mut commands, mut menus) = registries();
        // プラグインに見立てたコマンドと項目
        commands.register_plain("plugin.hello", || {
            crate::app::Message::MenuAction(crate::menu_bar::MenuAction::HelpAbout)
        });
        menus.contribute_item(
            location::HELP,
            150,
            MenuItem::Command(CommandItem::new(
                "Hello from a plugin",
                CommandInvocation::plain("plugin.hello"),
                ItemStyle::DROPDOWN,
            )),
        );
        menus.contribute_item(location::HELP, 50, MenuItem::Heading("First".to_string()));
        // 同じ order なら、足した順
        menus.contribute_item(location::HELP, 150, MenuItem::Heading("Second".to_string()));

        let items = menus.build(location::HELP, &all_shown());
        assert_eq!(
            describe(&items),
            vec![
                "# First",
                "Mouse & Keyboard...",
                "About YUI...",
                "Hello from a plugin",
                "# Second"
            ]
        );
        let plugin = items
            .iter()
            .flat_map(|i| i.command_items())
            .find(|c| c.label == "Hello from a plugin")
            .unwrap();
        assert_eq!(
            format!("{:?}", commands.resolve(&plugin.command).unwrap()),
            "MenuAction(HelpAbout)"
        );

        // 場所が違えば、現れない
        assert_eq!(
            describe(&menus.build(location::FILE, &all_shown()))
                .iter()
                .filter(|l| l.contains("plugin"))
                .count(),
            0
        );
    }

    #[test]
    fn test_unknown_or_malformed_commands_do_not_panic() {
        let (commands, _) = registries();
        assert!(matches!(
            commands.resolve(&CommandInvocation::plain("no.such.command")),
            Err(crate::menu::command::CommandError::Unknown(_))
        ));
        assert!(matches!(
            commands.resolve(&CommandInvocation::new(
                "viewport.select_atom",
                json!({ "atom_id": "not a number" })
            )),
            Err(crate::menu::command::CommandError::BadArgs { .. })
        ));
        assert!(
            commands
                .resolve(&CommandInvocation::plain("viewport.hide_atom"))
                .is_err()
        );

        // アプリに渡しても、何も起きず、警告が出る
        let mut app = crate::app::Yui::default();
        let _ = app.update(crate::app::Message::RunCommand(CommandInvocation::plain(
            "no.such.command",
        )));
        let status = app.status_message.as_ref().expect("a warning is shown");
        assert_eq!(status.level, crate::status_bar::StatusLevel::Warning);
        assert!(status.text.contains("no.such.command"));
    }

    #[test]
    fn test_invocation_is_serializable() {
        let inv = CommandInvocation::new(
            "viewport.show_as",
            json!({ "atom_id": 7, "kind": "cartoon" }),
        );
        let text = serde_json::to_string(&inv).unwrap();
        assert_eq!(
            serde_json::from_str::<CommandInvocation>(&text).unwrap(),
            inv
        );
        // 識別子と引数だけを書いたデータからも、同じコマンドを呼べる
        let (commands, _) = registries();
        assert_eq!(
            format!("{:?}", commands.resolve(&inv).unwrap()),
            "ContextMenuAction(ShowAs { atom_id: 7, kind: Cartoon })"
        );
    }

    #[test]
    fn test_running_a_command_through_the_app_performs_the_menu_action() {
        let mut app = crate::app::Yui::default();
        assert!(app.show_left);
        let _ = app.update(crate::app::Message::RunCommand(CommandInvocation::new(
            "view.toggle_pane",
            json!({ "pane": "left" }),
        )));
        assert!(!app.show_left);
    }

    #[test]
    fn test_theme_items_show_the_current_theme_and_resolve() {
        use crate::ui_style::ThemeKind;
        let (commands, menus) = registries();
        for (current, checked) in [
            (ThemeKind::Dark, ["✓   Dark", "    Light"]),
            (ThemeKind::Light, ["    Dark", "✓   Light"]),
        ] {
            let ctx = MenuContext {
                theme: current,
                ..all_shown()
            };
            let described = describe(&menus.build(location::VIEW, &ctx));
            assert!(
                described.windows(2).any(|w| w == checked),
                "{current:?}: {described:?}"
            );
        }
        // 不明なテーマの名前では、何も実行せず、エラーを返す（パニックしない）
        assert!(
            commands
                .resolve(&CommandInvocation::new(
                    "view.set_theme",
                    json!({ "theme": "blue" })
                ))
                .is_err()
        );
        assert!(
            commands
                .resolve(&CommandInvocation::new("view.set_theme", json!({})))
                .is_err()
        );
        let msg = commands
            .resolve(&CommandInvocation::new(
                "view.set_theme",
                json!({ "theme": "light" }),
            ))
            .expect("known theme");
        assert_eq!(format!("{msg:?}"), "MenuAction(ViewSetTheme(Light))");
    }
}
