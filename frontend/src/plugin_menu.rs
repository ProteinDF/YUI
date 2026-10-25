//! プラグインのメニューとコマンド（段階 6k-iii、architecture.md §6.4 と §8.7）。
//!
//! 起動時に読めたプラグインが `app.add_command` / `app.add_menu_item` で登録したものを、
//! アプリのコマンドとメニューの受け口に入れる。押されたコマンドは Python の専用スレッドで動かし、
//! UI は待たない。コマンドが失敗したプラグインは、以後その項目を出さず、押せなくする（アプリは止めない）。

use std::collections::BTreeMap;
use std::sync::Mutex;

use iced::Task;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::app::Message;
use crate::menu::{
    CommandInvocation, CommandItem, CommandRegistry, ItemStyle, MenuContext, MenuItem,
    MenuRegistry, MenuTarget, location,
};

/// 読み込めたプラグインが登録したもの（起動のとき、アプリを作る前に入る）
#[derive(Debug, Clone, PartialEq)]
pub struct Contribution {
    pub plugin: String,
    /// プラグインが `add_command` で登録したコマンドの識別子
    pub commands: Vec<String>,
    pub items: Vec<ItemSpec>,
}

/// `add_menu_item` で登録した項目
#[derive(Debug, Clone, PartialEq)]
pub struct ItemSpec {
    pub location: String,
    pub order: i32,
    pub label: String,
    pub command: String,
    pub args: Value,
}

/// Python 側が `register` のあとに返す内容（JSON）
#[cfg_attr(not(feature = "python"), allow(dead_code))] // Python が無いビルドでは使わない
#[derive(Deserialize)]
struct Export {
    commands: Vec<String>,
    items: Vec<ExportItem>,
}

#[cfg_attr(not(feature = "python"), allow(dead_code))] // Python が無いビルドでは使わない
#[derive(Deserialize)]
struct ExportItem {
    location: String,
    order: i32,
    label: String,
    command: String,
    #[serde(default)]
    args: Value,
}

/// Python が返した内容を検査する。識別子は `plugin.<name>.` で始まり、場所は §8.7 の一覧にあり、
/// 項目のコマンドはそのプラグインが登録したものでなければならない。違えば、そのプラグインは失敗になる。
#[cfg_attr(not(feature = "python"), allow(dead_code))] // Python が無いビルドでは使わない
pub fn contribution_from_export(plugin: &str, text: &str) -> Result<Contribution, String> {
    let export: Export =
        serde_json::from_str(text).map_err(|e| format!("cannot read the menu items ({e})"))?;
    let prefix = format!("plugin.{plugin}.");
    for id in &export.commands {
        if !id.starts_with(&prefix) || id.len() == prefix.len() {
            return Err(format!("command id '{id}' must start with '{prefix}'"));
        }
    }
    let mut items = Vec::new();
    for item in export.items {
        if !location::ALL.contains(&item.location.as_str()) {
            return Err(format!("unknown menu location '{}'", item.location));
        }
        if !export.commands.contains(&item.command) {
            return Err(format!(
                "menu item '{}' uses command '{}', which the plugin did not add",
                item.label, item.command
            ));
        }
        items.push(ItemSpec {
            location: item.location,
            order: item.order,
            label: item.label,
            command: item.command,
            args: item.args,
        });
    }
    Ok(Contribution {
        plugin: plugin.to_string(),
        commands: export.commands,
        items,
    })
}

/// 読み込めたプラグインの登録（起動の時に入り、アプリを作るときに受け口へ入る）
static LOADED: Mutex<Vec<Contribution>> = Mutex::new(Vec::new());
/// 失敗して、外されたプラグインと、その理由
static FAILED: Mutex<BTreeMap<String, String>> = Mutex::new(BTreeMap::new());

pub fn record_loaded(contribution: Contribution) {
    if let Ok(mut loaded) = LOADED.lock() {
        loaded.push(contribution);
    }
}

pub fn is_failed(plugin: &str) -> bool {
    FAILED
        .lock()
        .is_ok_and(|failed| failed.contains_key(plugin))
}

