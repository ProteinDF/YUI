//! プラグインのメニューとコマンド（段階 6k-iii、architecture.md §6.4 と §8.7）。
//!
//! 起動時に読めたプラグインが `register` で宣言したコマンドとメニュー項目（`add_command` / `add_menu_item`）を、
//! アプリのコマンドとメニューの受け口に入れる。押されたコマンドはプラグインのプロセスに頼み、
//! UI は待たない。コマンドが失敗したプラグインは、以後その項目を出さず、押せなくする（アプリは止めない）。

use std::collections::BTreeMap;
use std::sync::Mutex;

use iced::Task;
use serde_json::{Value, json};

use crate::app::Message;
use crate::menu::{
    CommandInvocation, CommandItem, CommandRegistry, ItemStyle, MenuContext, MenuItem,
    MenuRegistry, MenuTarget, location,
};
use crate::plugin_process::child::CommandOutcome;
use crate::plugin_process::{convert, protocol};

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

/// プラグインが `register` で宣言した内容を検査する。識別子は `plugin.<name>.` で始まり、場所は §8.7 の一覧にあり、
/// 項目のコマンドはそのプラグインが宣言したものでなければならない。違えば、そのプラグインは失敗になる。
pub fn contribution_from_register(
    plugin: &str,
    commands: Vec<String>,
    wire_items: Vec<protocol::MenuItem>,
) -> Result<Contribution, String> {
    let prefix = format!("plugin.{plugin}.");
    for id in &commands {
        if !id.starts_with(&prefix) || id.len() == prefix.len() {
            return Err(format!("command id '{id}' must start with '{prefix}'"));
        }
    }
    let mut items = Vec::new();
    for item in wire_items {
        if !location::ALL.contains(&item.location.as_str()) {
            return Err(format!("unknown menu location '{}'", item.location));
        }
        if !commands.contains(&item.command) {
            return Err(format!(
                "menu item '{}' uses command '{}', which the plugin did not add",
                item.label, item.command
            ));
        }
        let order = i32::try_from(item.order)
            .map_err(|_| format!("menu item '{}': the order is out of range", item.label))?;
        let args = convert::to_json(&item.args)
            .map_err(|e| format!("menu item '{}': args cannot be used ({e})", item.label))?;
        items.push(ItemSpec {
            location: item.location,
            order,
            label: item.label,
            command: item.command,
            args,
        });
    }
    Ok(Contribution {
        plugin: plugin.to_string(),
        commands,
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
    // 失敗したプラグインのプロセスは、残さずに止める
    crate::plugins::stop_plugin(plugin);
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

/// プラグインの返事を、コマンドの結果にする（成功なら `Ok`、失敗なら理由。返した値は、今は使わない）
pub fn command_outcome(reply: CommandOutcome) -> Result<(), String> {
    reply.map(|_| ())
}

/// コマンドをプラグインのプロセスに頼む。待つのは別のスレッドで、UI は止めない。
pub fn run(plugin: String, id: String, payload: Value) -> Task<Message> {
    let reply = match crate::plugins::invoke(&plugin, &id, &payload) {
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
            .map_err(|_| "the plugin process stopped".to_string())
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
    use rmpv::Value as Mp;

    /// `plugin.sample.hello` を宣言し、`menu/view` に項目を 1 つ出すプラグインの宣言
    fn good() -> (Vec<String>, Vec<protocol::MenuItem>) {
        (
            vec!["plugin.sample.hello".to_string()],
            vec![protocol::MenuItem {
                location: "menu/view".into(),
                order: 150,
                label: "Hello".into(),
                command: "plugin.sample.hello".into(),
                args: Mp::Map(vec![(Mp::from("n"), Mp::from(1))]),
            }],
        )
    }

    fn sample() -> Contribution {
        let (commands, items) = good();
        contribution_from_register("sample", commands, items).expect("valid")
    }

    #[test]
    fn accepts_commands_and_items_that_fit_the_rules() {
        let c = sample();
        assert_eq!(c.commands, vec!["plugin.sample.hello".to_string()]);
        assert_eq!(c.items.len(), 1);
        assert_eq!(c.items[0].order, 150);
        assert_eq!(c.items[0].args["n"], 1);
    }

    #[test]
    fn refuses_a_command_outside_the_plugin_namespace() {
        let (_, items) = good();
        let err = contribution_from_register("sample", vec!["file.new".into()], items.clone())
            .unwrap_err();
        assert!(err.contains("must start with 'plugin.sample.'"), "{err}");

        let bare = contribution_from_register("sample", vec!["plugin.sample.".into()], items);
        assert!(bare.is_err());
    }

    #[test]
    fn refuses_an_unknown_location_and_an_item_without_its_command() {
        let (commands, mut items) = good();
        items[0].location = "menu/edit".into();
        let err = contribution_from_register("sample", commands.clone(), items).unwrap_err();
        assert!(err.contains("unknown menu location 'menu/edit'"), "{err}");

        let (_, mut items) = good();
        items[0].command = "plugin.sample.other".into();
        let err = contribution_from_register("sample", commands, items).unwrap_err();
        assert!(err.contains("did not add"), "{err}");
    }

    #[test]
    fn refuses_an_order_or_args_that_cannot_be_used() {
        let (commands, mut items) = good();
        items[0].order = i64::from(i32::MAX) + 1;
        let err = contribution_from_register("sample", commands.clone(), items).unwrap_err();
        assert!(err.contains("order"), "{err}");

        let (_, mut items) = good();
        items[0].args = Mp::Binary(vec![1]);
        let err = contribution_from_register("sample", commands, items).unwrap_err();
        assert!(err.contains("args"), "{err}");
    }

    #[test]
    fn a_registered_command_is_passed_to_the_app_with_its_target() {
        let c = sample();
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
        let (commands_list, items) = good();
        let rename = |s: &str| s.replace("sample", "gone");
        let c = contribution_from_register(
            "gone",
            commands_list.iter().map(|s| rename(s)).collect(),
            items
                .into_iter()
                .map(|mut i| {
                    i.command = rename(&i.command);
                    i
                })
                .collect(),
        )
        .unwrap();
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
    fn a_failed_reply_is_a_failed_command() {
        assert_eq!(command_outcome(Ok(None)), Ok(()));
        assert_eq!(command_outcome(Ok(Some(Mp::from(1)))), Ok(()));
        assert_eq!(
            command_outcome(Err("ValueError: bad".into())),
            Err("ValueError: bad".to_string())
        );
    }
}
