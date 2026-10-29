//! プラグインの読み込みと実行（段階 6k、architecture.md §6.4 と §8.6）。
//!
//! `<config>/yui/plugins/<name>/` の `plugin.yaml` を検査し、`settings.yaml` の `plugins: enabled:` に
//! 書かれたものだけを、子プロセスとして起動する（`plugin_process`）。`command` のないプラグインは、
//! Python のパッケージとして、同梱のランナーで動かす。失敗は、そのプラグインだけを外して警告にする
//! （アプリは止まらない）。読み込みと失敗は `plugin.log` に書く（Log Viewer で開ける）。

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::plugin_process::child::{ChildPlugin, CommandOutcome, EventError};
use crate::plugin_process::protocol::Message;
use crate::plugin_process::{convert, python};

/// このアプリが対応するプラグイン API の版（架空の版ではなく、`plugin.yaml` の `api_version` と比べる）
pub const API_VERSION: u64 = 1;
/// プラグインのログ（読み込みの結果、`app.log`、標準エラー、失敗）
pub const LOG_FILE: &str = "plugin.log";
/// 起動から `register` が届くまでに待つ時間。過ぎたら、そのプラグインを失敗にする。
const LOAD_WAIT: Duration = Duration::from_secs(5);
/// プラグインを止めるとき、標準入力を閉じてから、終わるのを待つ時間
const STOP_GRACE: Duration = Duration::from_millis(500);

/// 動いているプラグインの子プロセス（名前 → 子プロセス）
static RUNNING: Mutex<BTreeMap<String, Arc<ChildPlugin>>> = Mutex::new(BTreeMap::new());

/// プラグインの動かし方（architecture.md §6.4）。`plugin.yaml` の `mode`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PluginMode {
    /// 長く動く 1 つのプロセス。`register` でコマンドとメニュー項目を宣言する
    #[default]
    Resident,
    /// コマンドを呼ぶたびに、入出力のファイルを渡してプロセスを起動する
    Oneshot,
}

/// `plugin.yaml` の内容
#[derive(Debug, Clone, PartialEq)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    pub api_version: u64,
    // 表示は段階 6k-iv（プラグインの一覧）で使う
    pub description: String,
    pub author: String,
    /// 動かし方（省くと `resident`）
    pub mode: PluginMode,
    /// 起動するコマンド（プログラムと引数。シェルは通さない）。省くと空（Python のパッケージとして、ランナーで動かす）
    pub command: Vec<String>,
}

/// プラグインを置くフォルダー（`<config>/yui/plugins`）
pub fn plugins_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("yui").join("plugins"))
}

/// 名前は、フォルダーの名前であり、Python のモジュール名（`-` は `_` に置き換える）にもなる。
/// 英字で始まり、英数字・`_`・`-` だけ。64 字まで。
pub fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic())
        && name.len() <= 64
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// `plugin.yaml` を検査する。`name` はフォルダーの名前と同じでなければならず、
/// `api_version` はこのアプリの版と同じでなければならない。
pub fn parse_manifest(text: &str, folder: &str) -> Result<Manifest, String> {
    let value: serde_yaml::Value =
        serde_yaml::from_str(text).map_err(|e| format!("invalid plugin.yaml ({e})"))?;
    let map = value
        .as_mapping()
        .ok_or_else(|| "plugin.yaml must be a mapping".to_string())?;
    let string = |key: &str| match map.get(key) {
        Some(serde_yaml::Value::String(s)) => Some(s.clone()),
        _ => None,
    };

    let name = string("name").ok_or_else(|| "plugin.yaml needs a 'name'".to_string())?;
    if name != folder {
        return Err(format!(
            "plugin.yaml name '{name}' does not match its folder '{folder}'"
        ));
    }
    let version = string("version").ok_or_else(|| "plugin.yaml needs a 'version'".to_string())?;
    let api_version = map
        .get("api_version")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| "plugin.yaml needs an integer 'api_version'".to_string())?;
    if api_version != API_VERSION {
        return Err(format!(
            "api_version {api_version} is not supported (this app supports {API_VERSION})"
        ));
    }
    let mode = match map.get("mode") {
        None => PluginMode::Resident,
        Some(serde_yaml::Value::String(s)) if s == "resident" => PluginMode::Resident,
        Some(serde_yaml::Value::String(s)) if s == "oneshot" => PluginMode::Oneshot,
        Some(_) => return Err("plugin.yaml 'mode' must be 'resident' or 'oneshot'".into()),
    };
    let command_error = || "plugin.yaml 'command' must be a non-empty list of strings".to_string();
    let command = match map.get("command") {
        None => Vec::new(),
        Some(serde_yaml::Value::Sequence(items)) => {
            let list: Option<Vec<String>> = items
                .iter()
                .map(|item| item.as_str().map(str::to_string))
                .collect();
            match list {
                Some(list) if !list.is_empty() => list,
                _ => return Err(command_error()),
            }
        }
        Some(_) => return Err(command_error()),
    };
    Ok(Manifest {
        name,
        version,
        api_version,
        description: string("description").unwrap_or_default(),
        author: string("author").unwrap_or_default(),
        mode,
        command,
    })
}

