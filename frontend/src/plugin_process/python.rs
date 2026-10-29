//! Python で書いたプラグインを動かす（architecture.md §6.4）。
//!
//! `plugin.yaml` に `command` がなく、`__init__.py` があるプラグインは、YUI が同梱するランナー
//! （`runner.py`）を、決めた Python で起動して動かす。ランナーが `register(app)` を呼び、結果を
//! `protocol` のメッセージで YUI に送る。

use std::path::Path;
#[cfg(test)]
use std::process::Command;

/// ランナーの Python のコード（`python -c` に渡す）
pub const RUNNER: &str = include_str!("runner.py");

/// 設定がないときに使う Python（`PATH` から探す）。Windows には `python3` の名前がないことが多い。
pub fn default_python() -> &'static str {
    if cfg!(windows) { "python" } else { "python3" }
}

/// ランナーを起動するコマンド（プログラムと引数。シェルは通さない）。
/// `-X utf8` は、OS の文字コードに左右されず、UTF-8 で読み書きするため。
pub fn command_for(python: &str, folder: &Path, name: &str, version: &str) -> Vec<String> {
    vec![
        python.to_string(),
        "-X".into(),
        "utf8".into(),
        "-c".into(),
        RUNNER.to_string(),
        folder.to_string_lossy().into_owned(),
        name.to_string(),
        version.to_string(),
    ]
}

/// `oneshot` のプラグインを動かすコマンド。`{input}` と `{output}` は、呼ぶたびに置き換える。
pub fn oneshot_command_for(python: &str, folder: &Path, name: &str, version: &str) -> Vec<String> {
    let mut argv = command_for(python, folder, name, version);
    argv.extend([
        "--oneshot".to_string(),
        super::oneshot::INPUT_PLACEHOLDER.to_string(),
        super::oneshot::OUTPUT_PLACEHOLDER.to_string(),
    ]);
    argv
}

