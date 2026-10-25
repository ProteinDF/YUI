//! Python プラグインの読み込み（段階 6k-ii、architecture.md §6.4 と §8.6）。
//!
//! `<config>/yui/plugins/<name>/` の `plugin.yaml` を検査し、`settings.yaml` の `plugins: enabled:` に
//! 書かれたものだけを読む。失敗は、そのプラグインだけを外して警告にする（アプリは止まらない）。
//! 読み込みと失敗は `plugin.log` に書く（Log Viewer で開ける）。

use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[cfg(feature = "python")]
use std::sync::Mutex;
#[cfg(feature = "python")]
use std::sync::mpsc::{Receiver, RecvTimeoutError};
#[cfg(feature = "python")]
use std::time::Duration;

#[cfg(feature = "python")]
use crate::python_runtime::PythonRuntime;

/// このアプリが対応するプラグイン API の版（架空の版ではなく、`plugin.yaml` の `api_version` と比べる）
pub const API_VERSION: u64 = 1;
/// プラグインのログ（読み込みの結果、`app.log`、失敗）
pub const LOG_FILE: &str = "plugin.log";
/// 1 回の読み込み（`register` の呼び出し）を待つ時間。過ぎたら、そのプラグインを失敗にする。
#[cfg(feature = "python")]
const LOAD_WAIT: Duration = Duration::from_secs(5);

/// Python の専用スレッド。読み込みの最初のプラグインで起動し、アプリの終わりまで残す。
#[cfg(feature = "python")]
static HOST: Mutex<Option<PythonRuntime>> = Mutex::new(None);

/// `plugin.yaml` の内容
#[derive(Debug, Clone, PartialEq)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    pub api_version: u64,
    // 表示は段階 6k-iv（プラグインの一覧）で使う
    pub description: String,
    pub author: String,
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
    Ok(Manifest {
        name,
        version,
        api_version,
        description: string("description").unwrap_or_default(),
        author: string("author").unwrap_or_default(),
    })
}

/// 起動時に呼ぶ。読み込めなかったものの理由（警告）を返す。
pub fn start(enabled: &[String]) -> Vec<String> {
    if enabled.is_empty() {
        return Vec::new();
    }
    match plugins_dir() {
        Some(dir) => load_all(&dir, enabled),
        None => vec!["plugins: no configuration folder; no plugin is loaded".into()],
    }
}

/// `enabled` に並んだプラグインを、`dir` から読む。結果は `plugin.log` にも書く。
pub fn load_all(dir: &Path, enabled: &[String]) -> Vec<String> {
    let log = dir.join(LOG_FILE);
    let mut warnings = Vec::new();
    let mut seen = HashSet::new();
    for name in enabled {
        if !seen.insert(name.as_str()) {
            continue;
        }
        match load_one(dir, name, &log) {
            Ok((version, contribution)) => {
                append_log(&log, &format!("{name} {version}: loaded"));
                crate::plugin_menu::record_loaded(contribution);
            }
            Err(reason) => {
                append_log(&log, &format!("{name}: failed: {reason}"));
                warnings.push(format!("plugin {name}: {reason}"));
            }
        }
    }
    warnings
}