/// 設定に書かれている、有効にしたプラグインの名前（一覧の表示と切り替えに使う）
static CONFIGURED: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// 起動のときの読み込みの結果（名前 → 成功なら `Ok`、失敗なら理由）
static STARTUP: Mutex<BTreeMap<String, Result<(), String>>> = Mutex::new(BTreeMap::new());

/// 設定に書かれている、有効にしたプラグインの名前
pub fn configured() -> Vec<String> {
    CONFIGURED.lock().map(|c| c.clone()).unwrap_or_default()
}

/// 設定を書き換えたあとの、有効にしたプラグインの名前
pub fn set_configured(names: &[String]) {
    if let Ok(mut configured) = CONFIGURED.lock() {
        *configured = names.to_vec();
    }
}

/// 起動のときの読み込みの結果。読み込みを試していなければ `None`。
pub fn startup_result(name: &str) -> Option<Result<(), String>> {
    STARTUP.lock().ok()?.get(name).cloned()
}

fn record_startup(name: &str, result: Result<(), String>) {
    if let Ok(mut startup) = STARTUP.lock() {
        startup.insert(name.to_string(), result);
    }
}

/// 起動時に呼ぶ。読み込めなかったものの理由（警告）を返す。`python` は、設定の `plugins.python`。
pub fn start(enabled: &[String], python: Option<&str>) -> Vec<String> {
    set_configured(enabled);
    if enabled.is_empty() {
        return Vec::new();
    }
    match plugins_dir() {
        Some(dir) => load_all(&dir, enabled, python),
        None => vec!["plugins: no configuration folder; no plugin is loaded".into()],
    }
}

/// `enabled` に並んだプラグインを、`dir` から読む。結果は `plugin.log` にも書く。
pub fn load_all(dir: &Path, enabled: &[String], python: Option<&str>) -> Vec<String> {
    let log = dir.join(LOG_FILE);
    let mut warnings = Vec::new();
    let mut seen = HashSet::new();
    for name in enabled {
        if !seen.insert(name.as_str()) {
            continue;
        }
        match load_one(dir, name, python, &log) {
            Ok((version, contribution)) => {
                append_log(&log, &format!("{name} {version}: loaded"));
                crate::plugin_menu::record_loaded(contribution);
                record_startup(name, Ok(()));
            }
            Err(reason) => {
                append_log(&log, &format!("{name}: failed: {reason}"));
                warnings.push(format!("plugin {name}: {reason}"));
                record_startup(name, Err(reason));
            }
        }
    }
    warnings
}

