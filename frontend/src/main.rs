mod app;
mod app_icon;
mod backend_auth;
mod camera_anim;
mod canvas;
mod context_menu;
mod cube_view;
mod culling;
mod file_open_dispatch;
mod fonts;
mod interaction_render;
mod lod;
mod log_viewer;
mod math;
mod menu;
mod menu_bar;
mod movie_export;
mod named_selections;
mod pane_view;
mod playback;
mod playback_bar;
mod png_export;
mod project_tree_pane;
// UI からはまだ直接呼ばない（プラグインの読み込みが使う）。メニューとコマンドへの配線は 6k-iii
#[cfg(all(feature = "python", not(target_arch = "wasm32")))]
#[allow(dead_code)]
mod python_runtime;
// プラグインの読み込み（6k-ii）。一覧の表示など、まだ使わない項目があるので dead_code を許す
#[cfg(not(target_arch = "wasm32"))]
#[allow(dead_code)]
mod plugins;
mod representation_inspector;
mod ribbon;
mod selection;
mod share_dialog;
mod state_sync;
mod status_bar;
mod streams;
mod structure_tree;
mod surface_render;
mod svg_export;
mod terminal_grid;
mod terminal_pane;
mod terminal_widget;
mod text_editor_pane;
mod trajectory;
mod ui_style;
mod user_settings;
mod viewport;
mod workspace_file_io;
mod zip_store;

pub use app::{Message, PaneKind, Yui};

pub fn main() -> iced::Result {
    #[cfg(target_arch = "wasm32")]
    {
        console_error_panic_hook::set_once();
        web_sys::console::log_1(&"[YUI WASM] Initializing Yui frontend on WebAssembly...".into());
    }

    // 利用者ごとの設定を、起動時に 1 回だけ読む（変えたら再起動が必要）。壊れた設定でも止めず、警告はステータスバーに出す。
    // backend に渡す秘密（--token / YUI_TOKEN / 接続ファイル、ブラウザは URL の #token=）
    backend_auth::init();
    let loaded = user_settings::load_startup_settings();
    // `plugins: enabled:` に書かれたプラグインを読む。失敗の理由は、設定の警告と同じくステータスバーに出す
    #[cfg(not(target_arch = "wasm32"))]
    let loaded = {
        let mut loaded = loaded;
        loaded
            .warnings
            .extend(plugins::start(&loaded.settings.plugins_enabled));
        loaded
    };
    fonts::init(&user_settings::font_choice(&loaded.settings));
    text_editor_pane::set_scrollbar(loaded.settings.editor.scrollbar.unwrap_or(true));
    crate::ui_style::set_theme(loaded.settings.theme);
    crate::ui_style::set_ui_settings(loaded.settings.ui.clone());
    user_settings::set_startup(loaded);

    iced::application(Yui::new, Yui::update, Yui::view)
        // ウィンドウのアイコン（デスクトップ版）。`exit_on_close_request` は、下で設定し直すので、先に置く
        .window(iced::window::Settings {
            icon: app_icon::window_icon(),
            ..Default::default()
        })
        // WASM上ではOSのシステムフォントを列挙できずcosmic-textが一切グリフを描画できないため、
        // 日本語UI文字列（"結" 等）を含む全テキスト表示に本フォントの同梱が必須。
        .font(include_bytes!("../assets/NotoSansJP.ttf").as_slice())
        // エディタとターミナルの等幅フォント（fonts.rs）。WASM では OS のフォントが使えないため同梱が必須。
        .font(fonts::MONO_FONT_REGULAR)
        .font(fonts::MONO_FONT_BOLD)
        .default_font(iced::Font::with_name(fonts::UI_FONT_NAME))
        // ウィンドウを閉じる操作は、未保存のファイルを確認してから終了するため、自分で処理する（Yui::update）
        .exit_on_close_request(false)
        .theme(Yui::theme)
        .subscription(Yui::subscription)
        .title(Yui::title)
        .run()
}
