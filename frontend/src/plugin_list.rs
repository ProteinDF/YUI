//! プラグインの一覧（段階 6k-iv、architecture.md §6.4）。View →「Plugins...」で開く。
//!
//! 各プラグインの名前・版・作者・説明と状態（読み込み済み・有効（次回の起動で読む）・無効・失敗）を表示し、
//! 有効・無効を切り替える。切り替えは `settings.yaml` の `plugins: enabled:` に書き、次回の起動から効く。

use std::path::Path;

use iced::widget::{button, column, container, mouse_area, row, scrollable, space, text};
use iced::{Alignment, Element, Fill, Length};

use crate::menu::{
    CommandInvocation, CommandItem, CommandRegistry, ItemStyle, MenuItem, MenuRegistry, location,
};
use crate::plugin_menu;
use crate::plugins;
use crate::ui_style::{self, UiPalette};

/// 一覧の 1 行（1 つのプラグイン）
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
    /// 設定で有効にしてあるか
    pub enabled: bool,
    pub state: State,
}

/// 一覧に出す状態
#[derive(Debug, Clone, PartialEq)]
pub enum State {
    /// 読み込み済み（起動のときに読めた）
    Loaded,
    /// 有効だが、この起動では読んでいない（次回の起動で読む）
    Enabled,
    /// 無効（読まない）
    Disabled,
    /// 失敗した（理由つき）。読めなかったフォルダーもこれ。
    Failed(String),
}

/// 状態を決める。実行中の失敗が最優先、次に読めなかったフォルダー、そのあと有効かどうかと起動の結果。
pub fn state_of(
    enabled: bool,
    invalid: Option<&str>,
    startup: Option<&Result<(), String>>,
    runtime_failure: Option<&str>,
) -> State {
    if let Some(reason) = runtime_failure {
        return State::Failed(reason.to_string());
    }
    if let Some(reason) = invalid {
        return State::Failed(reason.to_string());
    }
    if !enabled {
        return State::Disabled;
    }
    match startup {
        Some(Ok(())) => State::Loaded,
        Some(Err(reason)) => State::Failed(reason.clone()),
        None => State::Enabled,
    }
}