/// 実行中に失敗して外されたプラグインの理由（一覧の表示に使う）
pub fn failure_reason(plugin: &str) -> Option<String> {
    FAILED.lock().ok()?.get(plugin).cloned()
}

/// プラグインを失敗にして外す。理由は `plugin.log` に書く（書けなくても外すのは変えない）。
pub fn mark_failed(plugin: &str, reason: &str) {
    if let Ok(mut failed) = FAILED.lock() {
        failed.insert(plugin.to_string(), reason.to_string());
    }
    if let Some(dir) = crate::plugins::plugins_dir() {
        crate::plugins::append_log(
            &dir.join(crate::plugins::LOG_FILE),
            &format!("{plugin}: disabled after a failure: {reason}"),
        );
    }
}

/// 読み込めたプラグインを、コマンドとメニューの受け口に入れる（アプリを作るときに呼ぶ）
pub fn register_loaded(commands: &mut CommandRegistry, menus: &mut MenuRegistry) {
    let loaded = LOADED.lock().map(|l| l.clone()).unwrap_or_default();
    register_contributions(&loaded, commands, menus);
}

pub fn register_contributions(
    contributions: &[Contribution],
    commands: &mut CommandRegistry,
    menus: &mut MenuRegistry,
) {
    for contribution in contributions {
        for id in &contribution.commands {
            let plugin = contribution.plugin.clone();
            let command_id = id.clone();
            commands.register(id, move |args| {
                if is_failed(&plugin) {
                    return Err(format!("plugin {plugin} failed and is disabled"));
                }
                Ok(Message::PluginCommand {
                    plugin: plugin.clone(),
                    id: command_id.clone(),
                    payload: args.clone(),
                })
            });
        }
        for item in &contribution.items {
            let plugin = contribution.plugin.clone();
            let spec = item.clone();
            // 右クリックの場所では、ボタンの大きさを既存の操作に合わせる
            let style = if spec.location.starts_with("context/") {
                ItemStyle::ACTION_12
            } else {
                ItemStyle::DROPDOWN
            };
            menus.contribute(
                &spec.location.clone(),
                spec.order,
                move |ctx: &MenuContext| {
                    if is_failed(&plugin) {
                        return Vec::new();
                    }
                    let payload = json!({
                        "target": target_json(ctx.target.as_ref()),
                        "args": spec.args,
                    });
                    vec![MenuItem::Command(CommandItem::new(
                        spec.label.clone(),
                        CommandInvocation::new(&spec.command, payload),
                        style,
                    ))]
                },
            );
        }
    }
}

/// 右クリックの対象を、プラグインの `ctx.target` にする形（対象だけ。状態は入れない）
fn target_json(target: Option<&MenuTarget>) -> Value {
    match target {
        None => Value::Null,
        Some(MenuTarget::Atom(t)) => json!({"kind": "atom", "atom_id": t.atom_id}),
        Some(MenuTarget::TreeNode(t)) => json!({"kind": "tree_node", "key": t.key}),
        Some(MenuTarget::Representation(t)) => json!({"kind": "representation", "index": t.index}),
        Some(MenuTarget::File(t)) => json!({"kind": "file", "path": t.path}),
    }
}

/// Python の戻り値を、コマンドの結果にする（空なら成功、文字列なら Python の例外の内容）
pub fn command_outcome(reply: Result<String, String>) -> Result<(), String> {
    match reply {
        Ok(text) if text.is_empty() => Ok(()),
        Ok(text) => Err(text),
        Err(reason) => Err(reason),
    }
}

