//! 組み込みの CPython を、専用スレッドで動かす（Cargo feature `python`、architecture.md §6.4）。
//!
//! UI スレッドは Python に触れない。依頼はチャネルで送り、結果は戻りのチャネルで受ける。
//! 初期化に失敗したときは `start` が `Err` を返すだけで、呼び出し側はプラグインを無効にして続ける。

use std::ffi::{CStr, CString};
use std::panic::{self, AssertUnwindSafe};
use std::sync::OnceLock;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use pyo3::ffi;
use pyo3::prelude::*;

/// 評価の結果。成功は値の `str`、失敗は例外の文字列。
pub type EvalResult = Result<String, String>;

/// 終了のときに、Python のスレッドを待つ時間。過ぎたら切り離して終える（architecture.md §6.4）。
const SHUTDOWN_WAIT: Duration = Duration::from_secs(2);

enum Request {
    Eval(String, Sender<EvalResult>),
    Quit,
}

pub struct PythonRuntime {
    requests: Sender<Request>,
    // Python のスレッドが戻ると切断される（送るものはない）
    finished: Receiver<()>,
    thread: Option<JoinHandle<()>>,
}

impl PythonRuntime {
    /// Python の専用スレッドを起動し、初期化が終わるまで待つ。
    pub fn start() -> Result<Self, String> {
        let (requests, inbox) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        let (finished_tx, finished) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("python".into())
            .spawn(move || run(inbox, ready_tx, finished_tx))
            .map_err(|e| format!("cannot start the Python thread: {e}"))?;
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                requests,
                finished,
                thread: Some(thread),
            }),
            Ok(Err(message)) => {
                let _ = thread.join();
                Err(message)
            }
            Err(_) => {
                let _ = thread.join();
                Err("the Python thread stopped before it was ready".into())
            }
        }
    }

    /// コードの評価を依頼する。UI は待たず、結果は返したチャネルで受け取る。
    pub fn eval(&self, code: &str) -> Receiver<EvalResult> {
        let (reply, result) = mpsc::channel();
        if self
            .requests
            .send(Request::Eval(code.to_string(), reply.clone()))
            .is_err()
        {
            let _ = reply.send(Err("the Python thread has stopped".into()));
        }
        result
    }
}

impl Drop for PythonRuntime {
    // 終了を頼んだあと、SHUTDOWN_WAIT だけ待つ。終わらなければ（無限ループなど）スレッドを切り離して戻る。
    // プロセスの終了では Py_Finalize を呼ばないので、残ったスレッドが終了を止めることはない。
    fn drop(&mut self) {
        let _ = self.requests.send(Request::Quit);
        match self.finished.recv_timeout(SHUTDOWN_WAIT) {
            // JoinHandle はこの後の片付けで捨てられ、スレッドは切り離される
            Err(RecvTimeoutError::Timeout) => {}
            _ => {
                if let Some(thread) = self.thread.take() {
                    let _ = thread.join();
                }
            }
        }
    }
}

// `finished` は、この関数が戻るときに閉じる（Python のスレッドが終わったことを知らせる）
fn run(inbox: Receiver<Request>, ready: Sender<Result<(), String>>, finished: Sender<()>) {
    let _finished = finished;
    if let Err(message) = initialize() {
        let _ = ready.send(Err(message));
        return;
    }
    let _ = ready.send(Ok(()));
    for request in inbox {
        match request {
            Request::Eval(code, reply) => {
                let _ = reply.send(eval_one(&code));
            }
            Request::Quit => break,
        }
    }
}

// `Py_InitializeFromConfig` を使い、失敗を戻り値で受ける。`Py_InitializeEx` は失敗すると
// プロセスを終了させる（`Fatal Python error`）ため、使わない。成功したら GIL を手放す
// （PyO3 の `Python::initialize` と同じ。手放さないと、他の Python スレッドが動けない）。
// インタープリターはプロセスに 1 つだけ。2 回目の初期化は、既に動いている Python を壊す。
static INTERPRETER: OnceLock<Result<(), String>> = OnceLock::new();

fn initialize() -> Result<(), String> {
    INTERPRETER.get_or_init(initialize_interpreter).clone()
}

fn initialize_interpreter() -> Result<(), String> {
    unsafe {
        let mut config: ffi::PyConfig = std::mem::zeroed();
        ffi::PyConfig_InitPythonConfig(&mut config);
        // シグナルの処理は Python に任せない（PyO3 の既定と同じ）
        config.install_signal_handlers = 0;
        let status = ffi::Py_InitializeFromConfig(&config);
        ffi::PyConfig_Clear(&mut config);
        if ffi::PyStatus_Exception(status) != 0 {
            let detail = if status.err_msg.is_null() {
                String::from("no detail")
            } else {
                CStr::from_ptr(status.err_msg)
                    .to_string_lossy()
                    .into_owned()
            };
            return Err(format!(
                "the embedded Python could not be initialized: {detail}"
            ));
        }
        ffi::PyEval_SaveThread();
    }
    Ok(())
}

fn eval_one(code: &str) -> EvalResult {
    let code = CString::new(code).map_err(|e| e.to_string())?;
    let outcome = panic::catch_unwind(AssertUnwindSafe(|| {
        Python::attach(|py| {
            let value = py.eval(&code, None, None).map_err(|e| format!("{e}"))?;
            value
                .str()
                .map(|s| s.to_string())
                .map_err(|e| format!("{e}"))
        })
    }));
    outcome.unwrap_or_else(|_| Err("the Python call panicked".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_expressions_and_reports_exceptions() {
        let runtime = PythonRuntime::start().expect("start the embedded Python");
        assert_eq!(runtime.eval("1 + 2").recv().unwrap(), Ok("3".to_string()));
        let error = runtime.eval("1 / 0").recv().unwrap().unwrap_err();
        assert!(error.contains("ZeroDivisionError"), "{error}");
    }

    #[test]
    fn stops_cleanly_with_a_background_thread_alive() {
        let runtime = PythonRuntime::start().expect("start the embedded Python");
        let started = runtime
            .eval(
                "__import__('threading').Thread(target=lambda: __import__('time').sleep(60), daemon=True).start() or 'ok'",
            )
            .recv()
            .unwrap();
        assert_eq!(started, Ok("ok".to_string()));
        drop(runtime);
    }

    #[test]
    fn gives_up_waiting_for_a_call_that_is_still_running() {
        let runtime = PythonRuntime::start().expect("start the embedded Python");
        // 処理が 3 秒かかる呼び出しを送り、結果は受け取らずに終える（SHUTDOWN_WAIT より長い）
        let _pending = runtime.eval("__import__('time').sleep(3) or 'slept'");
        let started = std::time::Instant::now();
        drop(runtime);
        let waited = started.elapsed();
        assert!(waited >= SHUTDOWN_WAIT, "returned too early: {waited:?}");
        assert!(
            waited < SHUTDOWN_WAIT + Duration::from_secs(1),
            "waited too long: {waited:?}"
        );
    }
}