/// 一覧を作る。`dir` のフォルダーと、設定に書かれた名前（フォルダーがないもの）を、名前の順に並べる。
pub fn rows(dir: Option<&Path>, configured: &[String]) -> Vec<Row> {
    let mut found: Vec<(String, Result<plugins::Manifest, String>)> = Vec::new();
    if let Some(dir) = dir
        && let Ok(entries) = std::fs::read_dir(dir)
    {
        for entry in entries.flatten() {
            if !entry.path().is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let manifest = match std::fs::read_to_string(entry.path().join("plugin.yaml")) {
                Ok(text) => plugins::parse_manifest(&text, &name),
                Err(e) => Err(format!("cannot read plugin.yaml ({e})")),
            };
            found.push((name, manifest));
        }
    }
    for name in configured {
        if !found.iter().any(|(n, _)| n == name) {
            found.push((name.clone(), Err("no plugin folder with this name".into())));
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
        .into_iter()
        .map(|(name, manifest)| {
            let enabled = configured.contains(&name);
            let (version, author, description, invalid) = match manifest {
                Ok(m) => (m.version, m.author, m.description, None),
                Err(reason) => (String::new(), String::new(), String::new(), Some(reason)),
            };
            let startup = plugins::startup_result(&name);
            let runtime = plugin_menu::failure_reason(&name);
            let state = state_of(
                enabled,
                invalid.as_deref(),
                startup.as_ref(),
                runtime.as_deref(),
            );
            Row {
                name,
                version,
                author,
                description,
                enabled,
                state,
            }
        })
        .collect()
}

/// 切り替えたあとの、有効なプラグインの名前（一覧の順）
pub fn toggled_names(rows: &[Row], name: &str) -> Vec<String> {
    rows.iter()
        .filter(|r| r.enabled != (r.name == name))
        .map(|r| r.name.clone())
        .collect()
}

/// View メニューに「Plugins...」を足す（ネイティブ版だけ）
pub fn register(commands: &mut CommandRegistry, menus: &mut MenuRegistry) {
    commands.register_plain("view.plugins", || crate::app::Message::PluginsDialogOpen);
    menus.contribute_item(
        location::VIEW,
        400,
        MenuItem::Command(CommandItem::new(
            "Plugins...",
            CommandInvocation::plain("view.plugins"),
            ItemStyle::DROPDOWN,
        )),
    );
}

/// プラグインの一覧の画面（モーダル）。`on_toggle` は名前を受けて、その項目を切り替える。
pub fn view_plugins_modal<'a, Message: 'a + Clone>(
    rows: &[Row],
    dir: Option<&Path>,
    notice: Option<&str>,
    on_close: Message,
    on_toggle: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    let palette = ui_style::palette();
    let title = text("Plugins").size(20);
    let folder = dir
        .map(|d| d.display().to_string())
        .unwrap_or_else(|| "(no configuration folder)".into());
    let folder_line = text(format!("Folder: {folder}")).size(11);

    let mut list = column![].spacing(10);
    if rows.is_empty() {
        list = list.push(
            text("No plugins found. Put a plugin folder in the folder above and restart.").size(12),
        );
    }
    for r in rows {
        list = list.push(plugin_row(r, &palette, on_toggle(r.name.clone())));
    }

    let notice_line = notice.map(|n| text(n.to_string()).size(12));
    let close_btn = button(text("Close").size(13))
        .on_press(on_close)
        .padding([4, 16])
        .style(move |_theme, status| ui_style::accent_button_style(&palette, status));
    let footer_note = text("Changes take effect after restart.").size(11);

    let mut content = column![
        title,
        folder_line,
        space().height(8),
        scrollable(list).height(Length::Fixed(320.0)),
        space().height(8),
    ]
    .spacing(4)
    .align_x(Alignment::Start);
    if let Some(line) = notice_line {
        content = content.push(line);
    }
    content = content
        .push(footer_note)
        .push(container(close_btn).width(Fill).align_x(Alignment::End));

    let modal_card = container(content)
        .width(560)
        .padding(20)
        .style(move |_theme| ui_style::menu_card_style(&palette));
    let backdrop = mouse_area(
        container(space().width(Fill).height(Fill))
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

fn plugin_row<'a, Message: 'a + Clone>(
    r: &Row,
    palette: &UiPalette,
    toggle: Message,
) -> Element<'a, Message> {
    let p = *palette;
    let (state_label, state_color) = match &r.state {
        State::Loaded => ("Loaded".to_string(), p.text_secondary),
        State::Enabled => ("Enabled (restart to load)".to_string(), p.text_secondary),
        State::Disabled => ("Disabled".to_string(), p.text_disabled),
        State::Failed(reason) => (format!("Failed: {reason}"), p.text_muted),
    };
    let meta = if r.version.is_empty() {
        String::new()
    } else if r.author.is_empty() {
        format!("version {}", r.version)
    } else {
        format!("version {} · {}", r.version, r.author)
    };
    let toggle_label = if r.enabled { "Disable" } else { "Enable" };
    let toggle_btn = button(text(toggle_label).size(12))
        .on_press(toggle)
        .padding([3, 10])
        .style(move |_theme, status| ui_style::accent_button_style(&p, status));
    let head = row![
        text(r.name.clone()).size(14).color(p.text),
        text(meta).size(11).color(p.text_muted),
        space().width(Fill),
        toggle_btn,
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    let mut body = column![head, text(state_label).size(12).color(state_color)].spacing(3);
    if !r.description.is_empty() {
        body = body.push(text(r.description.clone()).size(12).color(p.text_secondary));
    }
    body.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, enabled: bool) -> Row {
        Row {
            name: name.into(),
            version: "0.1.0".into(),
            author: String::new(),
            description: String::new(),
            enabled,
            state: State::Disabled,
        }
    }

    #[test]
    fn state_follows_the_failure_then_the_folder_then_the_startup() {
        assert_eq!(state_of(false, None, None, None), State::Disabled);
        assert_eq!(state_of(true, None, None, None), State::Enabled);
        assert_eq!(state_of(true, None, Some(&Ok(())), None), State::Loaded);
        assert_eq!(
            state_of(true, None, Some(&Err("boom".into())), None),
            State::Failed("boom".into())
        );
        assert_eq!(
            state_of(false, Some("bad yaml"), None, None),
            State::Failed("bad yaml".into())
        );
        assert_eq!(
            state_of(true, None, Some(&Ok(())), Some("command failed")),
            State::Failed("command failed".into())
        );
    }

    #[test]
    fn toggling_one_name_keeps_the_others_in_order() {
        let rows = vec![row("a", true), row("b", false), row("c", true)];
        assert_eq!(toggled_names(&rows, "b"), vec!["a", "b", "c"]);
        assert_eq!(toggled_names(&rows, "a"), vec!["c"]);
    }

    #[test]
    fn lists_folders_and_names_that_have_no_folder() {
        let dir = std::env::temp_dir().join(format!(
            "yui-plugin-list-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let good = dir.join("good");
        std::fs::create_dir_all(&good).unwrap();
        std::fs::write(
            good.join("plugin.yaml"),
            "name: good\nversion: 1.2.0\napi_version: 1\nauthor: someone\n",
        )
        .unwrap();
        let bad = dir.join("bad");
        std::fs::create_dir_all(&bad).unwrap();
        std::fs::write(
            bad.join("plugin.yaml"),
            "name: other\nversion: 1\napi_version: 1\n",
        )
        .unwrap();

        let configured = vec!["good".to_string(), "missing".to_string()];
        let list = rows(Some(&dir), &configured);
        let names: Vec<&str> = list.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["bad", "good", "missing"]);

        let good_row = list.iter().find(|r| r.name == "good").unwrap();
        assert_eq!(good_row.version, "1.2.0");
        assert_eq!(good_row.author, "someone");
        assert!(good_row.enabled);
        assert_ne!(good_row.state, State::Disabled);

        let bad_row = list.iter().find(|r| r.name == "bad").unwrap();
        assert!(matches!(&bad_row.state, State::Failed(r) if r.contains("does not match")));
        assert!(!bad_row.enabled);

        let missing = list.iter().find(|r| r.name == "missing").unwrap();
        assert!(matches!(&missing.state, State::Failed(r) if r.contains("no plugin folder")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 画面を開かずに、一覧の画面（モーダル）の部品を試す（`iced_test` で、クリックを再現する）。
    mod screen {
        use super::*;

        #[derive(Debug, Clone, PartialEq)]
        enum Msg {
            Close,
            Toggle(String),
        }

        const NOTICE: &str = "Saved. Restart YUI to apply the change.";

        fn sample_rows() -> Vec<Row> {
            vec![
                Row {
                    name: "broken".into(),
                    version: String::new(),
                    author: String::new(),
                    description: String::new(),
                    enabled: false,
                    state: State::Failed(
                        "plugin.yaml name 'other' does not match its folder 'broken'".into(),
                    ),
                },
                Row {
                    name: "hello-yui".into(),
                    version: "0.1.0".into(),
                    author: "YUI project".into(),
                    description: "Adds View > Say hello.".into(),
                    enabled: true,
                    state: State::Loaded,
                },
                Row {
                    name: "later".into(),
                    version: "1.0.0".into(),
                    author: String::new(),
                    description: String::new(),
                    enabled: false,
                    state: State::Disabled,
                },
            ]
        }

        fn modal(rows: &[Row]) -> iced::Element<'_, Msg> {
            view_plugins_modal(rows, None, Some(NOTICE), Msg::Close, Msg::Toggle)
        }

        #[test]
        fn the_list_shows_each_plugin_its_state_and_the_notice() {
            let rows = sample_rows();
            let _gpu = crate::test_gpu::lock();
            let mut ui = iced_test::simulator(modal(&rows));
            for text in [
                "hello-yui",
                "version 0.1.0 · YUI project",
                "Adds View > Say hello.",
                "Loaded",
                "Disabled",
                "Failed: plugin.yaml name 'other' does not match its folder 'broken'",
                "Enable",
                "Disable",
                NOTICE,
                "Changes take effect after restart.",
                "Close",
            ] {
                assert!(ui.find(text).is_ok(), "missing on screen: {text}");
            }
        }

        #[test]
        fn pressing_disable_sends_that_plugins_name() {
            let rows = sample_rows();
            let _gpu = crate::test_gpu::lock();
            let mut ui = iced_test::simulator(modal(&rows));
            ui.click("Disable")
                .expect("the Disable button is on screen");
            let messages: Vec<Msg> = ui.into_messages().collect();
            assert_eq!(messages, vec![Msg::Toggle("hello-yui".into())]);
        }

        #[test]
        fn pressing_close_sends_close() {
            let rows = sample_rows();
            let _gpu = crate::test_gpu::lock();
            let mut ui = iced_test::simulator(modal(&rows));
            ui.click("Close").expect("the Close button is on screen");
            let messages: Vec<Msg> = ui.into_messages().collect();
            assert_eq!(messages, vec![Msg::Close]);
        }

        /// 画像を書く（確認用）。`YUI_UI_SHOTS_DIR` が無いときは何もしない。
        /// 比べはせず、同じ名前の画像は書き換える。
        #[test]
        fn writes_a_picture_of_the_list_for_a_visual_check() {
            let Ok(dir) = std::env::var("YUI_UI_SHOTS_DIR") else {
                return;
            };
            let rows = sample_rows();
            let _gpu = crate::test_gpu::lock();
            let mut ui = iced_test::simulator(modal(&rows));
            let snapshot = ui.snapshot(&iced::Theme::Dark).expect("take a snapshot");
            let base = std::path::Path::new(&dir).join("plugin-list");
            let _ = std::fs::remove_file(base.with_extension("png"));
            std::fs::create_dir_all(&dir).expect("create the picture folder");
            snapshot.matches_image(&base).expect("write the picture");
        }
    }
}