fn load_one(
    dir: &Path,
    name: &str,
    python: Option<&str>,
    log: &Path,
) -> Result<(String, crate::plugin_menu::Contribution), String> {
    if !valid_name(name) {
        return Err("invalid plugin name".into());
    }
    let folder = dir.join(name);
    let text = fs::read_to_string(folder.join("plugin.yaml"))
        .map_err(|e| format!("cannot read plugin.yaml ({e})"))?;
    let manifest = parse_manifest(&text, name)?;
    if manifest.mode == PluginMode::Oneshot {
        return Err("mode 'oneshot' is not supported yet".into());
    }

    // `command` があればそのまま実行する。なければ Python のパッケージとして、ランナーで動かす。
    let uses_runner = manifest.command.is_empty();
    let argv = if uses_runner {
        if !folder.join("__init__.py").is_file() {
            return Err("the plugin has no 'command' and no __init__.py".into());
        }
        let python = python.unwrap_or(python::default_python());
        python::command_for(python, &folder, &manifest.name, &manifest.version)
    } else {
        manifest.command.clone()
    };
    let plugin = ChildPlugin::spawn(&manifest.name, &argv, &folder, log).map_err(|e| {
        if uses_runner {
            format!("{e}; set 'plugins.python' in settings.yaml to the Python to use")
        } else {
            e
        }
    })?;
    // 登録に失敗したら、`plugin` はここで捨てられ、プロセスは止まる
    let contribution = wait_for_registration(&plugin, &manifest)?;
    if let Ok(mut running) = RUNNING.lock() {
        running.insert(manifest.name.clone(), Arc::new(plugin));
    }
    Ok((manifest.version, contribution))
}

/// `hello` と `register` を待ち、検査する。プロセスが先に終わったときは、標準エラーの最後の行を理由にする。
fn wait_for_registration(
    plugin: &ChildPlugin,
    manifest: &Manifest,
) -> Result<crate::plugin_menu::Contribution, String> {
    let deadline = Instant::now() + LOAD_WAIT;
    let mut said_hello = false;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match plugin.recv_event(left) {
            Ok(Message::Hello {
                name, api_version, ..
            }) => {
                if name != manifest.name {
                    return Err(format!(
                        "the plugin says its name is '{name}', not '{}'",
                        manifest.name
                    ));
                }
                if api_version != API_VERSION {
                    return Err(format!(
                        "api_version {api_version} is not supported (this app supports {API_VERSION})"
                    ));
                }
                said_hello = true;
            }
            Ok(Message::Register {
                commands,
                menu_items,
            }) => {
                if !said_hello {
                    return Err("the plugin registered before it said hello".into());
                }
                return crate::plugin_menu::contribution_from_register(
                    &manifest.name,
                    commands,
                    menu_items,
                );
            }
            Ok(_) => {}
            Err(EventError::Timeout) => {
                return Err(format!(
                    "did not register within {} seconds",
                    LOAD_WAIT.as_secs()
                ));
            }
            Err(EventError::Closed) => return Err(exit_reason(plugin)),
        }
    }
}

/// 登録の前にプロセスが終わったときの理由。ランナーは、失敗を `register failed: ...` の行で知らせる。
fn exit_reason(plugin: &ChildPlugin) -> String {
    let tail = plugin.stderr_tail(Duration::from_millis(500));
    match tail.last() {
        Some(line) => match line.strip_prefix("register failed: ") {
            Some(reason) => reason.to_string(),
            None => format!("the plugin process ended before it registered ({line})"),
        },
        None => "the plugin process ended before it registered".into(),
    }
}

/// プラグインのコマンドを、その子プロセスに頼む。返るのは結果の受け口（待つのは呼び出し側）。
/// `payload` は `{"target": ..., "args": ...}`。
pub fn invoke(
    plugin: &str,
    id: &str,
    payload: &serde_json::Value,
) -> Result<Receiver<CommandOutcome>, String> {
    let child = RUNNING
        .lock()
        .map_err(|_| "the plugin list is unusable".to_string())?
        .get(plugin)
        .cloned()
        .ok_or_else(|| format!("plugin {plugin} is not running"))?;
    let null = serde_json::Value::Null;
    let target = convert::from_json(payload.get("target").unwrap_or(&null));
    let args = convert::from_json(payload.get("args").unwrap_or(&null));
    Ok(child.invoke(id, target, args))
}

