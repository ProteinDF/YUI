//! 都度呼び出し（`mode: oneshot`）のプラグイン（architecture.md §6.4）。
//!
//! コマンドを呼ぶたびに、新しい一時フォルダに入力ファイルを書き、`plugin.yaml` のコマンドを、入力と出力の
//! ファイルのパスを渡して起動し、終わったら出力ファイルを読む。プロセスは呼び出しごとに終わるので、
//! 起動の宣言（`register`）も、メッセージのやり取りもない。標準出力と標準エラーは `plugin.log` に書く。

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use rmpv::Value;
use serde::{Deserialize, Serialize};

use super::child::{CommandOutcome, resolve_program};
use super::protocol::PROTOCOL_VERSION;
use crate::plugins::append_log;

/// コマンドの中の、入力ファイルのパスに置き換える印
pub const INPUT_PLACEHOLDER: &str = "{input}";
/// コマンドの中の、出力ファイルのパスに置き換える印
pub const OUTPUT_PLACEHOLDER: &str = "{output}";

/// 入力ファイルの名前（一時フォルダの中）
const INPUT_FILE: &str = "input.msgpack";
/// 出力ファイルの名前（一時フォルダの中）
const OUTPUT_FILE: &str = "output.msgpack";
/// 標準出力・標準エラーの読み取りが終わるのを、プロセスが終わったあとに待つ時間
const OUTPUT_DRAIN: Duration = Duration::from_millis(1000);

/// 入力ファイル（1 つの msgpack の連想配列）
#[derive(Serialize)]
struct InputFile<'a> {
    v: u64,
    command: &'a str,
    target: &'a Value,
    args: &'a Value,
}

/// 出力ファイル（1 つの msgpack の連想配列）
#[derive(Deserialize)]
struct OutputFile {
    v: u64,
    ok: bool,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    result: Option<Value>,
}

/// 動いている呼び出しのプロセス（プラグインの名前つき。YUI を終えるときに止める）
static RUNNING: Mutex<Vec<(String, Arc<Mutex<Child>>)>> = Mutex::new(Vec::new());
/// 一時フォルダの名前を分けるための通し番号
static COUNTER: AtomicU64 = AtomicU64::new(0);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// 呼ぶたびにプロセスを起動するプラグイン
pub struct OneshotPlugin {
    name: String,
    folder: PathBuf,
    /// 起動するコマンド（`{input}` と `{output}` を含む）
    template: Vec<String>,
    log: PathBuf,
}

/// `command` に、入力と出力の印が両方あるか
pub fn has_placeholders(template: &[String]) -> bool {
    let has = |mark: &str| template.iter().any(|arg| arg.contains(mark));
    has(INPUT_PLACEHOLDER) && has(OUTPUT_PLACEHOLDER)
}

impl OneshotPlugin {
    pub fn new(name: &str, folder: &Path, template: Vec<String>, log: &Path) -> Self {
        Self {
            name: name.to_string(),
            folder: folder.to_path_buf(),
            template,
            log: log.to_path_buf(),
        }
    }

    /// コマンドを 1 回実行する。結果は、返した受け口に 1 回だけ届く（待つのは別のスレッド）。
    pub fn call(&self, command: &str, target: Value, args: Value) -> Receiver<CommandOutcome> {
        let (tx, rx) = mpsc::channel();
        let (name, folder, template, log) = (
            self.name.clone(),
            self.folder.clone(),
            self.template.clone(),
            self.log.clone(),
        );
        let command = command.to_string();
        thread::spawn(move || {
            let outcome = run_once(&name, &folder, &template, &log, &command, &target, &args);
            let _ = tx.send(outcome);
        });
        rx
    }
}

/// 動いている呼び出しのプロセスをすべて止める（アプリを終えるとき）
pub fn kill_all() {
    kill_where(|_| true);
}

/// 名前が `matches` に合うプラグインの、動いている呼び出しのプロセスを止める
fn kill_where(matches: impl Fn(&str) -> bool) {
    let mut running = lock(&RUNNING);
    running.retain(|(name, child)| {
        if matches(name) {
            let _ = lock(child).kill();
            false
        } else {
            true
        }
    });
}

/// 一時フォルダ（終わりに消す）
struct TempDir(PathBuf);