/// コマンドを Python の専用スレッドで動かす。待つのは別のスレッドで、UI は止めない。
pub fn run(plugin: String, id: String, payload: Value) -> Task<Message> {
    let reply = match crate::plugins::send_command(&id, &payload) {
        Ok(reply) => reply,
        Err(reason) => {
            return Task::done(Message::PluginCommandDone {
                plugin,
                result: Err(reason),
            });
        }
    };
    let (tx, done) = iced::futures::channel::oneshot::channel();
    std::thread::spawn(move || {
        let outcome = reply
            .recv()
            .map_err(|_| "the Python thread stopped".to_string())
            .and_then(command_outcome);
        let _ = tx.send(outcome);
    });
    Task::perform(
        async move {
            done.await
                .unwrap_or_else(|_| Err("the command was lost".into()))
        },
        move |result| Message::PluginCommandDone { plugin, result },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"{
        "commands": ["plugin.sample.hello"],
        "items": [{"location": "menu/view", "order": 150, "label": "Hello",
                   "command": "plugin.sample.hello", "args": {"n": 1}}]
    }"#;

    #[test]
    fn accepts_commands_and_items_that_fit_the_rules() {
        let c = contribution_from_export("sample", GOOD).expect("valid");
        assert_eq!(c.commands, vec!["plugin.sample.hello".to_string()]);
        assert_eq!(c.items.len(), 1);
        assert_eq!(c.items[0].order, 150);
        assert_eq!(c.items[0].args["n"], 1);
    }

    #[test]
    fn refuses_a_command_outside_the_plugin_namespace() {
        let text = GOOD.replace("plugin.sample.hello", "file.new");
        let err = contribution_from_export("sample", &text).unwrap_err();
        assert!(err.contains("must start with 'plugin.sample.'"), "{err}");

        let bare = GOOD.replace("plugin.sample.hello", "plugin.sample.");
        assert!(contribution_from_export("sample", &bare).is_err());
    }

    #[test]
    fn refuses_an_unknown_location_and_an_item_without_its_command() {
        let location = GOOD.replace("menu/view", "menu/edit");
        let err = contribution_from_export("sample", &location).unwrap_err();
        assert!(err.contains("unknown menu location 'menu/edit'"), "{err}");

        let command = GOOD.replace(
            "\"command\": \"plugin.sample.hello\"",
            "\"command\": \"plugin.sample.other\"",
        );
        let err = contribution_from_export("sample", &command).unwrap_err();
        assert!(err.contains("did not add"), "{err}");
    }

    #[test]
    fn a_registered_command_is_passed_to_the_app_with_its_target() {
        let c = contribution_from_export("sample", GOOD).unwrap();
        let mut commands = CommandRegistry::new();
        let mut menus = MenuRegistry::new();
        register_contributions(std::slice::from_ref(&c), &mut commands, &mut menus);

        let ctx = MenuContext {
            target: Some(MenuTarget::Representation(
                crate::representation_inspector::RepContextTarget {
                    index: 2,
                    id: "r".into(),
                    kind: yui_core::representation::RepresentationKind::BallAndStick,
                    visible: true,
                },
            )),
            ..MenuContext::default()
        };
        let items = menus.build("menu/view", &ctx);
        let MenuItem::Command(item) = &items[0] else {
            panic!("expected a command item: {items:?}");
        };
        assert_eq!(item.label, "Hello");
        assert_eq!(item.command.args["target"]["kind"], "representation");
        assert_eq!(item.command.args["target"]["index"], 2);
        assert_eq!(item.command.args["args"]["n"], 1);

        let message = commands.resolve(&item.command).expect("registered");
        assert!(
            matches!(message, Message::PluginCommand { ref id, .. } if id == "plugin.sample.hello")
        );
    }

    #[test]
    fn a_failed_plugin_loses_its_items_and_commands() {
        let c = contribution_from_export("gone", &GOOD.replace("sample", "gone")).unwrap();
        let mut commands = CommandRegistry::new();
        let mut menus = MenuRegistry::new();
        register_contributions(std::slice::from_ref(&c), &mut commands, &mut menus);
        assert_eq!(menus.build("menu/view", &MenuContext::default()).len(), 1);

        mark_failed("gone", "RuntimeError: boom");
        assert!(menus.build("menu/view", &MenuContext::default()).is_empty());
        let invocation = CommandInvocation::new("plugin.gone.hello", json!({}));
        let err = commands.resolve(&invocation).unwrap_err();
        assert!(err.to_string().contains("failed and is disabled"), "{err}");
    }

    #[test]
    fn a_python_exception_is_a_failed_command() {
        assert_eq!(command_outcome(Ok(String::new())), Ok(()));
        assert_eq!(
            command_outcome(Ok("ValueError: bad".into())),
            Err("ValueError: bad".to_string())
        );
        assert_eq!(
            command_outcome(Err("stopped".into())),
            Err("stopped".to_string())
        );
    }
}
