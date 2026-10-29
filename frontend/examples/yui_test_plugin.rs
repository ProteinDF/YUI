//! テスト専用: プラグインのふりをする子プロセス（`plugin_process::child` のテストが起動する）。
//!
//! 引数でふるまいを選ぶ:
//! - なし / `stderr`: `hello` と `register` を送り、`invoke` に答える（`stderr` は標準エラーにも 1 行書く）
//! - `silent`: `hello` だけを送り、`register` は送らない
//! - `stubborn`: 標準入力が閉じても終わらない（強制終了のテスト用）
//!
//! `invoke` のコマンドは、名前の最後で選ぶ: `echo`（args をそのまま返す）、`fail`（理由 `boom`）、
//! `log`（ログを 1 行送る）、`exit`（答えずに終わる）。
#![allow(dead_code)]

#[path = "../src/plugin_process/protocol.rs"]
mod protocol;

use protocol::{MenuItem, Message, read_message, write_message};
use rmpv::Value;
use std::io::{stdin, stdout};
use std::thread::sleep;
use std::time::Duration;

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_default();
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
