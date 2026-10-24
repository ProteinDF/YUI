//! 利用者ごとの設定ファイル。
//!
//! ワークスペース（`AppState`、YAML、協調編集で共有される）とは別の、その利用者だけの設定を持つ。
//!
//! - ネイティブ版: OS の設定フォルダの `yui/settings.yaml`（Linux なら `~/.config/yui/settings.yaml`）
//! - WASM 版: ブラウザの `localStorage` のキー `yui.settings`（中身は同じ形式の YAML）
//!
//! 設定は起動時に 1 回だけ読む（変えたら再起動が必要）。ファイルがなければ既定値で動く。壊れた設定でもアプリは
//! 止めず、壊れた項目だけ既定値に戻して、ステータスバーに警告を出す。知らない項目は無視する
//! （将来、項目を足しても古い版で読めるようにするため）。

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::fonts;
use crate::selection::HighlightStyle;
use crate::ui_style::{self, ThemeKind, UiSettings};

/// 設定ファイルの名前
pub const SETTINGS_FILE_NAME: &str = "settings.yaml";
/// WASM 版で、設定を入れる `localStorage` のキー
#[cfg(target_arch = "wasm32")]
pub const STORAGE_KEY: &str = "yui.settings";
/// `editor.font_size` と `terminal.font_size` の下限・上限（ピクセル）
pub const FONT_SIZE_MIN: f64 = 6.0;
pub const FONT_SIZE_MAX: f64 = 72.0;

/// 設定ファイルを開く操作（File → Open Settings File）で、ファイルがなかったときに作る内容。
/// すべてコメントなので、読み込むと既定値になる。
pub const DEFAULT_SETTINGS_TEMPLATE: &str = "\
# YUI user settings.
# Applied at startup: restart YUI after editing this file.
# Unknown keys are ignored. Remove the leading '#' to enable a setting.

# UI theme: \"dark\" (default) or \"light\". Also changed by View -> Theme.
#theme: dark

# Highlight of the selected atoms: \"glow\" (default), \"halo\" or \"off\". Also changed by View -> Selection Highlight.
#selection_highlight: glow

# Drawing sizes (radii in angstroms; ball_scale is a fraction of the van der Waals radius).
# The ranges are checked; a value out of range is replaced by the default shown here.
#render:
#  ball_scale: 0.35          # Ball & Stick sphere size, 0.1 - 1.0
#  stick_radius: 0.12        # Ball & Stick stick radius, 0.02 - 0.5
#  licorice_radius: 0.2      # Licorice stick radius, 0.05 - 0.6
#  line_radius: 0.04         # Line radius, 0.01 - 0.2
#  interaction_radius: 0.08  # non-covalent interaction lines (also in the SVG export), 0.02 - 0.3
#  interaction_dash: 0.35    # length of a dash of the interaction lines, 0.05 - 2.0
#  interaction_gap: 0.25     # length of the gap between dashes (0 = solid line), 0 - 2.0
#  surface_grid_spacing: 0.5 # molecular surface grid spacing; smaller = finer but slower and heavier, 0.25 - 1.5
#  surface_probe_radius: 0.0 # molecular surface probe radius (0 = van der Waals; 1.4 = solvent-accessible), 0 - 3.0
#  bond_color: [0.7, 0.7, 0.7]  # color of the Ball & Stick sticks (RGB, each 0 - 1)
#  ribbon_helix_width: 2.6   # cartoon: helix ribbon width, 0.4 - 6.0
#  ribbon_helix_thickness: 0.7   # helix ribbon thickness, 0.1 - 2.0
#  ribbon_sheet_width: 2.4   # sheet arrow width, 0.4 - 6.0
#  ribbon_sheet_thickness: 0.3   # sheet thickness, 0.1 - 2.0
#  ribbon_loop_radius: 0.25  # loop tube radius, 0.05 - 1.0
#  selection_size: 1.0       # size of the selection glow / halo (a factor), 0.5 - 3.0
#  ssao_strength: 0.0        # ambient occlusion (SSAO) darkening of crevices; 0 = off, 0.5 - 1 is typical, 0 - 1
#  ssao_radius: 3.0          # how far (angstroms) SSAO looks for occluders, 0.3 - 10

# Look of the menus and the tabs. A value out of range is replaced by the default shown here.
#ui:
#  tab_style: underline      # \"underline\" (an accent line under the selected tab) or \"filled\" (a filled background)
#  tab_font_size: 12         # tab label size in pixels, 8 - 24
#  corner_radius: 8          # corner radius of menus and dialogs in pixels (items use half of it), 0 - 16
#  hide_empty_docks: false   # true: hide a side / bottom panel when its last tab is dragged away or closed
#                            # (false: keep it and show \"Drag a tab here.\"; View shows a hidden panel again)
#  # Colors as \"#rrggbb\" or \"#rrggbbaa\". `colors` applies to both themes; `dark_colors` / `light_colors`
#  # only to that theme. Names: accent, bar_bg, card_bg, card_border, text, text_muted, text_disabled,
#  # item_hover_bg, item_active_bg, separator, tab_bar_bg, tab_indicator, tab_active_text,
#  # tab_inactive_text, tab_active_bg, tab_hover_bg, status_bg, status_border, panel_bg, panel_border.
#  colors:
#    accent: \"#5b8def\"
#    tab_indicator: \"#5b8def\"
#  dark_colors: {}
#  light_colors: {}

#editor:
#  # Font family for the text editor and the terminal: the bundled \"JetBrains Mono\",
#  # or the family name of a font installed on this computer (not available in the browser).
#  font_family: \"JetBrains Mono\"
#  # Editor font size in pixels (6 - 72). The terminal keeps its own fixed size.
#  font_size: 14
#  # Show a scrollbar next to the text editor (true / false). Needs a restart.
#  scrollbar: true

#terminal:
#  # Terminal font size in pixels (6 - 72). Needs a restart. The family is `editor.font_family`.
#  font_size: 12
";

/// エディタ（とターミナル）の設定。`None` は、既定値を使うこと。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EditorSettings {
    pub font_family: Option<String>,
    pub font_size: Option<f32>,
    /// スクロールバーを出すか。`None` は、出す
    pub scrollbar: Option<bool>,
}

/// ターミナルの設定。`None` は、既定値を使うこと。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TerminalSettings {
    pub font_size: Option<f32>,
}