impl TempDir {
    fn create(name: &str) -> std::io::Result<Self> {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("yui-plugin-{name}-{}-{n}", std::process::id()));
        fs::create_dir_all(&dir)?;
        Ok(TempDir(dir))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn run_once(
    name: &str,
    folder: &Path,
    template: &[String],
    log: &Path,
    command: &str,
    target: &Value,
    args: &Value,
) -> CommandOutcome {
    let temp =
        TempDir::create(name).map_err(|e| format!("cannot create a temporary folder ({e})"))?;
    let input = temp.0.join(INPUT_FILE);
    let output = temp.0.join(OUTPUT_FILE);

    let body = rmp_serde::to_vec_named(&InputFile {
        v: PROTOCOL_VERSION,
        command,
        target,
        args,
    })
    .map_err(|e| format!("cannot write the input file ({e})"))?;
    fs::write(&input, body).map_err(|e| format!("cannot write the input file ({e})"))?;

    let argv: Vec<String> = template
        .iter()
        .map(|arg| {
            arg.replace(INPUT_PLACEHOLDER, &input.to_string_lossy())
                .replace(OUTPUT_PLACEHOLDER, &output.to_string_lossy())
        })
        .collect();
    let program = argv
        .first()
        .ok_or_else(|| "the command is empty".to_string())?;
    let mut child = Command::new(resolve_program(folder, program))
        .args(&argv[1..])
        .current_dir(folder)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot start '{program}' ({e})"))?;
    let stdout = child.stdout.take().expect("stdout is piped");
    let stderr = child.stderr.take().expect("stderr is piped");

    // 標準出力と標準エラーは、ログに書く。標準エラーの最後の行は、失敗の理由に使う。
    let last_stderr = Arc::new(Mutex::new(None::<String>));
    let readers_done = Arc::new(AtomicU64::new(0));
    for (stream_is_stderr, reader) in [
        (false, Box::new(stdout) as Box<dyn std::io::Read + Send>),
        (true, Box::new(stderr) as Box<dyn std::io::Read + Send>),
    ] {
        let (name, log) = (name.to_string(), log.to_path_buf());
        let (last, done) = (last_stderr.clone(), readers_done.clone());
        thread::spawn(move || {
            for line in BufReader::new(reader).lines() {
                let Ok(line) = line else { break };
                append_log(&log, &format!("{name}: {line}"));
                if stream_is_stderr {
                    *lock(&last) = Some(line);
                }
            }
            done.fetch_add(1, Ordering::SeqCst);
        });
    }

    let child = Arc::new(Mutex::new(child));
    lock(&RUNNING).push((name.to_string(), child.clone()));
    let status = loop {
        match lock(&child).try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => {}
            Err(e) => break Err(e),
        }
        thread::sleep(Duration::from_millis(10));
    };
    lock(&RUNNING).retain(|(_, running)| !Arc::ptr_eq(running, &child));
    let status = status.map_err(|e| format!("cannot wait for the command ({e})"))?;

    // 読み取りのスレッドが、残りの行を書き終えるのを少し待つ（失敗の理由に、最後の行を使うため）
    let deadline = Instant::now() + OUTPUT_DRAIN;
    while readers_done.load(Ordering::SeqCst) < 2 && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    let last_line = lock(&last_stderr).clone();
    let reason_suffix = last_line.map(|l| format!(": {l}")).unwrap_or_default();