#[cfg(test)]
/// 動く Python。見つからない環境（Python が入っていない）では `None`で、テストは読み飛ばす。
pub(crate) fn find_python() -> Option<String> {
    [default_python(), "python3", "python"]
        .into_iter()
        .find(|candidate| {
            Command::new(candidate)
                .arg("--version")
                .output()
                .is_ok_and(|out| out.status.success())
        })
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin_process::child::ChildPlugin;
    use crate::plugin_process::protocol::Message;
    use rmpv::Value;
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command;
    use std::thread;
    use std::time::{Duration, Instant};

    const WAIT: Duration = Duration::from_secs(10);

    /// `msgpack` のパッケージが入っているか
    fn has_msgpack_package(python: &str) -> bool {
        Command::new(python)
            .args(["-c", "import msgpack"])
            .output()
            .is_ok_and(|out| out.status.success())
    }

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let dir = std::env::temp_dir()
                .join(format!("yui-runner-{tag}-{}-{nanos}", std::process::id()));
            fs::create_dir_all(&dir).expect("scratch folder");
            Scratch(dir)
        }

        fn log(&self) -> PathBuf {
            self.0.join("plugin.log")
        }

        fn log_text(&self) -> String {
            fs::read_to_string(self.log()).unwrap_or_default()
        }

        /// プラグインのフォルダー（`<scratch>/sample/__init__.py`）を作る
        fn plugin(&self, source: &str) -> PathBuf {
            let folder = self.0.join("sample");
            fs::create_dir_all(&folder).expect("plugin folder");
            fs::write(folder.join("__init__.py"), source).expect("write __init__.py");
            folder
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn wait_until(mut condition: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + WAIT;
        while Instant::now() < deadline {
            if condition() {
                return true;
            }
            thread::sleep(Duration::from_millis(10));
        }
        condition()
    }

    const PLUGIN: &str = r#"
def register(app):
    def echo(ctx, args):
        return args

    def target(ctx, args):
        app.log("target=%r" % (ctx.target,))
        print("printed to stdout")

    def boom(ctx, args):
        raise ValueError("boom")

    def unsendable(ctx, args):
        return {1, 2, 3}

    app.add_command("plugin.sample.echo", echo)
    app.add_command("plugin.sample.target", target)
    app.add_command("plugin.sample.boom", boom)
    app.add_command("plugin.sample.unsendable", unsendable)
    app.add_menu_item("menu/view", 500, "Echo", "plugin.sample.echo", {"greeting": "hi"})
    app.add_menu_item("context/viewport", 100, "Target", "plugin.sample.target")
"#;

    /// ランナーでプラグインを起動する。`builtin` なら、`msgpack` の自前の実装を使わせる。
    fn start(python: &str, scratch: &Scratch, source: &str, builtin: bool) -> ChildPlugin {
        let folder = scratch.plugin(source);
        let mut argv = command_for(python, &folder, "sample", "0.1.0");
        if builtin {
            argv.push("--builtin-msgpack".into());
        }
        ChildPlugin::spawn("sample", &argv, &folder, &scratch.log()).expect("the runner starts")
    }

    /// 自前の実装と、`msgpack` のパッケージ（あれば）の両方で確かめる
    fn for_each_msgpack(test: impl Fn(&str, bool)) {
        let Some(python) = find_python() else {
            eprintln!("runner: skipped (no Python found)");
            return;
        };
        test(&python, true);
        if has_msgpack_package(&python) {
            test(&python, false);
        } else {
            eprintln!(
                "runner: the msgpack package is not installed; only the built-in one was tested"
            );
        }
    }

    #[test]
    fn the_command_runs_the_runner_with_the_plugin_folder_name_and_version() {
        let argv = command_for("python3", Path::new("/p/sample"), "sample", "0.1.0");
        assert_eq!(&argv[..4], ["python3", "-X", "utf8", "-c"]);
        assert_eq!(argv[4], RUNNER);
        assert_eq!(&argv[5..], ["/p/sample", "sample", "0.1.0"]);
    }

    #[test]
    fn a_python_plugin_says_hello_and_registers_its_commands_and_items() {
        for_each_msgpack(|python, builtin| {
            let scratch = Scratch::new("register");
            let plugin = start(python, &scratch, PLUGIN, builtin);
            match plugin.recv_event(WAIT) {
                Ok(Message::Hello {
                    name,
                    version,
                    api_version,
                }) => {
                    assert_eq!(
                        (name.as_str(), version.as_str(), api_version),
                        ("sample", "0.1.0", 1)
                    );
                }
                other => panic!("expected hello, got {other:?} ({})", scratch.log_text()),
            }
            match plugin.recv_event(WAIT) {
                Ok(Message::Register {
                    commands,
                    menu_items,
                }) => {
                    assert_eq!(
                        commands,
                        [
                            "plugin.sample.echo",
                            "plugin.sample.target",
                            "plugin.sample.boom",
                            "plugin.sample.unsendable"
                        ]
                    );
                    assert_eq!(menu_items.len(), 2);
                    assert_eq!(menu_items[0].location, "menu/view");
                    assert_eq!(menu_items[0].order, 500);
                    assert_eq!(menu_items[0].label, "Echo");
                    assert_eq!(
                        menu_items[0].args,
                        Value::Map(vec![(Value::from("greeting"), Value::from("hi"))])
                    );
                    // `args` を省いたときは、空の連想配列（今までと同じ）
                    assert_eq!(menu_items[1].location, "context/viewport");
                    assert_eq!(menu_items[1].args, Value::Map(vec![]));
                }
                other => panic!("expected register, got {other:?} ({})", scratch.log_text()),
            }
        });
    }

    #[test]
    fn values_survive_the_trip_to_python_and_back() {
        for_each_msgpack(|python, builtin| {
            let scratch = Scratch::new("values");
            let plugin = start(python, &scratch, PLUGIN, builtin);
            let value = Value::Map(vec![
                (Value::from("int"), Value::from(1)),
                (Value::from("negative"), Value::from(-5)),
                (Value::from("small_negative"), Value::from(-33)),
                (Value::from("u16"), Value::from(40_000)),
                (Value::from("u32"), Value::from(3_000_000_000u64)),
                (Value::from("big"), Value::from(1_099_511_627_776i64)),
                (Value::from("i64"), Value::from(-4_000_000_000i64)),
                (Value::from("float"), Value::F64(2.5)),
                (Value::from("true"), Value::Boolean(true)),
                (Value::from("false"), Value::Boolean(false)),
                (Value::from("nil"), Value::Nil),
                (Value::from("text"), Value::from("日本語 and ascii")),
                (Value::from("long_text"), Value::from("x".repeat(300))),
                (Value::from("huge_text"), Value::from("y".repeat(70_000))),
                (
                    Value::from("list"),
                    Value::Array((0..20).map(Value::from).collect()),
                ),
                (
                    Value::from("nested"),
                    Value::Array(vec![
                        Value::from(1),
                        Value::Nil,
                        Value::Map(vec![(Value::from("k"), Value::Array(vec![]))]),
                    ]),
                ),
                (
                    Value::from("many_keys"),
                    Value::Map(
                        (0..20)
                            .map(|i| (Value::from(format!("k{i}")), Value::from(i)))
                            .collect(),
                    ),
                ),
            ]);
            let rx = plugin.invoke("plugin.sample.echo", Value::Nil, value.clone());
            let outcome = rx.recv_timeout(WAIT).expect("an answer");
            assert_eq!(outcome, Ok(Some(value)), "builtin={builtin}");
        });
    }

    #[test]
    fn the_target_reaches_the_command_and_print_goes_to_the_log_not_the_protocol() {
        for_each_msgpack(|python, builtin| {
            let scratch = Scratch::new("target");
            let plugin = start(python, &scratch, PLUGIN, builtin);
            let target = Value::Map(vec![
                (Value::from("kind"), Value::from("atom")),
                (Value::from("atom_id"), Value::from(12)),
            ]);
            let rx = plugin.invoke("plugin.sample.target", target, Value::Nil);
            // `print()` が通信を壊していれば、ここで結果が届かない
            assert_eq!(rx.recv_timeout(WAIT).expect("an answer"), Ok(None));
            assert!(
                scratch
                    .log_text()
                    .contains("sample: target={'kind': 'atom', 'atom_id': 12}"),
                "{}",
                scratch.log_text()
            );
            assert!(
                wait_until(|| scratch.log_text().contains("sample: printed to stdout")),
                "{}",
                scratch.log_text()
            );
        });
    }

    #[test]
    fn an_exception_is_a_failed_result_with_its_reason_and_the_plugin_goes_on() {
        for_each_msgpack(|python, builtin| {
            let scratch = Scratch::new("boom");
            let plugin = start(python, &scratch, PLUGIN, builtin);
            let rx = plugin.invoke("plugin.sample.boom", Value::Nil, Value::Nil);
            assert_eq!(
                rx.recv_timeout(WAIT).expect("an answer"),
                Err("ValueError: boom".to_string())
            );
            // 失敗のあとも動く（失敗の扱いは YUI の側が決める）
            let rx = plugin.invoke("plugin.sample.echo", Value::Nil, Value::from(7));
            assert_eq!(rx.recv_timeout(WAIT).unwrap(), Ok(Some(Value::from(7))));
            // 知らないコマンド
            let rx = plugin.invoke("plugin.sample.nope", Value::Nil, Value::Nil);
            let reason = rx.recv_timeout(WAIT).unwrap().unwrap_err();
            assert!(reason.contains("unknown command"), "{reason}");
            // 送れない値を返したときは、失敗の結果になる（通信は壊れない）
            let rx = plugin.invoke("plugin.sample.unsendable", Value::Nil, Value::Nil);
            let reason = rx.recv_timeout(WAIT).unwrap().unwrap_err();
            assert!(reason.contains("cannot be sent"), "{reason}");
            let rx = plugin.invoke("plugin.sample.echo", Value::Nil, Value::from(8));
            assert_eq!(rx.recv_timeout(WAIT).unwrap(), Ok(Some(Value::from(8))));
        });
    }

    #[test]
    fn a_failure_in_register_ends_the_process_and_is_written_to_the_log() {
        for_each_msgpack(|python, builtin| {
            let scratch = Scratch::new("bad-register");
            let plugin = start(
                python,
                &scratch,
                "def register(app):\n    raise RuntimeError('cannot register')\n",
                builtin,
            );
            // `hello` も `register` も来ずに、プロセスが終わる
            assert!(wait_until(|| !plugin.is_alive()), "{}", scratch.log_text());
            assert!(
                wait_until(|| scratch
                    .log_text()
                    .contains("sample: register failed: RuntimeError: cannot register")),
                "{}",
                scratch.log_text()
            );
        });
    }

    #[test]
    fn a_plugin_that_does_not_import_ends_the_process_too() {
        for_each_msgpack(|python, builtin| {
            let scratch = Scratch::new("syntax");
            let plugin = start(python, &scratch, "def register(app:\n", builtin);
            assert!(wait_until(|| !plugin.is_alive()), "{}", scratch.log_text());
            assert!(
                wait_until(|| scratch.log_text().contains("register failed: SyntaxError")),
                "{}",
                scratch.log_text()
            );
        });
    }

    #[test]
    fn without_the_msgpack_package_the_runner_uses_its_own_implementation() {
        let Some(python) = find_python() else {
            eprintln!("runner: skipped (no Python found)");
            return;
        };
        let scratch = Scratch::new("no-package");
        let folder = scratch.plugin(PLUGIN);
        // `-S` は site-packages を読まない。`msgpack` を入れてあっても、import に失敗する。
        let mut argv = command_for(&python, &folder, "sample", "0.1.0");
        argv.insert(1, "-S".into());
        let plugin = ChildPlugin::spawn("sample", &argv, &folder, &scratch.log())
            .expect("the runner starts");
        assert!(
            matches!(plugin.recv_event(WAIT), Ok(Message::Hello { .. })),
            "{}",
            scratch.log_text()
        );
        let value = Value::Map(vec![(Value::from("n"), Value::from(-70_000))]);
        let rx = plugin.invoke("plugin.sample.echo", Value::Nil, value.clone());
        assert_eq!(rx.recv_timeout(WAIT).expect("an answer"), Ok(Some(value)));
    }

    /// ランナーを `--oneshot` で動かす呼び出し方（`builtin` なら自前の msgpack を使わせる）
    fn oneshot_plugin(
        python: &str,
        scratch: &Scratch,
        source: &str,
        builtin: bool,
    ) -> crate::plugin_process::oneshot::OneshotPlugin {
        let folder = scratch.plugin(source);
        let mut argv = oneshot_command_for(python, &folder, "sample", "0.1.0");
        if builtin {
            argv.push("--builtin-msgpack".into());
        }
        crate::plugin_process::oneshot::OneshotPlugin::new("sample", &folder, argv, &scratch.log())
    }

    #[test]
    fn the_oneshot_command_has_the_input_and_output_marks_at_the_end() {
        let argv = oneshot_command_for("python3", Path::new("/p/sample"), "sample", "0.1.0");
        assert_eq!(
            &argv[argv.len() - 3..],
            ["--oneshot", "{input}", "{output}"]
        );
        assert!(crate::plugin_process::oneshot::has_placeholders(&argv));
    }

    #[test]
    fn a_oneshot_call_runs_one_command_and_returns_its_value() {
        for_each_msgpack(|python, builtin| {
            let scratch = Scratch::new("once");
            let plugin = oneshot_plugin(python, &scratch, PLUGIN, builtin);
            let value = Value::Map(vec![
                (Value::from("n"), Value::from(-70_000)),
                (Value::from("text"), Value::from("日本語")),
                (
                    Value::from("list"),
                    Value::Array(vec![Value::Nil, Value::F64(2.5)]),
                ),
            ]);
            let rx = plugin.call("plugin.sample.echo", Value::Nil, value.clone());
            assert_eq!(
                rx.recv_timeout(WAIT).expect("an answer"),
                Ok(Some(value)),
                "builtin={builtin}"
            );
        });
    }

    #[test]
    fn a_oneshot_call_gets_the_target_and_print_goes_to_the_log() {
        for_each_msgpack(|python, builtin| {
            let scratch = Scratch::new("once-target");
            let plugin = oneshot_plugin(python, &scratch, PLUGIN, builtin);
            let target = Value::Map(vec![
                (Value::from("kind"), Value::from("atom")),
                (Value::from("atom_id"), Value::from(12)),
            ]);
            let rx = plugin.call("plugin.sample.target", target, Value::Nil);
            assert_eq!(rx.recv_timeout(WAIT).expect("an answer"), Ok(None));
            let log = scratch.log_text();
            assert!(
                log.contains("sample: target={'kind': 'atom', 'atom_id': 12}"),
                "{log}"
            );
            assert!(log.contains("sample: printed to stdout"), "{log}");
        });
    }

    #[test]
    fn a_oneshot_failure_has_its_reason() {
        for_each_msgpack(|python, builtin| {
            let scratch = Scratch::new("once-fail");
            let plugin = oneshot_plugin(python, &scratch, PLUGIN, builtin);
            let rx = plugin.call("plugin.sample.boom", Value::Nil, Value::Nil);
            assert_eq!(
                rx.recv_timeout(WAIT).unwrap(),
                Err("ValueError: boom".to_string())
            );
            let rx = plugin.call("plugin.sample.nope", Value::Nil, Value::Nil);
            let reason = rx.recv_timeout(WAIT).unwrap().unwrap_err();
            assert!(reason.contains("unknown command"), "{reason}");
            // 送れない値を返したときも、失敗の結果になる
            let rx = plugin.call("plugin.sample.unsendable", Value::Nil, Value::Nil);
            let reason = rx.recv_timeout(WAIT).unwrap().unwrap_err();
            assert!(reason.contains("cannot be sent"), "{reason}");
        });
    }

    #[test]
    fn a_oneshot_plugin_that_fails_to_register_reports_the_last_stderr_line() {
        for_each_msgpack(|python, builtin| {
            let scratch = Scratch::new("once-register");
            let plugin = oneshot_plugin(
                python,
                &scratch,
                "def register(app):\n    raise RuntimeError('cannot register')\n",
                builtin,
            );
            let rx = plugin.call("plugin.sample.echo", Value::Nil, Value::Nil);
            let reason = rx.recv_timeout(WAIT).unwrap().unwrap_err();
            assert!(
                reason.contains("register failed: RuntimeError: cannot register"),
                "{reason}"
            );
        });
    }

    #[test]
    fn closing_the_standard_input_ends_the_runner() {
        for_each_msgpack(|python, builtin| {
            let scratch = Scratch::new("stop");
            let plugin = start(python, &scratch, PLUGIN, builtin);
            plugin.recv_event(WAIT).expect("hello");
            let started = Instant::now();
            plugin.shutdown(WAIT);
            assert!(started.elapsed() < Duration::from_secs(5));
        });
    }
}