/// 描画の大きさの設定（検証済み。範囲外の値は既定値に戻してある）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderSettings {
    /// ボール・スティックの球の半径（ファンデルワールス半径に対する倍率）
    pub ball_scale: f32,
    /// ボール・スティックのスティックの半径（Å）
    pub stick_radius: f32,
    /// ライコリスのスティックの半径（Å）
    pub licorice_radius: f32,
    /// 線表示の太さ（円柱の半径、Å）
    pub line_radius: f32,
    /// 相互作用の線の太さ（円柱の半径、Å。SVG の書き出しも同じ）
    pub interaction_radius: f32,
    /// 相互作用の破線の、描く長さ（Å）
    pub interaction_dash: f32,
    /// 相互作用の破線の、空白の長さ（Å）
    pub interaction_gap: f32,
    /// 分子表面の格子間隔（Å）。小さいほど細かく、時間とメモリが増える
    pub surface_grid_spacing: f32,
    /// 分子表面のプローブ半径（Å）。0 はファンデルワールス面
    pub surface_probe_radius: f32,
    /// ボール・スティックのスティックの色（RGB、0.0〜1.0）
    pub bond_color: [f32; 3],
    /// リボンの幅と厚さ（Å。全体の長さ）
    pub ribbon_helix_width: f32,
    pub ribbon_helix_thickness: f32,
    pub ribbon_sheet_width: f32,
    pub ribbon_sheet_thickness: f32,
    /// ループ（細い管）の半径（Å）
    pub ribbon_loop_radius: f32,
    /// 選択の強調（グロー・ハロー）の大きさの倍率
    pub selection_size: f32,
    /// SSAO（隅や隙間を暗くする環境遮蔽）の強さ。0 でオフ
    pub ssao_strength: f32,
    /// SSAO が遮蔽を探す半径（Å）
    pub ssao_radius: f32,
}

impl Default for RenderSettings {
    fn default() -> Self {
        use crate::viewport::scene as sc;
        Self {
            ball_scale: sc::BALL_SCALE,
            stick_radius: sc::BALL_STICK_RADIUS,
            licorice_radius: sc::LICORICE_RADIUS,
            line_radius: sc::LINE_RADIUS,
            interaction_radius: sc::INTERACTION_STICK_RADIUS,
            interaction_dash: 0.35,
            interaction_gap: 0.25,
            surface_grid_spacing: yui_core::surface::SurfaceParams::default().grid_spacing,
            surface_probe_radius: yui_core::surface::SurfaceParams::default().probe_radius,
            bond_color: [0.7, 0.7, 0.7],
            ribbon_helix_width: 2.6,
            ribbon_helix_thickness: 0.7,
            ribbon_sheet_width: 2.4,
            ribbon_sheet_thickness: 0.3,
            ribbon_loop_radius: 0.25,
            selection_size: 1.0,
            ssao_strength: 0.0,
            ssao_radius: 3.0,
        }
    }
}

/// 利用者ごとの設定（検証済み。範囲外・見つからない値は `None` に戻してある）
#[derive(Debug, Clone, PartialEq, Default)]
pub struct UserSettings {
    /// UI のテーマ（書かれていなければ暗いテーマ）
    pub theme: ThemeKind,
    /// 選択の強調の方法（書かれていなければグロー）
    pub highlight: HighlightStyle,
    pub render: RenderSettings,
    pub editor: EditorSettings,
    pub terminal: TerminalSettings,
    /// メニューとタブの見た目（`ui:`）
    pub ui: UiSettings,
    /// 読み込むプラグインの名前（`plugins: enabled:`）。書かれていなければ空（何も読まない）
    pub plugins_enabled: Vec<String>,
}

/// 読み込んだ設定と、読み込み中に見つけた問題（警告）
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LoadedSettings {
    pub settings: UserSettings,
    pub warnings: Vec<String>,
}

/// 実際に使うフォント（名前と大きさ）
#[derive(Debug, Clone, PartialEq)]
pub struct FontChoice {
    pub family: String,
    pub size: f32,
    /// ターミナルの文字の大きさ
    pub terminal_size: f32,
}

/// 設定から、エディタとターミナルのフォントを決める（書かれていない項目は同梱フォントと既定の大きさ）。
pub fn font_choice(settings: &UserSettings) -> FontChoice {
    FontChoice {
        family: settings
            .editor
            .font_family
            .clone()
            .unwrap_or_else(|| fonts::MONO_FONT_NAME.to_string()),
        size: settings.editor.font_size.unwrap_or(fonts::EDITOR_FONT_SIZE),
        terminal_size: settings
            .terminal
            .font_size
            .unwrap_or(fonts::TERMINAL_FONT_SIZE),
    }
}

/// 同梱のフォントのファミリー名か（大文字・小文字は区別しない）
pub fn is_bundled_family(name: &str) -> bool {
    [fonts::MONO_FONT_NAME, fonts::UI_FONT_NAME]
        .iter()
        .any(|b| b.eq_ignore_ascii_case(name.trim()))
}

/// OS の設定フォルダから、設定ファイルのパスを求める（フォルダが分からなければ `None`）。
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub fn settings_path_in(config_dir: Option<PathBuf>) -> Option<PathBuf> {
    config_dir.map(|d| d.join("yui").join(SETTINGS_FILE_NAME))
}

/// 設定ファイルのパス（ネイティブ版）
#[cfg(not(target_arch = "wasm32"))]
pub fn settings_path() -> Option<PathBuf> {
    settings_path_in(dirs::config_dir())
}

/// 設定の文字列（YAML）を読み、項目ごとに検証する。
///
/// - 空・コメントだけなら、既定値（警告なし）
/// - YAML として読めない、または一番外が対応表（マッピング）でないなら、すべて既定値にして警告
/// - 項目の型が違う・範囲外・見つからないフォントなら、その項目だけ既定値にして警告
/// - 知らない項目は無視する（警告も出さない）
///
/// `source` は警告に書くファイルの名前、`family_available` はフォントのファミリー名が使えるかの判定。
pub fn parse_settings(
    text: &str,
    source: &str,
    family_available: &dyn Fn(&str) -> bool,
) -> LoadedSettings {
    let mut loaded = LoadedSettings::default();
    let value: serde_yaml::Value = match serde_yaml::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            loaded.warnings.push(format!(
                "{source}: invalid YAML ({e}); using default settings"
            ));
            return loaded;
        }
    };
    let root = match &value {
        serde_yaml::Value::Null => return loaded,
        serde_yaml::Value::Mapping(m) => m,
        _ => {
            loaded.warnings.push(format!(
                "{source}: expected a mapping of settings; using default settings"
            ));
            return loaded;
        }
    };

    match root.get("theme") {
        None | Some(serde_yaml::Value::Null) => {}
        Some(serde_yaml::Value::String(name)) => match ThemeKind::parse(name) {
            Some(kind) => loaded.settings.theme = kind,
            None => loaded.warnings.push(format!(
                "{source}: theme '{name}' is not 'dark' or 'light'; using dark"
            )),
        },
        Some(_) => loaded.warnings.push(format!(
            "{source}: theme must be 'dark' or 'light'; using dark"
        )),
    }

    match root.get("selection_highlight") {
        None | Some(serde_yaml::Value::Null) => {}
        Some(serde_yaml::Value::String(name)) => match HighlightStyle::parse(name) {
            Some(style) => loaded.settings.highlight = style,
            None => loaded.warnings.push(format!(
                "{source}: selection_highlight '{name}' is not 'glow', 'halo' or 'off'; using glow"
            )),
        },
        Some(_) => loaded.warnings.push(format!(
            "{source}: selection_highlight must be 'glow', 'halo' or 'off'; using glow"
        )),
    }

    parse_render_settings(root.get("render"), source, &mut loaded);
    parse_terminal_settings(root.get("terminal"), source, &mut loaded);
    parse_ui_settings(root.get("ui"), source, &mut loaded);
    parse_plugins_settings(root.get("plugins"), source, &mut loaded);

    let Some(editor) = root.get("editor") else {
        return loaded;
    };
    let editor = match editor {
        serde_yaml::Value::Null => return loaded,
        serde_yaml::Value::Mapping(m) => m,
        _ => {
            loaded
                .warnings
                .push(format!("{source}: 'editor' must be a mapping; ignored"));
            return loaded;
        }
    };

    match editor.get("font_family") {
        None | Some(serde_yaml::Value::Null) => {}
        Some(serde_yaml::Value::String(name)) if !name.trim().is_empty() => {
            let name = name.trim().to_string();
            if family_available(&name) {
                loaded.settings.editor.font_family = Some(name);
            } else {
                loaded.warnings.push(format!(
                    "{source}: font '{name}' was not found; using {}",
                    fonts::MONO_FONT_NAME
                ));
            }
        }
        Some(_) => loaded.warnings.push(format!(
            "{source}: editor.font_family must be a non-empty string; using {}",
            fonts::MONO_FONT_NAME
        )),
    }

    match editor.get("font_size") {
        None | Some(serde_yaml::Value::Null) => {}
        Some(v) => match v.as_f64() {
            Some(size) if (FONT_SIZE_MIN..=FONT_SIZE_MAX).contains(&size) => {
                loaded.settings.editor.font_size = Some(size as f32);
            }
            Some(size) => loaded.warnings.push(format!(
                "{source}: editor.font_size {size} is out of range ({FONT_SIZE_MIN} - {FONT_SIZE_MAX}); using {}",
                fonts::EDITOR_FONT_SIZE
            )),
            None => loaded.warnings.push(format!(
                "{source}: editor.font_size must be a number; using {}",
                fonts::EDITOR_FONT_SIZE
            )),
        },
    }

    match editor.get("scrollbar") {
        None | Some(serde_yaml::Value::Null) => {}
        Some(serde_yaml::Value::Bool(b)) => loaded.settings.editor.scrollbar = Some(*b),
        Some(_) => loaded.warnings.push(format!(
            "{source}: editor.scrollbar must be true or false; using true"
        )),
    }
    loaded
}