fn load_one(
    dir: &Path,
    name: &str,
    log: &Path,
) -> Result<(String, crate::plugin_menu::Contribution), String> {
    if !valid_name(name) {
        return Err("invalid plugin name".into());
    }
    let folder = dir.join(name);
    let text = fs::read_to_string(folder.join("plugin.yaml"))
        .map_err(|e| format!("cannot read plugin.yaml ({e})"))?;
    let manifest = parse_manifest(&text, name)?;
    if !folder.join("__init__.py").is_file() {
        return Err("the package has no __init__.py".into());
    }
    let contribution = run_register(&folder, &manifest, log)?;
    Ok((manifest.version, contribution))
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

#[cfg(not(feature = "python"))]
fn run_register(
    _folder: &Path,
    _manifest: &Manifest,
    _log: &Path,
) -> Result<crate::plugin_menu::Contribution, String> {
    Err("this build has no Python support (build with --features python)".into())
}

#[cfg(feature = "python")]
fn run_register(
    folder: &Path,
    manifest: &Manifest,
    log: &Path,
) -> Result<crate::plugin_menu::Contribution, String> {
    let mut host = HOST
        .lock()
        .map_err(|_| "the Python host is unusable".to_string())?;
    if host.is_none() {
        let runtime = PythonRuntime::start().map_err(|e| format!("{e}; plugins are disabled"))?;
        *host = Some(runtime);
    }
    let runtime = host
        .as_ref()
        .ok_or_else(|| "the Python host is unusable".to_string())?;
    wait(runtime.exec(&load_code(folder, &manifest.name, log)))?;
    // 登録の関数で起きた例外は、Python 側で `_yui_plugin_error` に入れてある
    match wait(runtime.eval("_yui_plugin_error"))? {
        outcome if outcome == "None" => {}
        outcome => return Err(outcome),
    }
    // 成功したら、登録したコマンドと項目の一覧（JSON）を受け取り、検査する
    let export = wait(runtime.eval("_yui_plugin_export"))?;
    crate::plugin_menu::contribution_from_export(&manifest.name, &export)
}

#[cfg(feature = "python")]
fn wait(reply: Receiver<Result<String, String>>) -> Result<String, String> {
    match reply.recv_timeout(LOAD_WAIT) {
        Ok(result) => result,
        Err(RecvTimeoutError::Timeout) => Err(format!(
            "did not finish within {} seconds",
            LOAD_WAIT.as_secs()
        )),
        Err(RecvTimeoutError::Disconnected) => Err("the Python thread stopped".into()),
    }
}

/// プラグインのコマンドを、Python の専用スレッドで呼ぶ。返るのは結果の受け口（待つのは呼び出し側）。
#[cfg(feature = "python")]
pub fn send_command(
    id: &str,
    payload: &serde_json::Value,
) -> Result<Receiver<Result<String, String>>, String> {
    let host = HOST
        .lock()
        .map_err(|_| "the Python host is unusable".to_string())?;
    let runtime = host
        .as_ref()
        .ok_or_else(|| "the Python host is not running".to_string())?;
    Ok(runtime.eval(&format!(
        "_yui_run_plugin_command({}, {})",
        py_str(id),
        py_str(&payload.to_string())
    )))
}

#[cfg(not(feature = "python"))]
pub fn send_command(
    _id: &str,
    _payload: &serde_json::Value,
) -> Result<std::sync::mpsc::Receiver<Result<String, String>>, String> {
    Err("this build has no Python support (build with --features python)".into())
}

/// Python の文字列の書き方（JSON の文字列は、Python の文字列としても同じに読める）
fn py_str(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".into())
}

/// プラグインを読み込む Python のコード。パスと名前は Python の文字列として埋める。
#[cfg(feature = "python")]
fn load_code(folder: &Path, name: &str, log: &Path) -> String {
    let module = name.replace('-', "_");
    let init = folder.join("__init__.py");
    let template = r#"
import importlib.util as _yui_util
import json as _yui_json
import sys as _yui_sys
_yui_plugin_error = None
_yui_plugin_export = None
try:
    _yui_plugin_commands
except NameError:
    _yui_plugin_commands = {}
class _YuiApp:
    def __init__(self, name, log_path):
        self._name = name
        self._log_path = log_path
        self._commands = {}
        self._items = []
    def log(self, message):
        with open(self._log_path, "a", encoding="utf-8") as _f:
            _f.write("[" + self._name + "] " + str(message) + "\n")
    def add_command(self, command_id, func):
        if not callable(func):
            raise TypeError("add_command needs a function")
        self._commands[str(command_id)] = func
    def add_menu_item(self, location, order, label, command, args=None):
        self._items.append({"location": str(location), "order": int(order), "label": str(label), "command": str(command), "args": {} if args is None else args})
class _YuiContext:
    def __init__(self, target):
        self.target = target
def _yui_run_plugin_command(command_id, payload_text):
    try:
        payload = _yui_json.loads(payload_text)
        func = _yui_plugin_commands[command_id]
        func(_YuiContext(payload.get("target")), payload.get("args", {}))
        return ""
    except Exception as _yui_e:
        return type(_yui_e).__name__ + ": " + str(_yui_e)
try:
    _yui_spec = _yui_util.spec_from_file_location("yui_plugin_MODULE", INIT_PATH, submodule_search_locations=[DIR_PATH])
    _yui_module = _yui_util.module_from_spec(_yui_spec)
    _yui_sys.modules[_yui_spec.name] = _yui_module
    _yui_spec.loader.exec_module(_yui_module)
    _yui_app = _YuiApp(NAME_TEXT, LOG_PATH)
    _yui_module.register(_yui_app)
    _yui_plugin_export = _yui_json.dumps({"commands": list(_yui_app._commands), "items": _yui_app._items})
    _yui_plugin_commands.update(_yui_app._commands)
except Exception as _yui_e:
    _yui_plugin_error = type(_yui_e).__name__ + ": " + str(_yui_e)
"#;
    template
        .replace("MODULE", &module)
        .replace("INIT_PATH", &py_str(&init.to_string_lossy()))
        .replace("DIR_PATH", &py_str(&folder.to_string_lossy()))
        .replace("NAME_TEXT", &py_str(name))
        .replace("LOG_PATH", &py_str(&log.to_string_lossy()))
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
        let warnings = load_all(&dir, &enabled);

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

    /// Python を実際に動かす。`register` が `app.log` を使い、同じパッケージの別のモジュールを
    /// 相対 import し、失敗した別のプラグインは警告になる。
    #[cfg(feature = "python")]
    #[test]
    fn a_plugin_registers_logs_and_fails_alone() {
        let dir = scratch("python");
        let good = dir.join("good-one");
        fs::create_dir_all(&good).unwrap();
        fs::write(good.join("plugin.yaml"), GOOD.replace("sample", "good-one")).unwrap();
        fs::write(good.join("helper.py"), "VALUE = 'from helper'\n").unwrap();
        fs::write(
            good.join("__init__.py"),
            "from . import helper\n\ndef register(app):\n    app.log(helper.VALUE)\n",
        )
        .unwrap();
        let broken = dir.join("broken");
        fs::create_dir_all(&broken).unwrap();
        fs::write(broken.join("plugin.yaml"), GOOD.replace("sample", "broken")).unwrap();
        fs::write(
            broken.join("__init__.py"),
            "def register(app):\n    raise RuntimeError('boom')\n",
        )
        .unwrap();

        let enabled = vec!["good-one".to_string(), "broken".to_string()];
        let warnings = load_all(&dir, &enabled);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].starts_with("plugin broken: RuntimeError: boom"),
            "{warnings:?}"
        );

        let log = fs::read_to_string(dir.join(LOG_FILE)).expect("log written");
        assert!(log.contains("good-one 0.1.0: loaded"), "{log}");
        assert!(log.contains("[good-one] from helper"), "{log}");
        assert!(log.contains("broken: failed: RuntimeError: boom"), "{log}");
        let _ = fs::remove_dir_all(&dir);
    }

    /// 登録したコマンドを、Python の専用スレッドで実行する。例外は失敗として返り、ログには書かれない。
    #[cfg(feature = "python")]
    #[test]
    fn a_menu_command_runs_on_the_python_thread_and_a_failing_one_reports_it() {
        let dir = scratch("command");
        let plugin = dir.join("cmd");
        fs::create_dir_all(&plugin).unwrap();
        fs::write(plugin.join("plugin.yaml"), GOOD.replace("sample", "cmd")).unwrap();
        fs::write(
            plugin.join("__init__.py"),
            concat!(
                "def register(app):\n",
                "    def say(ctx, args):\n",
                "        app.log('said ' + str(args['n']) + ' at ' + str(ctx.target['atom_id']))\n",
                "    def fail(ctx, args):\n",
                "        raise ValueError('bad')\n",
                "    app.add_command('plugin.cmd.say', say)\n",
                "    app.add_command('plugin.cmd.fail', fail)\n",
                "    app.add_menu_item('menu/view', 500, 'Say', 'plugin.cmd.say', {'n': 7})\n",
            ),
        )
        .unwrap();

        let manifest = parse_manifest(&GOOD.replace("sample", "cmd"), "cmd").unwrap();
        let log = dir.join(LOG_FILE);
        let contribution = run_register(&plugin, &manifest, &log).expect("registers");
        assert_eq!(
            contribution.commands,
            vec!["plugin.cmd.say".to_string(), "plugin.cmd.fail".to_string()]
        );
        assert_eq!(contribution.items.len(), 1);

        let said = send_command(
            "plugin.cmd.say",
            &serde_json::json!({"target": {"kind": "atom", "atom_id": 42}, "args": {"n": 7}}),
        )
        .expect("the host runs");
        let outcome = crate::plugin_menu::command_outcome(said.recv().expect("replies"));
        assert_eq!(outcome, Ok(()));
        let text = fs::read_to_string(&log).expect("log written");
        assert!(text.contains("[cmd] said 7 at 42"), "{text}");

        let failed =
            send_command("plugin.cmd.fail", &serde_json::json!({})).expect("the host runs");
        let outcome = crate::plugin_menu::command_outcome(failed.recv().expect("replies"));
        assert_eq!(outcome, Err("ValueError: bad".to_string()));
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(not(feature = "python"))]
    #[test]
    fn a_build_without_python_says_so() {
        let dir = scratch("nopython");
        let plugin = dir.join("sample");
        fs::create_dir_all(&plugin).unwrap();
        fs::write(plugin.join("plugin.yaml"), GOOD).unwrap();
        fs::write(plugin.join("__init__.py"), "def register(app):\n    pass\n").unwrap();

        let warnings = load_all(&dir, &["sample".to_string()]);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("no Python support"), "{warnings:?}");
        let _ = fs::remove_dir_all(&dir);
    }
}