    let written = fs::read(&output).ok();
    match written {
        Some(bytes) => {
            let file: OutputFile = rmp_serde::from_slice(&bytes)
                .map_err(|e| format!("cannot read the output file ({e})"))?;
            if file.v != PROTOCOL_VERSION {
                return Err(format!(
                    "the output file has protocol version {}, not {PROTOCOL_VERSION}",
                    file.v
                ));
            }
            if !status.success() {
                return Err(format!("the command exited with {status}{reason_suffix}"));
            }
            if file.ok {
                Ok(file.result)
            } else {
                Err(file
                    .error
                    .unwrap_or_else(|| "the command failed".to_string()))
            }
        }
        None if status.success() => Ok(None),
        None => Err(format!("the command exited with {status}{reason_suffix}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin_process::child::test_plugin_path;

    const WAIT: Duration = Duration::from_secs(10);

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let dir = std::env::temp_dir()
                .join(format!("yui-oneshot-{tag}-{}-{nanos}", std::process::id()));
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

    /// テスト用のプラグイン（`examples/yui_test_plugin.rs`）を `oneshot` で起動する
    fn plugin(scratch: &Scratch, name: &str) -> OneshotPlugin {
        OneshotPlugin::new(
            name,
            &scratch.0,
            vec![
                test_plugin_path(),
                "oneshot".into(),
                INPUT_PLACEHOLDER.into(),
                OUTPUT_PLACEHOLDER.into(),
            ],
            &scratch.log(),
        )
    }

    fn temp_dirs_of(name: &str) -> usize {
        let prefix = format!("yui-plugin-{name}-");
        fs::read_dir(std::env::temp_dir())
            .map(|entries| {
                entries
                    .flatten()
                    .filter(|e| e.file_name().to_string_lossy().starts_with(&prefix))
                    .count()
            })
            .unwrap_or(0)
    }

    #[test]
    fn the_placeholders_must_both_be_in_the_command() {
        let args = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(has_placeholders(&args(&["run", "{input}", "{output}"])));
        assert!(has_placeholders(&args(&[
            "run",
            "--in={input}",
            "--out={output}"
        ])));
        assert!(!has_placeholders(&args(&["run", "{input}"])));
        assert!(!has_placeholders(&args(&["run", "{output}"])));
        assert!(!has_placeholders(&args(&["run"])));
    }

    #[test]
    fn a_command_returns_its_result_through_the_files() {
        let scratch = Scratch::new("echo");
        let plugin = plugin(&scratch, "oneshot-echo");
        let args = Value::Map(vec![(Value::from("n"), Value::from(3))]);
        let target = Value::Map(vec![(Value::from("kind"), Value::from("atom"))]);
        let rx = plugin.call("plugin.test-plugin.echo", target, args.clone());
        assert_eq!(rx.recv_timeout(WAIT).expect("an answer"), Ok(Some(args)));
    }

    #[test]
    fn a_failing_command_returns_its_reason_and_the_next_call_still_works() {
        let scratch = Scratch::new("fail");
        let plugin = plugin(&scratch, "oneshot-fail");
        let rx = plugin.call("plugin.test-plugin.fail", Value::Nil, Value::Nil);
        assert_eq!(
            rx.recv_timeout(WAIT).expect("an answer"),
            Err("boom".to_string())
        );
        let rx = plugin.call("plugin.test-plugin.echo", Value::Nil, Value::from(1));
        assert_eq!(rx.recv_timeout(WAIT).unwrap(), Ok(Some(Value::from(1))));
    }

    #[test]
    fn a_process_that_exits_without_an_output_file_is_a_failure_with_its_last_stderr_line() {
        let scratch = Scratch::new("exit");
        let plugin = plugin(&scratch, "oneshot-exit");
        let rx = plugin.call("plugin.test-plugin.exit", Value::Nil, Value::Nil);
        let reason = rx.recv_timeout(WAIT).unwrap().unwrap_err();
        assert!(reason.contains("exited with"), "{reason}");
        assert!(reason.contains("goodbye from exit"), "{reason}");
    }

    #[test]
    fn standard_output_and_error_go_to_the_log() {
        let scratch = Scratch::new("log");
        let plugin = plugin(&scratch, "oneshot-log");
        let rx = plugin.call("plugin.test-plugin.log", Value::Nil, Value::Nil);
        assert_eq!(rx.recv_timeout(WAIT).expect("an answer"), Ok(None));
        let log = scratch.log_text();
        assert!(log.contains("oneshot-log: printed to stdout"), "{log}");
        assert!(log.contains("oneshot-log: written to stderr"), "{log}");
    }

    #[test]
    fn the_temporary_folder_is_removed_after_each_call() {
        let scratch = Scratch::new("cleanup");
        let name = "oneshot-cleanup";
        let plugin = plugin(&scratch, name);
        for _ in 0..3 {
            let rx = plugin.call("plugin.test-plugin.echo", Value::Nil, Value::Nil);
            rx.recv_timeout(WAIT).unwrap().unwrap();
        }
        // 失敗した呼び出しでも消える
        let rx = plugin.call("plugin.test-plugin.fail", Value::Nil, Value::Nil);
        rx.recv_timeout(WAIT).unwrap().unwrap_err();
        assert_eq!(temp_dirs_of(name), 0);
    }

    #[test]
    fn calls_at_the_same_time_do_not_mix_their_files() {
        let scratch = Scratch::new("parallel");
        let plugin = plugin(&scratch, "oneshot-parallel");
        let receivers: Vec<_> = (0..6)
            .map(|i| plugin.call("plugin.test-plugin.echo", Value::Nil, Value::from(i)))
            .collect();
        for (i, rx) in receivers.into_iter().enumerate() {
            assert_eq!(
                rx.recv_timeout(WAIT).expect("an answer"),
                Ok(Some(Value::from(i as i64)))
            );
        }
    }

    #[test]
    fn a_program_that_cannot_start_is_a_failure() {
        let scratch = Scratch::new("nobin");
        let plugin = OneshotPlugin::new(
            "oneshot-nobin",
            &scratch.0,
            vec![
                "yui-no-such-program-xyz".into(),
                INPUT_PLACEHOLDER.into(),
                OUTPUT_PLACEHOLDER.into(),
            ],
            &scratch.log(),
        );
        let rx = plugin.call("x", Value::Nil, Value::Nil);
        let reason = rx.recv_timeout(WAIT).unwrap().unwrap_err();
        assert!(reason.contains("cannot start"), "{reason}");
    }

    #[test]
    fn a_running_call_is_stopped_by_kill_all() {
        let scratch = Scratch::new("kill");
        let plugin = plugin(&scratch, "oneshot-kill");
        let rx = plugin.call("plugin.test-plugin.hang", Value::Nil, Value::Nil);
        // 起動して、動いている呼び出しに登録されるのを待つ（ほかのテストの呼び出しは止めない）
        let started = || {
            lock(&RUNNING)
                .iter()
                .any(|(name, _)| name == "oneshot-kill")
        };
        let deadline = Instant::now() + WAIT;
        while !started() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(started(), "the call never started");
        kill_where(|name| name == "oneshot-kill");
        let reason = rx.recv_timeout(WAIT).expect("an answer").unwrap_err();
        assert!(reason.contains("exited with"), "{reason}");
    }
}