/// `plugins:` の項目（`enabled`: 読み込むプラグインの名前の一覧）。
/// 一覧でない・名前でない項目は無視して、警告する。
fn parse_plugins_settings(
    plugins: Option<&serde_yaml::Value>,
    source: &str,
    loaded: &mut LoadedSettings,
) {
    let map = match plugins {
        None | Some(serde_yaml::Value::Null) => return,
        Some(serde_yaml::Value::Mapping(m)) => m,
        Some(_) => {
            loaded
                .warnings
                .push(format!("{source}: 'plugins' must be a mapping; ignored"));
            return;
        }
    };
    match map.get("enabled") {
        None | Some(serde_yaml::Value::Null) => {}
        Some(serde_yaml::Value::Sequence(items)) => {
            for item in items {
                match item {
                    serde_yaml::Value::String(name) => {
                        loaded.settings.plugins_enabled.push(name.clone())
                    }
                    _ => loaded.warnings.push(format!(
                        "{source}: 'plugins.enabled' items must be plugin names; item ignored"
                    )),
                }
            }
        }
        Some(_) => loaded.warnings.push(format!(
            "{source}: 'plugins.enabled' must be a list of plugin names; ignored"
        )),
    }
}

/// `ui:` の項目（`tab_style`、`tab_font_size`、`corner_radius`、`hide_empty_docks`、`colors` / `dark_colors` /
/// `light_colors`）。
/// 型が違う・範囲外・知らない色の名前なら、その項目を既定値のままにして警告する。
fn parse_ui_settings(ui: Option<&serde_yaml::Value>, source: &str, loaded: &mut LoadedSettings) {
    let map = match ui {
        None | Some(serde_yaml::Value::Null) => return,
        Some(serde_yaml::Value::Mapping(m)) => m,
        Some(_) => {
            loaded
                .warnings
                .push(format!("{source}: 'ui' must be a mapping; ignored"));
            return;
        }
    };
    match map.get("tab_style") {
        None | Some(serde_yaml::Value::Null) => {}
        Some(serde_yaml::Value::String(name)) => match ui_style::TabStyle::parse(name) {
            Some(style) => loaded.settings.ui.tab_style = style,
            None => loaded.warnings.push(format!(
                "{source}: ui.tab_style '{name}' is not 'underline' or 'filled'; using underline"
            )),
        },
        Some(_) => loaded.warnings.push(format!(
            "{source}: ui.tab_style must be 'underline' or 'filled'; using underline"
        )),
    }
    match map.get("hide_empty_docks") {
        None | Some(serde_yaml::Value::Null) => {}
        Some(serde_yaml::Value::Bool(on)) => loaded.settings.ui.hide_empty_docks = *on,
        Some(_) => loaded.warnings.push(format!(
            "{source}: ui.hide_empty_docks must be true or false; using false"
        )),
    }
    for (key, min, max, default) in [
        (
            "tab_font_size",
            ui_style::TAB_FONT_SIZE_MIN,
            ui_style::TAB_FONT_SIZE_MAX,
            ui_style::TAB_FONT_SIZE_DEFAULT,
        ),
        (
            "corner_radius",
            ui_style::CORNER_RADIUS_MIN,
            ui_style::CORNER_RADIUS_MAX,
            ui_style::CORNER_RADIUS_DEFAULT,
        ),
    ] {
        match map.get(key) {
            None | Some(serde_yaml::Value::Null) => {}
            Some(v) => match v.as_f64() {
                Some(x) if (min..=max).contains(&x) => {
                    let slot = if key == "tab_font_size" {
                        &mut loaded.settings.ui.tab_font_size
                    } else {
                        &mut loaded.settings.ui.corner_radius
                    };
                    *slot = x as f32;
                }
                Some(x) => loaded.warnings.push(format!(
                    "{source}: ui.{key} {x} is out of range ({min} - {max}); using {default}"
                )),
                None => loaded.warnings.push(format!(
                    "{source}: ui.{key} must be a number; using {default}"
                )),
            },
        }
    }
    for key in ["colors", "dark_colors", "light_colors"] {
        let colors = match map.get(key) {
            None | Some(serde_yaml::Value::Null) => continue,
            Some(serde_yaml::Value::Mapping(m)) => m,
            Some(_) => {
                loaded
                    .warnings
                    .push(format!("{source}: ui.{key} must be a mapping; ignored"));
                continue;
            }
        };
        let mut list = Vec::new();
        for (name, value) in colors {
            let Some(name) = name.as_str() else {
                loaded.warnings.push(format!(
                    "{source}: ui.{key} has a key that is not text; ignored"
                ));
                continue;
            };
            if !ui_style::COLOR_NAMES.contains(&name) {
                loaded.warnings.push(format!(
                    "{source}: ui.{key}.{name} is not a color name ({}); ignored",
                    ui_style::COLOR_NAMES.join(", ")
                ));
                continue;
            }
            match value.as_str().and_then(ui_style::parse_hex_color) {
                Some(color) => list.push((name.to_string(), color)),
                None => loaded.warnings.push(format!(
                    "{source}: ui.{key}.{name} must be a color like \"#5b8def\" or \"#5b8def80\"; ignored"
                )),
            }
        }
        match key {
            "colors" => loaded.settings.ui.colors = list,
            "dark_colors" => loaded.settings.ui.dark_colors = list,
            _ => loaded.settings.ui.light_colors = list,
        }
    }
}

