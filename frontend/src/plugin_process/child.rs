//! プラグインの子プロセス（architecture.md §6.4）。
//!
//! YUI が親、プラグインが子である。`plugin.yaml` の `command` で起動し、標準入力と標準出力で
//! `protocol` のメッセージをやり取りする。標準エラーは `plugin.log` に書く。プラグインが落ちても、
//! 止まっても、YUI は止まらない。そのプラグインの呼び出しが失敗になるだけである。

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use rmpv::Value;

use super::protocol::{self, Message};
use crate::plugins::append_log;

/// コマンドの結果。成功なら `result`（なければ `None`）、失敗なら理由。
pub type CommandOutcome = Result<Option<Value>, String>;

/// プロセスが終わった（または落ちた）あとに残った呼び出しの理由
const EXITED: &str = "the plugin process exited";
/// `Drop` で、標準入力を閉じたあとに終了を待つ時間
const DROP_GRACE: Duration = Duration::from_millis(500);

/// `recv_event` が返せなかった理由
#[derive(Debug, PartialEq, Eq)]
pub enum EventError {
    /// 時間内に届かなかった
    Timeout,
    /// プロセスが終わって、これ以上届かない
    Closed,
}

/// 読み取りのスレッドと呼び出しの側が共有するもの
struct Shared {
    /// 結果を待っている呼び出し（`id` → 結果の送り先）
    pending: Mutex<HashMap<u64, Sender<CommandOutcome>>>,
    /// プロセスと通信できている間は真
    alive: AtomicBool,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// 起動したプラグインの子プロセス
pub struct ChildPlugin {
    name: String,
    child: Mutex<Child>,
    stdin: Mutex<Option<ChildStdin>>,
    shared: Arc<Shared>,
    events: Mutex<Receiver<Message>>,
    next_id: AtomicU64,
    stopped: AtomicBool,
}

/// `command` の先頭（プログラム）を決める。ディレクトリを含む相対パスは、プラグインのフォルダーから
/// 見たものとして扱う（OS によって解釈が違うのを避けるため）。名前だけなら `PATH` から探す。
pub fn resolve_program(dir: &Path, program: &str) -> PathBuf {
    let path = Path::new(program);
    if path.is_absolute() || path.components().count() == 1 {
        path.to_path_buf()
    } else {
        dir.join(path)
    }
}

impl ChildPlugin {
    /// `argv`（プログラムと引数。シェルは通さない）を、`dir` を作業フォルダーとして起動する。
    /// `Hello` と `Register` は `recv_event` で受ける。標準エラーとログのメッセージは `log` に書く。
    pub fn spawn(name: &str, argv: &[String], dir: &Path, log: &Path) -> Result<Self, String> {
        let program = argv
            .first()
            .ok_or_else(|| "the command is empty".to_string())?;
        let mut child = Command::new(resolve_program(dir, program))
            .args(&argv[1..])
            .current_dir(dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("cannot start '{program}' ({e})"))?;
        let stdin = child.stdin.take().expect("stdin is piped");
        let stdout = child.stdout.take().expect("stdout is piped");
        let stderr = child.stderr.take().expect("stderr is piped");

        let shared = Arc::new(Shared {
            pending: Mutex::new(HashMap::new()),
            alive: AtomicBool::new(true),
        });
        let (events_tx, events_rx) = mpsc::channel();

        {
            let (shared, name, log) = (shared.clone(), name.to_string(), log.to_path_buf());
            thread::spawn(move || read_loop(&name, stdout, &shared, &events_tx, &log));
        }
        {
            let (name, log) = (name.to_string(), log.to_path_buf());
            thread::spawn(move || {
                for line in BufReader::new(stderr).lines() {
                    match line {
                        Ok(line) => append_log(&log, &format!("{name}: {line}")),
                        Err(_) => break,
                    }
                }
            });
        }

        Ok(Self {
            name: name.to_string(),
            child: Mutex::new(child),
            stdin: Mutex::new(Some(stdin)),
            shared,
            events: Mutex::new(events_rx),
            next_id: AtomicU64::new(1),
            stopped: AtomicBool::new(false),
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// プロセスと通信できているか
    pub fn is_alive(&self) -> bool {
        self.shared.alive.load(Ordering::SeqCst)
    }

    /// プラグインが自分から送るメッセージ（`Hello`、`Register`）を、最大 `timeout` 待って 1 つ受ける。
    pub fn recv_event(&self, timeout: Duration) -> Result<Message, EventError> {
        match lock(&self.events).recv_timeout(timeout) {
            Ok(message) => Ok(message),
            Err(RecvTimeoutError::Timeout) => Err(EventError::Timeout),
            Err(RecvTimeoutError::Disconnected) => Err(EventError::Closed),
        }
    }

    /// コマンドの実行を頼む。結果は、返した受け口に 1 回だけ届く（時間の制限は、呼ぶ側が決める）。
    /// 送れなかったとき、プロセスが終わっていたときは、失敗がすぐ入る。
    pub fn invoke(&self, command: &str, target: Value, args: Value) -> Receiver<CommandOutcome> {
        let (tx, rx) = mpsc::channel();
        if !self.is_alive() {
            let _ = tx.send(Err(EXITED.into()));
            return rx;
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        lock(&self.shared.pending).insert(id, tx);
        // 登録の間に読み取りが終わっていたら、誰も結果を届けない。その場で失敗にする。
        if !self.is_alive() {
            self.fail(id, EXITED);
            return rx;
        }
        let message = Message::Invoke {
            id,
            command: command.to_string(),
            target,
            args,
        };
        let written = match lock(&self.stdin).as_mut() {
            Some(stdin) => protocol::write_message(stdin, &message).map_err(|e| e.to_string()),
            None => Err("the plugin was stopped".to_string()),
        };
        if let Err(reason) = written {
            self.fail(id, &format!("cannot send the command ({reason})"));
        }
        rx
    }

    fn fail(&self, id: u64, reason: &str) {
        if let Some(tx) = lock(&self.shared.pending).remove(&id) {
            let _ = tx.send(Err(reason.to_string()));
        }
    }

    /// 標準入力を閉じ、`grace` まで終わるのを待つ。それでも動いていれば止める。
    pub fn stop(self, grace: Duration) {
        self.shutdown(grace);
    }

    fn shutdown(&self, grace: Duration) {
        if self.stopped.swap(true, Ordering::SeqCst) {
            return;
        }
        // 標準入力を閉じる。行儀のよいプラグインは、これで終わる。
        lock(&self.stdin).take();
        let mut child = lock(&self.child);
        let deadline = Instant::now() + grace;
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break;
                }
            }
        }
    }
}

impl Drop for ChildPlugin {
    fn drop(&mut self) {
        self.shutdown(DROP_GRACE);
    }
}

/// 標準出力を読み続ける。終わったら、結果を待っている呼び出しをすべて失敗にする。
fn read_loop(
    name: &str,
    stdout: impl std::io::Read,
    shared: &Shared,
    events: &Sender<Message>,
    log: &Path,
) {
    let mut input = BufReader::new(stdout);
    loop {
        match protocol::read_message(&mut input) {
            Ok(Some(Message::Result {
                id,
                ok,
                error,
                result,
            })) => {
                if let Some(tx) = lock(&shared.pending).remove(&id) {
                    let outcome = if ok {
                        Ok(result)
                    } else {
                        Err(error.unwrap_or_else(|| "the command failed".to_string()))
                    };
                    let _ = tx.send(outcome);
                }
            }
            Ok(Some(Message::Log { text })) => append_log(log, &format!("{name}: {text}")),
            Ok(Some(message @ (Message::Hello { .. } | Message::Register { .. }))) => {
                let _ = events.send(message);
            }
            Ok(Some(Message::Invoke { .. })) => {
                append_log(
                    log,
                    &format!("{name}: ignored an 'invoke' sent by the plugin"),
                );
            }
            Ok(None) => break,
            Err(e) => {
                append_log(log, &format!("{name}: protocol error: {e}"));
                break;
            }
        }
    }
    shared.alive.store(false, Ordering::SeqCst);
    for (_, tx) in lock(&shared.pending).drain() {
        let _ = tx.send(Err(EXITED.to_string()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::OnceLock;

    const WAIT: Duration = Duration::from_secs(5);

    /// テスト用のプラグイン（`examples/yui_test_plugin.rs`）の実行ファイル。`cargo test` はサンプルを
    /// 検査するだけで実行ファイルは作らないので、最初に使うとき、同じ設定で `cargo build` して作る。
    fn test_plugin() -> String {
        static PATH: OnceLock<String> = OnceLock::new();
        PATH.get_or_init(|| {
            let exe = std::env::current_exe().expect("the test executable");
            // <target>/<profile>/deps/<test exe>
            let profile_dir = exe
                .parent()
                .and_then(Path::parent)
                .expect("the profile folder");
            let name = if cfg!(windows) {
                "yui_test_plugin.exe"
            } else {
                "yui_test_plugin"
            };
            let path = profile_dir.join("examples").join(name);

            let mut build = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
            build.args([
                "build",
                "-p",
                "yui-frontend",
                "--example",
                "yui_test_plugin",
            ]);
            if profile_dir.file_name().is_some_and(|n| n == "release") {
                build.arg("--release");
            }
            if let Some(target_dir) = profile_dir.parent() {
                build.arg("--target-dir").arg(target_dir);
            }
            let status = build.status().expect("run cargo build for the test plugin");
            assert!(
                status.success(),
                "cargo build --example yui_test_plugin failed"
            );
            assert!(path.is_file(), "missing {}", path.display());
            path.to_string_lossy().into_owned()
        })
        .clone()
    }

    /// テスト用の一時フォルダー（テストごとに別の名前。終わりに消す）
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let dir = std::env::temp_dir()
                .join(format!("yui-child-{tag}-{}-{nanos}", std::process::id()));
            fs::create_dir_all(&dir).expect("scratch folder");
            Scratch(dir)
        }

        fn log(&self) -> PathBuf {
            self.0.join("plugin.log")
        }

        fn log_text(&self) -> String {
            fs::read_to_string(self.log()).unwrap_or_default()
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn start(scratch: &Scratch, mode: &str) -> ChildPlugin {
        ChildPlugin::spawn(
            "test-plugin",
            &[test_plugin(), mode.to_string()],
            &scratch.0,
            &scratch.log(),
        )
        .expect("the test plugin starts")
    }

    /// 条件が成り立つまで（最大 `WAIT`）待つ
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

    #[test]
    fn a_plugin_says_hello_and_registers() {
        let scratch = Scratch::new("hello");
        let plugin = start(&scratch, "");
        match plugin.recv_event(WAIT) {
            Ok(Message::Hello {
                name, api_version, ..
            }) => {
                assert_eq!(name, "test-plugin");
                assert_eq!(api_version, 1);
            }
            other => panic!("expected hello, got {other:?}"),
        }
        match plugin.recv_event(WAIT) {
            Ok(Message::Register {
                commands,
                menu_items,
            }) => {
                assert!(commands.contains(&"plugin.test-plugin.echo".to_string()));
                assert_eq!(menu_items.len(), 1);
                assert_eq!(menu_items[0].location, "menu/view");
            }
            other => panic!("expected register, got {other:?}"),
        }
        assert!(plugin.is_alive());
    }

    #[test]
    fn a_command_returns_its_result() {
        let scratch = Scratch::new("echo");
        let plugin = start(&scratch, "");
        let args = Value::Map(vec![(Value::from("greeting"), Value::from("hi"))]);
        let rx = plugin.invoke("plugin.test-plugin.echo", Value::Nil, args.clone());
        assert_eq!(rx.recv_timeout(WAIT).expect("a result"), Ok(Some(args)));
    }

    #[test]
    fn several_commands_are_matched_to_their_own_results() {
        let scratch = Scratch::new("many");
        let plugin = start(&scratch, "");
        let receivers: Vec<_> = (0..5)
            .map(|i| plugin.invoke("plugin.test-plugin.echo", Value::Nil, Value::from(i)))
            .collect();
        for (i, rx) in receivers.into_iter().enumerate() {
            assert_eq!(
                rx.recv_timeout(WAIT).expect("a result"),
                Ok(Some(Value::from(i as i64)))
            );
        }
    }

    #[test]
    fn a_failing_command_returns_its_reason() {
        let scratch = Scratch::new("fail");
        let plugin = start(&scratch, "");
        let rx = plugin.invoke("plugin.test-plugin.fail", Value::Nil, Value::Nil);
        assert_eq!(
            rx.recv_timeout(WAIT).expect("a result"),
            Err("boom".to_string())
        );
        // 失敗のあとも、そのプロセスは使える（失敗の扱いは呼ぶ側が決める）
        let rx = plugin.invoke("plugin.test-plugin.echo", Value::Nil, Value::from(1));
        assert_eq!(rx.recv_timeout(WAIT).unwrap(), Ok(Some(Value::from(1))));
    }

    #[test]
    fn log_messages_and_stderr_go_to_plugin_log() {
        let scratch = Scratch::new("log");
        let plugin = start(&scratch, "stderr");
        let rx = plugin.invoke("plugin.test-plugin.log", Value::Nil, Value::Nil);
        rx.recv_timeout(WAIT).expect("a result").expect("ok");
        // `log` のメッセージは、結果より先に書かれる
        assert!(
            scratch.log_text().contains("test-plugin: a log line"),
            "{}",
            scratch.log_text()
        );
        // 標準エラーは別のスレッドが書くので、少し待つ
        assert!(
            wait_until(|| scratch.log_text().contains("test-plugin: hello on stderr")),
            "{}",
            scratch.log_text()
        );
    }

    #[test]
    fn a_plugin_that_dies_fails_the_call_and_is_no_longer_alive() {
        let scratch = Scratch::new("crash");
        let plugin = start(&scratch, "");
        let rx = plugin.invoke("plugin.test-plugin.exit", Value::Nil, Value::Nil);
        let reason = rx
            .recv_timeout(WAIT)
            .expect("an answer")
            .expect_err("the call fails");
        assert!(reason.contains("exited"), "{reason}");
        assert!(wait_until(|| !plugin.is_alive()));
        // 終わったあとの呼び出しは、すぐ失敗になる
        let again = plugin.invoke("plugin.test-plugin.echo", Value::Nil, Value::Nil);
        assert!(again.recv_timeout(WAIT).unwrap().is_err());
    }

    #[test]
    fn a_plugin_that_never_registers_times_out() {
        let scratch = Scratch::new("silent");
        let plugin = start(&scratch, "silent");
        assert!(matches!(plugin.recv_event(WAIT), Ok(Message::Hello { .. })));
        assert_eq!(
            plugin.recv_event(Duration::from_millis(300)),
            Err(EventError::Timeout)
        );
    }

    #[test]
    fn stop_closes_stdin_and_a_polite_plugin_ends_at_once() {
        let scratch = Scratch::new("polite");
        let plugin = start(&scratch, "");
        plugin.recv_event(WAIT).expect("hello");
        let started = Instant::now();
        plugin.stop(WAIT);
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn stop_kills_a_plugin_that_ignores_the_closed_stdin() {
        let scratch = Scratch::new("stubborn");
        let plugin = start(&scratch, "stubborn");
        plugin.recv_event(WAIT).expect("hello");
        let started = Instant::now();
        plugin.stop(Duration::from_millis(200));
        // 終わらないプラグインがあっても、止める側は待ち続けない
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_stopped_plugin_refuses_new_calls() {
        let scratch = Scratch::new("stopped");
        let plugin = start(&scratch, "");
        plugin.shutdown(Duration::from_millis(500));
        let rx = plugin.invoke("plugin.test-plugin.echo", Value::Nil, Value::Nil);
        assert!(rx.recv_timeout(WAIT).unwrap().is_err());
    }

    #[test]
    fn a_missing_program_or_an_empty_command_is_an_error() {
        let scratch = Scratch::new("missing");
        let missing = ChildPlugin::spawn(
            "x",
            &["yui-no-such-program-xyz".to_string()],
            &scratch.0,
            &scratch.log(),
        );
        assert!(missing.err().expect("fails").contains("cannot start"));

        let empty = ChildPlugin::spawn("x", &[], &scratch.0, &scratch.log());
        assert!(empty.err().expect("fails").contains("empty"));
    }

    #[test]
    fn a_relative_program_with_a_directory_is_resolved_from_the_plugin_folder() {
        let dir = Path::new("/plugins/sample");
        assert_eq!(resolve_program(dir, "python3"), Path::new("python3"));
        assert_eq!(
            resolve_program(dir, "/usr/bin/env"),
            Path::new("/usr/bin/env")
        );
        assert_eq!(
            resolve_program(dir, "bin/run"),
            Path::new("/plugins/sample/bin/run")
        );
        assert_eq!(
            resolve_program(dir, "./run"),
            Path::new("/plugins/sample/./run")
        );
    }
}
