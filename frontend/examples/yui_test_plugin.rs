//! テスト専用: プラグインのふりをする子プロセス（`plugin_process::child` のテストが起動する）。
//!
//! 引数でふるまいを選ぶ:
//! - なし / `stderr`: `hello` と `register` を送り、`invoke` に答える（`stderr` は標準エラーにも 1 行書く）
//! - `silent`: `hello` だけを送り、`register` は送らない
//! - `stubborn`: 標準入力が閉じても終わらない（強制終了のテスト用）
//!
//! - `oneshot <入力> <出力>`: 入力ファイルのコマンドを 1 回実行し、出力ファイルを書いて終わる
//!
//! `invoke` のコマンドは、名前の最後で選ぶ: `echo`（args をそのまま返す）、`fail`（理由 `boom`）、
//! `log`（ログを 1 行送る。`oneshot` では標準出力と標準エラーに書く）、`exit`（答えずに終わる）、
//! `hang`（`oneshot` で、終わらない）。
#![allow(dead_code)]

#[path = "../src/plugin_process/protocol.rs"]
mod protocol;

use protocol::{MenuItem, Message, read_message, write_message};
use rmpv::Value;
use std::io::{stdin, stdout};
use std::thread::sleep;
use std::time::Duration;

/// `oneshot`: 入力ファイルのコマンドを実行して、出力ファイルを書く
fn run_oneshot(input: &str, output: &str) {
    let bytes = std::fs::read(input).expect("read the input file");
    let value = rmpv::decode::read_value(&mut bytes.as_slice()).expect("msgpack");
    let get = |key: &str| {
        value
            .as_map()
            .and_then(|map| map.iter().find(|(k, _)| k.as_str() == Some(key)))
            .map(|(_, v)| v.clone())
            .unwrap_or(Value::Nil)
    };
    let command = get("command").as_str().unwrap_or("").to_string();
    let (ok, error, result) = match command.rsplit('.').next().unwrap_or("") {
        "echo" => (true, Value::Nil, get("args")),
        "fail" => (false, Value::from("boom"), Value::Nil),
        "log" => {
            println!("printed to stdout");
            eprintln!("written to stderr");
            (true, Value::Nil, Value::Nil)
        }
        "exit" => {
            eprintln!("goodbye from exit");
            std::process::exit(3);
        }
        "hang" => loop {
            sleep(Duration::from_secs(1));
        },
        other => (
            false,
            Value::from(format!("unknown command '{other}'")),
            Value::Nil,
        ),
    };
    let reply = Value::Map(vec![
        (Value::from("v"), Value::from(protocol::PROTOCOL_VERSION)),
        (Value::from("ok"), Value::Boolean(ok)),
        (Value::from("error"), error),
        (Value::from("result"), result),
    ]);
    let mut body = Vec::new();
    rmpv::encode::write_value(&mut body, &reply).expect("encode");
    std::fs::write(output, body).expect("write the output file");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).cloned().unwrap_or_default();
    if mode == "oneshot" {
        run_oneshot(&args[2], &args[3]);
        return;
    }
    let mut out = stdout().lock();
    let mut input = stdin().lock();

    if mode == "stderr" {
        eprintln!("hello on stderr");
    }
    write_message(
        &mut out,
        &Message::Hello {
            name: "test-plugin".into(),
            version: "1.0".into(),
            api_version: 1,
        },
    )
    .expect("hello");
    if mode != "silent" {
        write_message(
            &mut out,
            &Message::Register {
                commands: vec![
                    "plugin.test-plugin.echo".into(),
                    "plugin.test-plugin.fail".into(),
                ],
                menu_items: vec![MenuItem {
                    location: "menu/view".into(),
                    order: 500,
                    label: "Echo".into(),
                    command: "plugin.test-plugin.echo".into(),
                    args: Value::Nil,
                }],
            },
        )
        .expect("register");
    }
    if mode == "stubborn" {
        loop {
            sleep(Duration::from_secs(1));
        }
    }

    // 標準入力が閉じる（YUI が終わらせる）まで、依頼に答える
    while let Ok(Some(message)) = read_message(&mut input) {
        let Message::Invoke {
            id, command, args, ..
        } = message
        else {
            continue;
        };
        let reply = match command.rsplit('.').next().unwrap_or("") {
            "echo" => Message::Result {
                id,
                ok: true,
                error: None,
                result: Some(args),
            },
            "fail" => Message::Result {
                id,
                ok: false,
                error: Some("boom".into()),
                result: None,
            },
            "log" => {
                write_message(
                    &mut out,
                    &Message::Log {
                        text: "a log line".into(),
                    },
                )
                .expect("log");
                Message::Result {
                    id,
                    ok: true,
                    error: None,
                    result: None,
                }
            }
            "exit" => std::process::exit(3),
            other => Message::Result {
                id,
                ok: false,
                error: Some(format!("unknown command '{other}'")),
                result: None,
            },
        };
        write_message(&mut out, &reply).expect("result");
    }
}