/// `terminal:` の項目（今は `font_size` だけ）。型が違う・範囲外なら、既定値にして警告する。
fn parse_terminal_settings(
    terminal: Option<&serde_yaml::Value>,
    source: &str,
    loaded: &mut LoadedSettings,
) {
    let map = match terminal {
        None | Some(serde_yaml::Value::Null) => return,
        Some(serde_yaml::Value::Mapping(m)) => m,
        Some(_) => {
            loaded
                .warnings
                .push(format!("{source}: 'terminal' must be a mapping; ignored"));
            return;
        }
    };
    match map.get("font_size") {
        None | Some(serde_yaml::Value::Null) => {}
        Some(v) => match v.as_f64() {
            Some(size) if (FONT_SIZE_MIN..=FONT_SIZE_MAX).contains(&size) => {
                loaded.settings.terminal.font_size = Some(size as f32);
            }
            Some(size) => loaded.warnings.push(format!(
                "{source}: terminal.font_size {size} is out of range ({FONT_SIZE_MIN} - {FONT_SIZE_MAX}); using {}",
                fonts::TERMINAL_FONT_SIZE
            )),
            None => loaded.warnings.push(format!(
                "{source}: terminal.font_size must be a number; using {}",
                fonts::TERMINAL_FONT_SIZE
            )),
        },
    }
}

/// `render:` の項目（数値、範囲つき）。型が違う・範囲外の項目だけ、既定値にして警告する。
fn parse_render_settings(
    render: Option<&serde_yaml::Value>,
    source: &str,
    loaded: &mut LoadedSettings,
) {
    let map = match render {
        None | Some(serde_yaml::Value::Null) => return,
        Some(serde_yaml::Value::Mapping(m)) => m,
        Some(_) => {
            loaded
                .warnings
                .push(format!("{source}: 'render' must be a mapping; ignored"));
            return;
        }
    };
    let mut pick = |key: &str, min: f64, max: f64, slot: &mut f32| match map.get(key) {
        None | Some(serde_yaml::Value::Null) => {}
        Some(v) => match v.as_f64() {
            Some(x) if (min..=max).contains(&x) => *slot = x as f32,
            Some(x) => loaded.warnings.push(format!(
                "{source}: render.{key} {x} is out of range ({min} - {max}); using {slot}"
            )),
            None => loaded.warnings.push(format!(
                "{source}: render.{key} must be a number; using {slot}"
            )),
        },
    };
    let mut r = RenderSettings::default();
    pick("ball_scale", 0.1, 1.0, &mut r.ball_scale);
    pick("stick_radius", 0.02, 0.5, &mut r.stick_radius);
    pick("licorice_radius", 0.05, 0.6, &mut r.licorice_radius);
    pick("line_radius", 0.01, 0.2, &mut r.line_radius);
    pick("interaction_radius", 0.02, 0.3, &mut r.interaction_radius);
    pick("interaction_dash", 0.05, 2.0, &mut r.interaction_dash);
    pick("interaction_gap", 0.0, 2.0, &mut r.interaction_gap);
    pick(
        "surface_grid_spacing",
        0.25,
        1.5,
        &mut r.surface_grid_spacing,
    );
    pick(
        "surface_probe_radius",
        0.0,
        3.0,
        &mut r.surface_probe_radius,
    );
    pick("ribbon_helix_width", 0.4, 6.0, &mut r.ribbon_helix_width);
    pick(
        "ribbon_helix_thickness",
        0.1,
        2.0,
        &mut r.ribbon_helix_thickness,
    );
    pick("ribbon_sheet_width", 0.4, 6.0, &mut r.ribbon_sheet_width);
    pick(
        "ribbon_sheet_thickness",
        0.1,
        2.0,
        &mut r.ribbon_sheet_thickness,
    );
    pick("ribbon_loop_radius", 0.05, 1.0, &mut r.ribbon_loop_radius);
    pick("selection_size", 0.5, 3.0, &mut r.selection_size);
    pick("ssao_strength", 0.0, 1.0, &mut r.ssao_strength);
    pick("ssao_radius", 0.3, 10.0, &mut r.ssao_radius);
    match map.get("bond_color") {
        None | Some(serde_yaml::Value::Null) => {}
        Some(serde_yaml::Value::Sequence(seq)) => {
            let rgb: Vec<f64> = seq.iter().filter_map(serde_yaml::Value::as_f64).collect();
            if seq.len() == 3 && rgb.len() == 3 && rgb.iter().all(|c| (0.0..=1.0).contains(c)) {
                r.bond_color = [rgb[0] as f32, rgb[1] as f32, rgb[2] as f32];
            } else {
                loaded.warnings.push(format!(
                    "{source}: render.bond_color must be three numbers between 0 and 1, like [0.7, 0.7, 0.7]; using {:?}",
                    r.bond_color
                ));
            }
        }
        Some(_) => loaded.warnings.push(format!(
            "{source}: render.bond_color must be three numbers between 0 and 1, like [0.7, 0.7, 0.7]; using {:?}",
            r.bond_color
        )),
    }
    loaded.settings.render = r;
}

/// フォントのファミリー名が使えるか。同梱のフォントは常に使える。
/// ネイティブ版では OS に入っているフォントも調べる。ブラウザ版では同梱のフォントだけ。
pub fn family_available(name: &str) -> bool {
    if is_bundled_family(name) {
        return true;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        installed_families()
            .iter()
            .any(|f| f.eq_ignore_ascii_case(name))
    }
    #[cfg(target_arch = "wasm32")]
    {
        false
    }
}

/// OS に入っているフォントのファミリー名（初めて必要になったときに 1 回だけ調べる）
#[cfg(not(target_arch = "wasm32"))]
fn installed_families() -> &'static Vec<String> {
    static FAMILIES: OnceLock<Vec<String>> = OnceLock::new();
    FAMILIES.get_or_init(|| {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        db.faces()
            .flat_map(|f| f.families.iter().map(|(name, _)| name.clone()))
            .collect()
    })
}

/// 起動時の設定の読み込み（ファイルがなければ既定値）
#[cfg(not(target_arch = "wasm32"))]
pub fn load_startup_settings() -> LoadedSettings {
    let Some(path) = settings_path() else {
        return LoadedSettings::default();
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => parse_settings(&text, &path.display().to_string(), &family_available),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => LoadedSettings::default(),
        Err(e) => LoadedSettings {
            settings: UserSettings::default(),
            warnings: vec![format!(
                "Could not read settings file '{}': {e}; using default settings",
                path.display()
            )],
        },
    }
}

/// 起動時の設定の読み込み（ブラウザの `localStorage`。なければ既定値）
#[cfg(target_arch = "wasm32")]
pub fn load_startup_settings() -> LoadedSettings {
    let stored = web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .and_then(|s| s.get_item(STORAGE_KEY).ok().flatten());
    match stored {
        Some(text) => parse_settings(&text, "localStorage 'yui.settings'", &family_available),
        None => LoadedSettings::default(),
    }
}