/// 動いているプラグインを止める（失敗して外すとき）。止まるのを待たない。
pub fn stop_plugin(name: &str) {
    let plugin = RUNNING
        .lock()
        .ok()
        .and_then(|mut running| running.remove(name));
    if let Some(plugin) = plugin {
        std::thread::spawn(move || plugin.shutdown(STOP_GRACE));
    }
}

/// アプリを終えるとき、すべてのプラグインを止める（終わるのを待つ）。
pub fn stop_all() {
    let plugins: Vec<Arc<ChildPlugin>> = RUNNING
        .lock()
        .map(|mut running| std::mem::take(&mut *running).into_values().collect())
        .unwrap_or_default();
    let handles: Vec<_> = plugins
        .into_iter()
        .map(|plugin| std::thread::spawn(move || plugin.shutdown(STOP_GRACE)))
        .collect();
    for handle in handles {
        let _ = handle.join();
    }
}

/// ログに 1 行足す。書けなくても、読み込みは続ける。
pub(crate) fn append_log(log: &Path, line: &str) {
    if let Some(parent) = log.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(log) {
        let _ = writeln!(file, "{line}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str =
        "name: sample\nversion: 0.1.0\napi_version: 1\ndescription: a test\nauthor: someone\n";

    #[test]
    fn accepts_names_that_are_identifiers_once_dashes_become_underscores() {
        assert!(valid_name("sample"));
        assert!(valid_name("my-plugin_2"));
        assert!(!valid_name("2fast"));
        assert!(!valid_name("../escape"));
        assert!(!valid_name(""));
        assert!(!valid_name(&"a".repeat(65)));
    }

    #[test]
    fn parses_a_good_manifest() {
        let m = parse_manifest(GOOD, "sample").expect("valid");
        assert_eq!(m.name, "sample");
        assert_eq!(m.version, "0.1.0");
        assert_eq!(m.api_version, 1);
        assert_eq!(m.author, "someone");
    }

    #[test]
    fn rejects_a_manifest_that_does_not_fit() {
        let other_name = parse_manifest(GOOD, "other").unwrap_err();
        assert!(other_name.contains("does not match"), "{other_name}");

        let newer_api = parse_manifest(&GOOD.replace("api_version: 1", "api_version: 2"), "sample")
            .unwrap_err();
        assert!(
            newer_api.contains("api_version 2 is not supported"),
            "{newer_api}"
        );

        let no_version = parse_manifest("name: sample\napi_version: 1\n", "sample").unwrap_err();
        assert!(no_version.contains("'version'"), "{no_version}");

        let not_a_mapping = parse_manifest("- a\n- b\n", "sample").unwrap_err();
        assert!(not_a_mapping.contains("mapping"), "{not_a_mapping}");
    }

    #[test]
    fn mode_defaults_to_resident_and_command_to_empty() {
        let m = parse_manifest(GOOD, "sample").expect("valid");
        assert_eq!(m.mode, PluginMode::Resident);
        assert!(m.command.is_empty());
    }

    #[test]
    fn parses_the_mode_and_the_command() {
        let text = format!("{GOOD}mode: oneshot\ncommand: [python3, run.py, \"{{input}}\"]\n");
        let m = parse_manifest(&text, "sample").expect("valid");
        assert_eq!(m.mode, PluginMode::Oneshot);
        assert_eq!(m.command, ["python3", "run.py", "{input}"]);

        let resident = format!("{GOOD}mode: resident\n");
        assert_eq!(
            parse_manifest(&resident, "sample").unwrap().mode,
            PluginMode::Resident
        );
    }

    #[test]
    fn rejects_a_wrong_mode_or_command() {
        let bad_mode = parse_manifest(&format!("{GOOD}mode: daemon\n"), "sample").unwrap_err();
        assert!(bad_mode.contains("'mode'"), "{bad_mode}");

        for bad in [
            "command: python3\n",
            "command: []\n",
            "command: [python3, 1]\n",
        ] {
            let error = parse_manifest(&format!("{GOOD}{bad}"), "sample").unwrap_err();
            assert!(error.contains("'command'"), "{bad}: {error}");
        }
    }

    /// テスト用の一時フォルダー（テストごとに別の名前）
    fn scratch(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir =
            std::env::temp_dir().join(format!("yui-plugins-{tag}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&dir).expect("create the test folder");
        dir
    }

    #[test]
    fn reports_missing_and_broken_plugins_and_keeps_going() {
        let dir = scratch("failures");
        let bad = dir.join("bad");
        fs::create_dir_all(&bad).unwrap();
        fs::write(
            bad.join("plugin.yaml"),
            GOOD.replace("sample", "bad")
                .replace("api_version: 1", "api_version: 9"),
        )
        .unwrap();

        let enabled = vec![
            "absent".to_string(),
            "bad".to_string(),
            "absent".to_string(),
        ];
        let warnings = load_all(&dir, &enabled, None);

        assert_eq!(
            warnings.len(),
            2,
            "one warning per plugin, duplicates skipped: {warnings:?}"
        );
        assert!(
            warnings[0].starts_with("plugin absent: cannot read plugin.yaml"),
            "{warnings:?}"
        );
        assert!(
            warnings[1].contains("api_version 9 is not supported"),
            "{warnings:?}"
        );

        let log = fs::read_to_string(dir.join(LOG_FILE)).expect("log written");
        assert!(log.contains("absent: failed"), "{log}");
        assert!(log.contains("bad: failed"), "{log}");
        let _ = fs::remove_dir_all(&dir);
    }

    use crate::plugin_process::child::test_plugin_path;
    use crate::plugin_process::python::find_python;

    /// 動く Python。ない環境では `None` で、その Python を使うテストは読み飛ばす
    fn python_or_skip() -> Option<String> {
        let python = find_python();
        if python.is_none() {
            eprintln!("plugins: skipped (no Python found)");
        }
        python
    }

    /// `dir/<name>/` に、`plugin.yaml`（`GOOD` の名前を替えたもの。`extra` を足す）と `__init__.py` を作る
    fn make_plugin(dir: &Path, name: &str, extra: &str, init: Option<&str>) -> PathBuf {
        let folder = dir.join(name);
        fs::create_dir_all(&folder).unwrap();
        fs::write(
            folder.join("plugin.yaml"),
            format!("{}{extra}", GOOD.replace("sample", name)),
        )
        .unwrap();
        if let Some(init) = init {
            fs::write(folder.join("__init__.py"), init).unwrap();
        }
        folder
    }

    fn log_text(dir: &Path) -> String {
        fs::read_to_string(dir.join(LOG_FILE)).unwrap_or_default()
    }

    /// Python のプラグインが、`app.log` を使い、同じパッケージの別のモジュールを相対 import し、
    /// 失敗した別のプラグインは警告になる。
    #[test]
    fn a_plugin_registers_logs_and_fails_alone() {
        let Some(python) = python_or_skip() else {
            return;
        };
        let dir = scratch("python");
        let good = make_plugin(
            &dir,
            "good-one",
            "",
            Some("from . import helper\n\ndef register(app):\n    app.log(helper.VALUE)\n"),
        );
        fs::write(good.join("helper.py"), "VALUE = 'from helper'\n").unwrap();
        make_plugin(
            &dir,
            "broken",
            "",
            Some("def register(app):\n    raise RuntimeError('boom')\n"),
        );

        let enabled = vec!["good-one".to_string(), "broken".to_string()];
        let warnings = load_all(&dir, &enabled, Some(&python));
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].starts_with("plugin broken: RuntimeError: boom"),
            "{warnings:?}"
        );

        let log = log_text(&dir);
        assert!(log.contains("good-one 0.1.0: loaded"), "{log}");
        assert!(log.contains("good-one: from helper"), "{log}");
        assert!(log.contains("broken: failed: RuntimeError: boom"), "{log}");
        stop_plugin("good-one");
        let _ = fs::remove_dir_all(&dir);
    }

    /// 登録したコマンドを、プラグインのプロセスで実行する。例外は失敗として返る。
    #[test]
    fn a_menu_command_runs_in_the_plugin_process_and_a_failing_one_reports_it() {
        let Some(python) = python_or_skip() else {
            return;
        };
        let dir = scratch("command");
        make_plugin(
            &dir,
            "cmd",
            "",
            Some(concat!(
                "def register(app):\n",
                "    def say(ctx, args):\n",
                "        app.log('said ' + str(args['n']) + ' at ' + str(ctx.target['atom_id']))\n",
                "    def fail(ctx, args):\n",
                "        raise ValueError('bad')\n",
                "    app.add_command('plugin.cmd.say', say)\n",
                "    app.add_command('plugin.cmd.fail', fail)\n",
                "    app.add_menu_item('menu/view', 500, 'Say', 'plugin.cmd.say', {'n': 7})\n",
            )),
        );
        let warnings = load_all(&dir, &["cmd".to_string()], Some(&python));
        assert!(warnings.is_empty(), "{warnings:?}");

        let said = invoke(
            "cmd",
            "plugin.cmd.say",
            &serde_json::json!({"target": {"kind": "atom", "atom_id": 42}, "args": {"n": 7}}),
        )
        .expect("the plugin is running");
        let outcome = crate::plugin_menu::command_outcome(
            said.recv_timeout(Duration::from_secs(10)).expect("replies"),
        );
        assert_eq!(outcome, Ok(()));
        assert!(
            log_text(&dir).contains("cmd: said 7 at 42"),
            "{}",
            log_text(&dir)
        );

        let failed = invoke("cmd", "plugin.cmd.fail", &serde_json::json!({}))
            .expect("the plugin is running");
        let outcome = crate::plugin_menu::command_outcome(
            failed
                .recv_timeout(Duration::from_secs(10))
                .expect("replies"),
        );
        assert_eq!(outcome, Err("ValueError: bad".to_string()));
        stop_plugin("cmd");
        let _ = fs::remove_dir_all(&dir);
    }

    /// 同梱のサンプル（`examples/plugins/` の 2 つ）が、そのまま読めて、コマンドも動く。
    #[test]
    fn the_example_plugins_load_and_their_commands_run() {
        let Some(python) = python_or_skip() else {
            return;
        };
        let dir = scratch("example");
        let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/plugins");
        for name in ["hello-yui", "atom-info"] {
            let target = dir.join(name);
            fs::create_dir_all(&target).unwrap();
            for file in ["plugin.yaml", "__init__.py"] {
                fs::copy(examples.join(name).join(file), target.join(file))
                    .expect("the example is in the repository");
            }
        }

        let enabled = vec!["hello-yui".to_string(), "atom-info".to_string()];
        let warnings = load_all(&dir, &enabled, Some(&python));
        assert!(warnings.is_empty(), "{warnings:?}");

        let wait = |reply: Receiver<CommandOutcome>| {
            crate::plugin_menu::command_outcome(
                reply
                    .recv_timeout(Duration::from_secs(10))
                    .expect("replies"),
            )
        };
        let hello = invoke(
            "hello-yui",
            "plugin.hello-yui.say",
            &serde_json::json!({"target": null, "args": {"greeting": "hi"}}),
        )
        .expect("running");
        assert_eq!(wait(hello), Ok(()));
        let atom = invoke(
            "atom-info",
            "plugin.atom-info.show",
            &serde_json::json!({"target": {"kind": "atom", "atom_id": 42}, "args": {}}),
        )
        .expect("running");
        assert_eq!(wait(atom), Ok(()));

        let text = log_text(&dir);
        assert!(text.contains("hello-yui: hi from the View menu"), "{text}");
        assert!(text.contains("atom-info: atom 42"), "{text}");
        stop_plugin("hello-yui");
        stop_plugin("atom-info");
        let _ = fs::remove_dir_all(&dir);
    }

    /// `command` のあるプラグインは、そのまま実行する（Python でなくてよい）。
    #[test]
    fn a_plugin_with_its_own_command_runs_as_it_is() {
        let dir = scratch("own-command");
        let program = test_plugin_path();
        make_plugin(
            &dir,
            "test-plugin",
            &format!("command: [\"{program}\"]\n"),
            None,
        );
        let warnings = load_all(&dir, &["test-plugin".to_string()], None);
        assert!(warnings.is_empty(), "{warnings:?}");

        let echoed = invoke(
            "test-plugin",
            "plugin.test-plugin.echo",
            &serde_json::json!({"target": null, "args": {"n": 3}}),
        )
        .expect("running");
        assert_eq!(
            crate::plugin_menu::command_outcome(
                echoed
                    .recv_timeout(Duration::from_secs(10))
                    .expect("replies")
            ),
            Ok(())
        );
        stop_plugin("test-plugin");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_plugin_that_does_not_fit_the_rules_is_refused_with_a_reason() {
        let dir = scratch("refused");
        let program = test_plugin_path();

        // 名前が、プラグインの自己申告と違う
        make_plugin(&dir, "other", &format!("command: [\"{program}\"]\n"), None);
        // 都度呼び出しは、まだ対応していない
        make_plugin(&dir, "later", "mode: oneshot\ncommand: [\"x\"]\n", None);
        // `command` も `__init__.py` もない
        make_plugin(&dir, "empty", "", None);
        // 起動できない `command`
        make_plugin(
            &dir,
            "nobin",
            "command: [\"yui-no-such-program-xyz\"]\n",
            None,
        );

        let enabled: Vec<String> = ["other", "later", "empty", "nobin"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let warnings = load_all(&dir, &enabled, None);
        assert_eq!(warnings.len(), 4, "{warnings:?}");
        let has = |needle: &str| warnings.iter().any(|w| w.contains(needle));
        assert!(
            has("says its name is 'test-plugin', not 'other'"),
            "{warnings:?}"
        );
        assert!(has("mode 'oneshot' is not supported yet"), "{warnings:?}");
        assert!(has("no 'command' and no __init__.py"), "{warnings:?}");
        assert!(
            has("cannot start 'yui-no-such-program-xyz'"),
            "{warnings:?}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// 登録が時間内に届かないプラグイン（`silent` は `hello` だけを送る）は、失敗になる
    #[test]
    fn a_plugin_that_never_registers_fails_after_the_wait() {
        let dir = scratch("quiet");
        let program = test_plugin_path();
        make_plugin(
            &dir,
            "test-plugin",
            &format!("command: [\"{program}\", \"silent\"]\n"),
            None,
        );
        let warnings = load_all(&dir, &["test-plugin".to_string()], None);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].contains("did not register within 5 seconds"),
            "{warnings:?}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_python_points_at_the_setting() {
        let dir = scratch("no-python");
        make_plugin(&dir, "pyless", "", Some("def register(app):\n    pass\n"));
        let warnings = load_all(
            &dir,
            &["pyless".to_string()],
            Some("/no/such/python-for-yui"),
        );
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("cannot start"), "{warnings:?}");
        assert!(warnings[0].contains("plugins.python"), "{warnings:?}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_command_for_a_plugin_that_is_not_running_fails_at_once() {
        let err = invoke(
            "never-started",
            "plugin.never-started.x",
            &serde_json::json!({}),
        )
        .unwrap_err();
        assert!(err.contains("not running"), "{err}");
    }
}