/// 設定ファイルがなければ、コメントだけの雛形を作る（親のフォルダも作る）。すでにあれば何もしない。
pub fn ensure_default_file(path: &Path) -> std::io::Result<()> {
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, DEFAULT_SETTINGS_TEMPLATE)
}

static STARTUP: OnceLock<LoadedSettings> = OnceLock::new();

/// 起動時に読んだ設定を、アプリ全体で参照できるように記録する（`main` で 1 回だけ呼ぶ）。
pub fn set_startup(loaded: LoadedSettings) {
    let _ = STARTUP.set(loaded);
}

/// 起動時に読んだ設定のテーマ（設定を記録していなければ暗いテーマ）
pub fn startup_theme() -> ThemeKind {
    STARTUP.get().map(|l| l.settings.theme).unwrap_or_default()
}

/// 起動時に読んだ設定の、選択の強調の方法（設定を記録していなければグロー）
pub fn startup_highlight() -> HighlightStyle {
    STARTUP
        .get()
        .map(|l| l.settings.highlight)
        .unwrap_or_default()
}

/// 起動時に読んだ設定の、描画の大きさ（設定を記録していなければ既定値）
pub fn startup_render() -> RenderSettings {
    STARTUP.get().map(|l| l.settings.render).unwrap_or_default()
}

/// 設定の文字列の `theme:` を書き換える（コメントなどの他の行は、そのまま残す）。
///
/// 値の入っている行（行頭が `theme:`）があればそれを置き換え、なければコメントアウトされた
/// `#theme:` の行を置き換え、それもなければ末尾に足す。
#[cfg(test)]
pub fn set_theme_in_text(text: &str, kind: ThemeKind) -> String {
    set_key_in_text(text, "theme", kind.as_str())
}

/// 設定の文字列の、トップレベルの `key:` を書き換える（`set_theme_in_text` と同じ規則）。
pub fn set_key_in_text(text: &str, key: &str, value: &str) -> String {
    let line = format!("{key}: {value}");
    let (active_prefix, commented_prefix) = (format!("{key}:"), format!("#{key}:"));
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let active = lines.iter().position(|l| l.starts_with(&active_prefix));
    let commented = lines.iter().position(|l| l.starts_with(&commented_prefix));
    match active.or(commented) {
        Some(i) => lines[i] = line,
        None => lines.push(line),
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// 設定の 1 項目（トップレベルの `key`）を、利用者ごとの設定に書き込む（ネイティブ版: 設定ファイル。ファイルがなければ雛形から作る）。
/// 他の設定とコメントは残す。失敗したときは、画面に出す説明を返す。
#[cfg(not(target_arch = "wasm32"))]
pub fn save_setting(key: &str, value: &str) -> Result<(), String> {
    let path = settings_path().ok_or_else(|| "no settings folder on this system".to_string())?;
    let current = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => DEFAULT_SETTINGS_TEMPLATE.to_string(),
        Err(e) => return Err(format!("could not read '{}': {e}", path.display())),
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("could not create '{}': {e}", parent.display()))?;
    }
    std::fs::write(&path, set_key_in_text(&current, key, value))
        .map_err(|e| format!("could not write '{}': {e}", path.display()))
}

/// 設定の 1 項目を、利用者ごとの設定に書き込む（ブラウザ版: `localStorage`）。
#[cfg(target_arch = "wasm32")]
pub fn save_setting(key: &str, value: &str) -> Result<(), String> {
    let storage = web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .ok_or_else(|| "localStorage is not available".to_string())?;
    let current = storage
        .get_item(STORAGE_KEY)
        .ok()
        .flatten()
        .unwrap_or_else(|| DEFAULT_SETTINGS_TEMPLATE.to_string());
    storage
        .set_item(STORAGE_KEY, &set_key_in_text(&current, key, value))
        .map_err(|_| "could not write to localStorage".to_string())
}

/// 選んだテーマを、利用者ごとの設定に書き込む
pub fn save_theme(kind: ThemeKind) -> Result<(), String> {
    save_setting("theme", kind.as_str())
}

/// 選んだ選択の強調の方法を、利用者ごとの設定に書き込む
pub fn save_highlight(style: HighlightStyle) -> Result<(), String> {
    save_setting("selection_highlight", style.as_str())
}

/// 起動時の設定の警告（ステータスバーに出す）。設定を記録していなければ空。
pub fn startup_warnings() -> Vec<String> {
    STARTUP
        .get()
        .map(|l| l.warnings.clone())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn any(_: &str) -> bool {
        true
    }
    fn none(_: &str) -> bool {
        false
    }
    fn bundled_only(name: &str) -> bool {
        is_bundled_family(name)
    }

    #[test]
    fn test_empty_and_comment_only_give_defaults_without_warnings() {
        for text in ["", "   \n", "# only a comment\n", DEFAULT_SETTINGS_TEMPLATE] {
            let l = parse_settings(text, "s.yaml", &any);
            assert_eq!(l, LoadedSettings::default(), "{text:?}");
        }
    }

    #[test]
    fn test_valid_settings_are_read() {
        let l = parse_settings(
            "editor:\n  font_family: \"Fira Code\"\n  font_size: 16\n",
            "s.yaml",
            &any,
        );
        assert!(l.warnings.is_empty());
        assert_eq!(l.settings.editor.font_family.as_deref(), Some("Fira Code"));
        assert_eq!(l.settings.editor.font_size, Some(16.0));
        // 小数の大きさも読める。境界の値も有効
        for size in ["6", "72", "13.5"] {
            let l = parse_settings(&format!("editor:\n  font_size: {size}\n"), "s", &any);
            assert!(l.warnings.is_empty(), "{size}");
            assert_eq!(l.settings.editor.font_size, size.parse::<f32>().ok());
        }
    }

    #[test]
    fn test_partial_settings_keep_the_rest_default() {
        let l = parse_settings("editor:\n  font_size: 20\n", "s.yaml", &any);
        assert_eq!(l.settings.editor.font_family, None);
        assert_eq!(l.settings.editor.font_size, Some(20.0));
        let l = parse_settings("editor:\n  font_family: JetBrains Mono\n", "s.yaml", &any);
        assert_eq!(
            l.settings.editor.font_family.as_deref(),
            Some("JetBrains Mono")
        );
        assert_eq!(l.settings.editor.font_size, None);
        // editor が空でも、editor がなくても警告は出ない
        assert!(parse_settings("editor:\n", "s", &any).warnings.is_empty());
        assert!(parse_settings("other: 1\n", "s", &any).warnings.is_empty());
    }

    #[test]
    fn test_broken_yaml_gives_defaults_and_names_the_file() {
        let l = parse_settings("editor: [unclosed\n  font_size: : :\n", "~/s.yaml", &any);
        assert_eq!(l.settings, UserSettings::default());
        assert_eq!(l.warnings.len(), 1);
        assert!(l.warnings[0].contains("~/s.yaml") && l.warnings[0].contains("invalid YAML"));

        // 一番外が対応表でない
        let l = parse_settings("- a\n- b\n", "s.yaml", &any);
        assert_eq!(l.settings, UserSettings::default());
        assert!(l.warnings[0].contains("mapping"));
        let l = parse_settings("editor: 5\n", "s.yaml", &any);
        assert_eq!(l.settings, UserSettings::default());
        assert!(l.warnings[0].contains("'editor' must be a mapping"));
    }

    #[test]
    fn test_type_errors_only_drop_that_item() {
        let l = parse_settings(
            "editor:\n  font_family: 12\n  font_size: big\n",
            "s.yaml",
            &any,
        );
        assert_eq!(l.settings, UserSettings::default());
        assert_eq!(l.warnings.len(), 2);

        // 片方が間違っていても、もう片方は使われる
        let l = parse_settings(
            "editor:\n  font_family: Fira Code\n  font_size: big\n",
            "s.yaml",
            &any,
        );
        assert_eq!(l.settings.editor.font_family.as_deref(), Some("Fira Code"));
        assert_eq!(l.settings.editor.font_size, None);
        assert_eq!(l.warnings.len(), 1);
        assert!(l.warnings[0].contains("font_size must be a number"));

        // 空の名前
        let l = parse_settings("editor:\n  font_family: \"  \"\n", "s", &any);
        assert_eq!(l.settings.editor.font_family, None);
        assert_eq!(l.warnings.len(), 1);
    }

    #[test]
    fn test_font_size_out_of_range_falls_back_with_warning() {
        for size in ["5.9", "0", "-3", "72.1", "1000", ".nan", ".inf"] {
            let l = parse_settings(&format!("editor:\n  font_size: {size}\n"), "s.yaml", &any);
            assert_eq!(l.settings.editor.font_size, None, "{size}");
            assert_eq!(l.warnings.len(), 1, "{size}");
            assert!(l.warnings[0].contains("out of range"), "{size}");
        }
    }

    #[test]
    fn test_render_settings_are_read_and_validated() {
        let l = parse_settings("", "s.yaml", &any);
        assert_eq!(l.settings.render, RenderSettings::default());

        let l = parse_settings(
            "render:\n  ball_scale: 0.5\n  interaction_radius: 0.15\n",
            "s.yaml",
            &any,
        );
        assert!(l.warnings.is_empty());
        assert_eq!(l.settings.render.ball_scale, 0.5);
        assert_eq!(l.settings.render.interaction_radius, 0.15);
        assert_eq!(
            l.settings.render.stick_radius,
            RenderSettings::default().stick_radius
        );

        // 範囲外・型違いは、その項目だけ既定値にして警告
        let l = parse_settings(
            "render:\n  ball_scale: 5\n  stick_radius: thick\n  line_radius: 0.05\n",
            "s.yaml",
            &any,
        );
        assert_eq!(l.warnings.len(), 2);
        assert_eq!(
            l.settings.render.ball_scale,
            RenderSettings::default().ball_scale
        );
        assert_eq!(
            l.settings.render.stick_radius,
            RenderSettings::default().stick_radius
        );
        assert_eq!(l.settings.render.line_radius, 0.05);

        // 追加の項目: 破線、分子表面、CUBE
        let l = parse_settings(
            "render:\n  interaction_dash: 0.2\n  interaction_gap: 0\n  surface_grid_spacing: 0.4\n  surface_probe_radius: 1.4\n",
            "s.yaml",
            &any,
        );
        assert!(l.warnings.is_empty());
        let r = l.settings.render;
        assert_eq!(
            (
                r.interaction_dash,
                r.interaction_gap,
                r.surface_grid_spacing
            ),
            (0.2, 0.0, 0.4)
        );
        assert_eq!(r.surface_probe_radius, 1.4);
        let l = parse_settings(
            "render:\n  surface_grid_spacing: 0.05\n  interaction_dash: 0\n",
            "s.yaml",
            &any,
        );
        assert_eq!(l.warnings.len(), 2);
        assert_eq!(l.settings.render, RenderSettings::default());

        // SSAO
        let l = parse_settings(
            "render:\n  ssao_strength: 0.7\n  ssao_radius: 5\n",
            "s.yaml",
            &any,
        );
        assert!(l.warnings.is_empty(), "{:?}", l.warnings);
        assert_eq!(
            (
                l.settings.render.ssao_strength,
                l.settings.render.ssao_radius
            ),
            (0.7, 5.0)
        );
        assert_eq!(
            RenderSettings::default().ssao_strength,
            0.0,
            "off by default"
        );
        let l = parse_settings(
            "render:\n  ssao_strength: 2\n  ssao_radius: 0\n",
            "s.yaml",
            &any,
        );
        assert_eq!(l.warnings.len(), 2);
        assert_eq!(l.settings.render, RenderSettings::default());

        // リボン、結合の色、選択の強調
        let l = parse_settings(
            "render:\n  ribbon_helix_width: 4\n  ribbon_loop_radius: 0.4\n  selection_size: 2\n  bond_color: [1, 0.5, 0]\n",
            "s.yaml",
            &any,
        );
        assert!(l.warnings.is_empty(), "{:?}", l.warnings);
        let r = l.settings.render;
        assert_eq!((r.ribbon_helix_width, r.ribbon_loop_radius), (4.0, 0.4));
        assert_eq!((r.selection_size, r.bond_color), (2.0, [1.0, 0.5, 0.0]));
        assert_eq!(
            r.ribbon_sheet_width,
            RenderSettings::default().ribbon_sheet_width
        );
        // 色は、3 つの 0〜1 の数でなければ、既定の色にして警告
        for bad in ["[1, 0.5]", "[1, 2, 0]", "red", "[a, b, c]", "[0, 0, 0, 0]"] {
            let l = parse_settings(&format!("render:\n  bond_color: {bad}\n"), "s.yaml", &any);
            assert_eq!(l.warnings.len(), 1, "{bad}");
            assert_eq!(
                l.settings.render.bond_color,
                RenderSettings::default().bond_color,
                "{bad}"
            );
        }
        let l = parse_settings(
            "render:\n  ribbon_sheet_thickness: 9\n  selection_size: 0\n",
            "s.yaml",
            &any,
        );
        assert_eq!(l.warnings.len(), 2);
        assert_eq!(l.settings.render, RenderSettings::default());

        let l = parse_settings("render: 3\n", "s.yaml", &any);
        assert_eq!(l.warnings.len(), 1);
    }

    #[test]
    fn test_unknown_keys_are_ignored_without_warnings() {
        let l = parse_settings(
            "theme: dark\neditor:\n  font_size: 15\n  tab_width: 4\nfuture:\n  a: [1, 2]\n",
            "s.yaml",
            &any,
        );
        assert!(l.warnings.is_empty());
        assert_eq!(l.settings.editor.font_size, Some(15.0));
    }

    #[test]
    fn test_missing_font_falls_back_with_warning() {
        let l = parse_settings(
            "editor:\n  font_family: No Such Font\n",
            "s.yaml",
            &bundled_only,
        );
        assert_eq!(l.settings.editor.font_family, None);
        assert_eq!(l.warnings.len(), 1);
        assert!(l.warnings[0].contains("'No Such Font' was not found"));
        assert!(l.warnings[0].contains("JetBrains Mono"));

        // 同梱のフォントは、大文字・小文字を区別せずに使える
        let l = parse_settings(
            "editor:\n  font_family: jetbrains mono\n",
            "s",
            &bundled_only,
        );
        assert!(l.warnings.is_empty());
        assert_eq!(
            l.settings.editor.font_family.as_deref(),
            Some("jetbrains mono")
        );
        // 何も使えない環境では警告になる
        let l = parse_settings("editor:\n  font_family: X\n", "s", &none);
        assert_eq!(l.warnings.len(), 1);
    }

    #[test]
    fn test_bundled_families_are_always_available() {
        assert!(family_available("JetBrains Mono"));
        assert!(family_available(" noto sans jp "));
        assert!(is_bundled_family("JETBRAINS MONO"));
        assert!(!is_bundled_family("Courier New"));
    }

    #[test]
    fn test_font_choice_defaults_and_overrides() {
        let default = font_choice(&UserSettings::default());
        assert_eq!(default.family, fonts::MONO_FONT_NAME);
        assert_eq!(default.size, fonts::EDITOR_FONT_SIZE);
        assert_eq!(default.terminal_size, fonts::TERMINAL_FONT_SIZE);

        let custom = font_choice(&UserSettings {
            editor: EditorSettings {
                font_family: Some("Fira Code".into()),
                font_size: Some(18.0),
                scrollbar: None,
            },
            ..UserSettings::default()
        });
        assert_eq!(custom.family, "Fira Code");
        assert_eq!(custom.size, 18.0);

        // 設定のファイルから決めたフォントが、そのまま使われる
        let l = parse_settings(
            "editor:\n  font_family: Fira Code\n  font_size: 11\n",
            "s",
            &any,
        );
        let c = font_choice(&l.settings);
        assert_eq!((c.family.as_str(), c.size), ("Fira Code", 11.0));
    }

    #[test]
    fn test_settings_path_from_config_dir() {
        let p = settings_path_in(Some(PathBuf::from("/home/u/.config"))).unwrap();
        assert_eq!(p, PathBuf::from("/home/u/.config/yui/settings.yaml"));
        // OS の設定フォルダが分からないときは、ファイルを使わない
        assert_eq!(settings_path_in(None), None);
    }

    #[test]
    fn test_ensure_default_file_creates_once_and_never_overwrites() {
        let dir = std::env::temp_dir().join(format!("yui-settings-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("nested").join("yui").join(SETTINGS_FILE_NAME);

        ensure_default_file(&path).expect("create");
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text, DEFAULT_SETTINGS_TEMPLATE);
        // 雛形は、そのまま読める（既定値、警告なし）
        assert_eq!(parse_settings(&text, "s", &any), LoadedSettings::default());

        // 利用者が書き換えたファイルは、上書きしない
        std::fs::write(&path, "editor:\n  font_size: 20\n").unwrap();
        ensure_default_file(&path).expect("keep");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "editor:\n  font_size: 20\n"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_ui_section_is_read_and_checked() {
        use crate::ui_style::TabStyle;
        let any = |_: &str| true;
        let l = parse_settings(
            "ui:\n  tab_style: Filled\n  tab_font_size: 14\n  corner_radius: 4\n  hide_empty_docks: true\n  colors:\n    accent: \"#ff8800\"\n  dark_colors:\n    bar_bg: \"#10121880\"\n",
            "s",
            &any,
        );
        assert!(l.warnings.is_empty(), "{:?}", l.warnings);
        let ui = &l.settings.ui;
        assert_eq!(ui.tab_style, TabStyle::Filled);
        assert_eq!((ui.tab_font_size, ui.corner_radius), (14.0, 4.0));
        assert!(ui.hide_empty_docks);
        assert_eq!(ui.colors.len(), 1);
        assert_eq!(ui.colors[0].0, "accent");
        assert_eq!(ui.dark_colors[0].0, "bar_bg");
        assert!(ui.light_colors.is_empty());
        // 書かれていなければ既定値
        assert_eq!(
            parse_settings("", "s", &any).settings.ui,
            UiSettings::default()
        );
    }

    #[test]
    fn test_bad_ui_values_are_kept_at_the_default_with_warnings() {
        let any = |_: &str| true;
        let text = "ui:\n  tab_style: pill\n  tab_font_size: 99\n  corner_radius: abc\n  hide_empty_docks: yes please\n  colors:\n    accent: red\n    nope: \"#ffffff\"\n    text: \"#ffffff\"\n";
        let l = parse_settings(text, "s", &any);
        let ui = &l.settings.ui;
        assert_eq!(ui.tab_style, crate::ui_style::TabStyle::Underline);
        assert_eq!(ui.tab_font_size, crate::ui_style::TAB_FONT_SIZE_DEFAULT);
        assert_eq!(ui.corner_radius, crate::ui_style::CORNER_RADIUS_DEFAULT);
        assert!(!ui.hide_empty_docks);
        // 正しい色だけが残る
        assert_eq!(ui.colors.len(), 1);
        assert_eq!(ui.colors[0].0, "text");
        // tab_style、tab_font_size、corner_radius、hide_empty_docks、accent、nope の 6 つ
        assert_eq!(l.warnings.len(), 6, "{:?}", l.warnings);
        assert!(
            l.warnings.iter().all(|w| w.contains("ui.")),
            "{:?}",
            l.warnings
        );
        // 'ui' が mapping でない
        let l = parse_settings("ui: 3\n", "s", &any);
        assert_eq!(l.warnings.len(), 1);
        assert_eq!(l.settings.ui, UiSettings::default());
    }

    #[test]
    fn test_plugins_enabled_is_a_list_of_names() {
        let l = parse_settings(
            "plugins:\n  enabled:\n    - sample\n    - my-plugin\n",
            "s",
            &any,
        );
        assert!(l.warnings.is_empty(), "{:?}", l.warnings);
        assert_eq!(l.settings.plugins_enabled, ["sample", "my-plugin"]);

        // 名前でない項目と、一覧でない値は無視して、警告する
        let l = parse_settings("plugins:\n  enabled:\n    - 3\n", "s", &any);
        assert!(l.settings.plugins_enabled.is_empty());
        assert_eq!(l.warnings.len(), 1, "{:?}", l.warnings);

        let l = parse_settings("plugins:\n  enabled: sample\n", "s", &any);
        assert!(l.settings.plugins_enabled.is_empty());
        assert_eq!(l.warnings.len(), 1, "{:?}", l.warnings);

        // 無ければ何も読まない
        assert!(
            parse_settings("", "s", &any)
                .settings
                .plugins_enabled
                .is_empty()
        );
    }

    #[test]
    fn test_theme_is_read_and_defaults_to_dark() {
        assert_eq!(
            parse_settings("", "s", &any).settings.theme,
            ThemeKind::Dark
        );
        for (text, want) in [
            ("theme: light\n", ThemeKind::Light),
            ("theme: Dark\n", ThemeKind::Dark),
            ("theme: \" LIGHT \"\n", ThemeKind::Light),
        ] {
            let l = parse_settings(text, "s", &any);
            assert!(l.warnings.is_empty(), "{text:?}");
            assert_eq!(l.settings.theme, want, "{text:?}");
        }
        // 他の項目と一緒でも読める
        let l = parse_settings("theme: light\neditor:\n  font_size: 15\n", "s", &any);
        assert_eq!(l.settings.theme, ThemeKind::Light);
        assert_eq!(l.settings.editor.font_size, Some(15.0));
    }

    #[test]
    fn test_bad_theme_falls_back_to_dark_with_warning() {
        for text in ["theme: blue\n", "theme: 3\n", "theme: [a]\n"] {
            let l = parse_settings(text, "s.yaml", &any);
            assert_eq!(l.settings.theme, ThemeKind::Dark, "{text:?}");
            assert_eq!(l.warnings.len(), 1, "{text:?}");
            assert!(l.warnings[0].contains("theme"), "{text:?}");
        }
        // 壊れたテーマでも、他の項目は使われる
        let l = parse_settings("theme: blue\neditor:\n  font_size: 15\n", "s", &any);
        assert_eq!(l.settings.editor.font_size, Some(15.0));
    }

    #[test]
    fn test_set_theme_in_text_keeps_the_rest() {
        // 雛形のコメントアウトされた行を置き換える
        let out = set_theme_in_text(DEFAULT_SETTINGS_TEMPLATE, ThemeKind::Light);
        assert!(out.contains("\ntheme: light\n"));
        assert!(!out.contains("#theme:"));
        assert!(out.contains("#editor:") && out.contains("# YUI user settings."));
        assert_eq!(
            parse_settings(&out, "s", &any).settings.theme,
            ThemeKind::Light
        );

        // 値の入っている行を置き換える。コメントと他の項目は残る
        let text = "# mine\ntheme: light\neditor:\n  font_size: 20\n";
        let out = set_theme_in_text(text, ThemeKind::Dark);
        assert_eq!(out, "# mine\ntheme: dark\neditor:\n  font_size: 20\n");

        // ない場合は末尾に足す（空でも、改行で終わらなくても）
        assert_eq!(set_theme_in_text("", ThemeKind::Light), "theme: light\n");
        assert_eq!(
            set_theme_in_text("editor:\n  font_size: 9", ThemeKind::Light),
            "editor:\n  font_size: 9\ntheme: light\n"
        );

        // 2 回続けても 1 行のまま
        let twice = set_theme_in_text(
            &set_theme_in_text("a: 1\n", ThemeKind::Light),
            ThemeKind::Dark,
        );
        assert_eq!(twice, "a: 1\ntheme: dark\n");
    }

    #[test]
    fn test_selection_highlight_is_read_and_defaults_to_glow() {
        assert_eq!(
            parse_settings("", "s", &any).settings.highlight,
            HighlightStyle::Glow
        );
        for (text, want) in [
            ("selection_highlight: halo\n", HighlightStyle::Halo),
            ("selection_highlight: Off\n", HighlightStyle::Off),
            ("selection_highlight: \" GLOW \"\n", HighlightStyle::Glow),
        ] {
            let l = parse_settings(text, "s", &any);
            assert!(l.warnings.is_empty(), "{text:?}");
            assert_eq!(l.settings.highlight, want, "{text:?}");
        }
        // 壊れた値は、警告して既定（グロー）。他の項目は使われる
        for text in ["selection_highlight: blur\n", "selection_highlight: 3\n"] {
            let l = parse_settings(&format!("{text}theme: light\n"), "s.yaml", &any);
            assert_eq!(l.settings.highlight, HighlightStyle::Glow, "{text:?}");
            assert_eq!(l.settings.theme, ThemeKind::Light, "{text:?}");
            assert_eq!(l.warnings.len(), 1, "{text:?}");
            assert!(l.warnings[0].contains("selection_highlight"), "{text:?}");
        }
    }

    #[test]
    fn test_editor_scrollbar_setting() {
        let any = |_: &str| true;
        assert_eq!(
            parse_settings("editor:\n  scrollbar: false\n", "s", &any)
                .settings
                .editor
                .scrollbar,
            Some(false)
        );
        assert_eq!(
            parse_settings("editor:\n  scrollbar: true\n", "s", &any)
                .settings
                .editor
                .scrollbar,
            Some(true)
        );
        // 書かれていなければ None（出す）。真偽値でなければ警告して None
        assert_eq!(
            parse_settings("editor:\n", "s", &any)
                .settings
                .editor
                .scrollbar,
            None
        );
        let l = parse_settings("editor:\n  scrollbar: maybe\n", "s.yaml", &any);
        assert_eq!(l.settings.editor.scrollbar, None);
        assert!(l.warnings[0].contains("editor.scrollbar"));
    }

    #[test]
    fn test_set_key_in_text_handles_several_keys() {
        // 雛形の、コメントアウトされた 2 つの行を、それぞれ置き換える
        let out = set_key_in_text(DEFAULT_SETTINGS_TEMPLATE, "selection_highlight", "halo");
        let out = set_key_in_text(&out, "theme", "light");
        assert!(out.contains("\nselection_highlight: halo\n"));
        assert!(out.contains("\ntheme: light\n"));
        assert!(!out.contains("#selection_highlight:") && !out.contains("#theme:"));
        let l = parse_settings(&out, "s", &any);
        assert!(l.warnings.is_empty());
        assert_eq!(l.settings.highlight, HighlightStyle::Halo);
        assert_eq!(l.settings.theme, ThemeKind::Light);

        // 名前が前方一致する別のキーは、書き換えない
        let out = set_key_in_text("theme_extra: 1\n", "theme", "dark");
        assert_eq!(out, "theme_extra: 1\ntheme: dark\n");
    }

    #[test]
    fn test_terminal_font_size_is_read_and_validated() {
        let l = parse_settings("terminal:\n  font_size: 16\n", "s", &any);
        assert_eq!(l.settings.terminal.font_size, Some(16.0));
        assert!(l.warnings.is_empty());
        assert_eq!(font_choice(&l.settings).terminal_size, 16.0);
        // editor の大きさとは別
        let l = parse_settings(
            "editor:\n  font_size: 20\nterminal:\n  font_size: 9\n",
            "s",
            &any,
        );
        let c = font_choice(&l.settings);
        assert_eq!((c.size, c.terminal_size), (20.0, 9.0));
        // 範囲外・型違い・形違いは、既定値に戻して警告する（ほかの項目は読む）
        for bad in ["2", "100", "big"] {
            let l = parse_settings(&format!("terminal:\n  font_size: {bad}\n"), "s", &any);
            assert_eq!(l.settings.terminal.font_size, None, "{bad}");
            assert_eq!(l.warnings.len(), 1, "{bad}");
            assert!(l.warnings[0].contains("terminal.font_size"));
        }
        let l = parse_settings("terminal: 5\neditor:\n  font_size: 15\n", "s", &any);
        assert!(l.warnings[0].contains("'terminal' must be a mapping"));
        assert_eq!(l.settings.editor.font_size, Some(15.0));
    }
}
