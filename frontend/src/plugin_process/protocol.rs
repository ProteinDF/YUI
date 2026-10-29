//! YUI とプラグインの間のメッセージ（architecture.md §6.4）。
//!
//! 各メッセージは msgpack の連想配列で、`type`（種別）と、種別ごとの項目を持つ。前に長さ（4 バイト、
//! big-endian）を置いて区切る。`v`（プロトコルの版）は、送るとき自動で足し、読むとき確かめて外す。
//!
//! このファイルは標準ライブラリと `serde` / `rmpv` だけを使う（テスト用のプラグイン
//! `examples/yui_test_plugin.rs` も、このファイルをそのまま取り込む）。

use std::fmt;
use std::io::{self, Read, Write};

use rmpv::Value;
use serde::{Deserialize, Serialize};

/// プロトコルの版。メッセージの `v`。`api_version`（Python の公開 API の版）とは別。
pub const PROTOCOL_VERSION: u64 = 1;
/// 1 つのメッセージの大きさの上限（壊れた長さで、メモリを使い切らないため）
pub const MAX_MESSAGE_BYTES: usize = 16 * 1024 * 1024;

/// 整数でなければ読めない `v`
#[derive(Deserialize)]
struct Version(u64);

fn nil() -> Value {
    Value::Nil
}

/// `register` で宣言するメニュー項目
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MenuItem {
    pub location: String,
    pub order: i64,
    pub label: String,
    pub command: String,
    #[serde(default = "nil")]
    pub args: Value,
}

/// プラグインと YUI の間を流れるメッセージ
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Message {
    /// プラグイン → YUI: 起動の最初に送る
    Hello {
        name: String,
        version: String,
        api_version: u64,
    },
    /// プラグイン → YUI: コマンドとメニュー項目の宣言
    Register {
        #[serde(default)]
        commands: Vec<String>,
        #[serde(default)]
        menu_items: Vec<MenuItem>,
    },
    /// YUI → プラグイン: コマンドの実行。`target` は右クリックの対象（なければ nil）
    Invoke {
        id: u64,
        command: String,
        #[serde(default = "nil")]
        target: Value,
        #[serde(default = "nil")]
        args: Value,
    },
    /// プラグイン → YUI: `Invoke` の結果。失敗のときは `ok` が偽で、理由が `error` に入る
    Result {
        id: u64,
        ok: bool,
        #[serde(default)]
        error: Option<String>,
        #[serde(default)]
        result: Option<Value>,
    },
    /// プラグイン → YUI: ログの 1 行
    Log { text: String },
}

/// メッセージの読み書きの失敗
#[derive(Debug)]
pub enum ProtocolError {
    Io(io::Error),
    /// 長さの途中、または本体の途中で、入力が終わった
    Truncated,
    /// 長さが上限を超えた
    TooLarge(usize),
    /// 対応していないプロトコルの版
    Version(u64),
    Encode(String),
    Decode(String),
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtocolError::Io(e) => write!(f, "I/O error ({e})"),
            ProtocolError::Truncated => write!(f, "the message ended early"),
            ProtocolError::TooLarge(n) => {
                write!(
                    f,
                    "a message of {n} bytes is larger than {MAX_MESSAGE_BYTES}"
                )
            }
            ProtocolError::Version(v) => write!(
                f,
                "protocol version {v} is not supported (this app speaks {PROTOCOL_VERSION})"
            ),
            ProtocolError::Encode(e) => write!(f, "cannot encode a message ({e})"),
            ProtocolError::Decode(e) => write!(f, "cannot decode a message ({e})"),
        }
    }
}

impl From<io::Error> for ProtocolError {
    fn from(e: io::Error) -> Self {
        if e.kind() == io::ErrorKind::UnexpectedEof {
            ProtocolError::Truncated
        } else {
            ProtocolError::Io(e)
        }
    }
}

/// 送るときの外側: `v` を足して、メッセージの項目を同じ連想配列に並べる
#[derive(Serialize)]
struct OutFrame<'a> {
    v: u64,
    #[serde(flatten)]
    message: &'a Message,
}

/// 読むときの外側
#[derive(Deserialize)]
struct InFrame {
    #[serde(flatten)]
    message: Message,
}

/// 版だけを先に読む（種別が未知でも、版の違いを知らせるため）
#[derive(Deserialize)]
struct VersionOnly {
    v: Option<Version>,
}

/// メッセージを msgpack にする（長さは付けない）。`v` を足す。構造体は連想配列で書く。
pub fn encode(message: &Message) -> Result<Vec<u8>, ProtocolError> {
    rmp_serde::to_vec_named(&OutFrame {
        v: PROTOCOL_VERSION,
        message,
    })
    .map_err(|e| ProtocolError::Encode(e.to_string()))
}

/// msgpack（長さなし）からメッセージを読む。`v` を確かめる。
pub fn decode(body: &[u8]) -> Result<Message, ProtocolError> {
    let only: VersionOnly =
        rmp_serde::from_slice(body).map_err(|e| ProtocolError::Decode(e.to_string()))?;
    match only.v {
        None => return Err(ProtocolError::Decode("the message has no 'v'".into())),
        Some(Version(PROTOCOL_VERSION)) => {}
        Some(Version(other)) => return Err(ProtocolError::Version(other)),
    }
    let frame: InFrame =
        rmp_serde::from_slice(body).map_err(|e| ProtocolError::Decode(e.to_string()))?;
    Ok(frame.message)
}

/// 長さ（4 バイト、big-endian）と本体を書き、流す
pub fn write_message<W: Write>(out: &mut W, message: &Message) -> Result<(), ProtocolError> {
    let body = encode(message)?;
    if body.len() > MAX_MESSAGE_BYTES {
        return Err(ProtocolError::TooLarge(body.len()));
    }
    out.write_all(&(body.len() as u32).to_be_bytes())?;
    out.write_all(&body)?;
    out.flush()?;
    Ok(())
}

/// 1 つのメッセージを読む。区切りの位置で入力が終わっていれば `Ok(None)`（相手が閉じた）。
pub fn read_message<R: Read>(input: &mut R) -> Result<Option<Message>, ProtocolError> {
    let mut length = [0u8; 4];
    // 最初の 1 バイトで終わっていれば、きれいに閉じたとみなす
    loop {
        match input.read(&mut length[..1]) {
            Ok(0) => return Ok(None),
            Ok(_) => break,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        }
    }
    input.read_exact(&mut length[1..])?;
    let size = u32::from_be_bytes(length) as usize;
    if size > MAX_MESSAGE_BYTES {
        return Err(ProtocolError::TooLarge(size));
    }
    let mut body = vec![0u8; size];
    input.read_exact(&mut body)?;
    decode(&body).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(message: Message) {
        let mut wire = Vec::new();
        write_message(&mut wire, &message).expect("write");
        let mut input = wire.as_slice();
        assert_eq!(read_message(&mut input).expect("read"), Some(message));
        assert_eq!(read_message(&mut input).expect("end"), None);
    }

    #[test]
    fn every_message_survives_a_round_trip() {
        round_trip(Message::Hello {
            name: "sample".into(),
            version: "0.1.0".into(),
            api_version: 1,
        });
        round_trip(Message::Register {
            commands: vec!["plugin.sample.go".into()],
            menu_items: vec![MenuItem {
                location: "menu/view".into(),
                order: 500,
                label: "Go".into(),
                command: "plugin.sample.go".into(),
                args: Value::Map(vec![(Value::from("greeting"), Value::from("hello"))]),
            }],
        });
        round_trip(Message::Invoke {
            id: 7,
            command: "plugin.sample.go".into(),
            target: Value::Map(vec![
                (Value::from("kind"), Value::from("atom")),
                (Value::from("atom_id"), Value::from(12)),
            ]),
            args: Value::Nil,
        });
        round_trip(Message::Result {
            id: 7,
            ok: true,
            error: None,
            result: Some(Value::from(vec![Value::from(1), Value::from(2)])),
        });
        round_trip(Message::Result {
            id: 8,
            ok: false,
            error: Some("boom".into()),
            result: None,
        });
        round_trip(Message::Log {
            text: "日本語も通る".into(),
        });
    }

    #[test]
    fn the_wire_format_is_a_length_then_a_msgpack_map() {
        let mut wire = Vec::new();
        write_message(&mut wire, &Message::Log { text: "hi".into() }).expect("write");
        // 長さ（4 バイト、big-endian）の後ろは、msgpack の連想配列
        let size = u32::from_be_bytes(wire[..4].try_into().unwrap()) as usize;
        assert_eq!(size, wire.len() - 4);
        let mut body = &wire[4..];
        let value = rmpv::decode::read_value(&mut body).expect("msgpack");
        let map = value.as_map().expect("a map");
        let get = |key: &str| {
            map.iter()
                .find(|(k, _)| k.as_str() == Some(key))
                .map(|(_, v)| v.clone())
        };
        assert_eq!(get("type"), Some(Value::from("log")));
        assert_eq!(get("text"), Some(Value::from("hi")));
        assert_eq!(get("v"), Some(Value::from(PROTOCOL_VERSION)));
    }

    #[test]
    fn several_messages_in_a_row_are_read_in_order() {
        let mut wire = Vec::new();
        for text in ["a", "b", "c"] {
            write_message(&mut wire, &Message::Log { text: text.into() }).unwrap();
        }
        let mut input = wire.as_slice();
        for text in ["a", "b", "c"] {
            assert_eq!(
                read_message(&mut input).unwrap(),
                Some(Message::Log { text: text.into() })
            );
        }
        assert_eq!(read_message(&mut input).unwrap(), None);
    }

    #[test]
    fn a_cut_message_is_an_error_not_a_clean_end() {
        let mut wire = Vec::new();
        write_message(
            &mut wire,
            &Message::Log {
                text: "hello".into(),
            },
        )
        .unwrap();
        // 長さの途中で切れた
        let mut short_length = &wire[..2];
        assert!(matches!(
            read_message(&mut short_length),
            Err(ProtocolError::Truncated)
        ));
        // 本体の途中で切れた
        let mut short_body = &wire[..wire.len() - 1];
        assert!(matches!(
            read_message(&mut short_body),
            Err(ProtocolError::Truncated)
        ));
    }

    #[test]
    fn an_oversized_length_is_refused_before_reading_it() {
        let wire = (MAX_MESSAGE_BYTES as u32 + 1).to_be_bytes();
        let mut input = wire.as_slice();
        assert!(matches!(
            read_message(&mut input),
            Err(ProtocolError::TooLarge(_))
        ));
    }

    #[test]
    fn a_wrong_protocol_version_is_refused() {
        let value = Value::Map(vec![
            (Value::from("type"), Value::from("log")),
            (Value::from("text"), Value::from("hi")),
            (Value::from("v"), Value::from(PROTOCOL_VERSION + 1)),
        ]);
        let mut body = Vec::new();
        rmpv::encode::write_value(&mut body, &value).unwrap();
        assert!(matches!(
            decode(&body),
            Err(ProtocolError::Version(v)) if v == PROTOCOL_VERSION + 1
        ));
    }

    #[test]
    fn a_bad_message_is_a_decode_error() {
        let encode_map = |entries: Vec<(Value, Value)>| {
            let mut body = Vec::new();
            rmpv::encode::write_value(&mut body, &Value::Map(entries)).unwrap();
            body
        };
        // 版がない
        let no_version = encode_map(vec![(Value::from("type"), Value::from("log"))]);
        assert!(matches!(decode(&no_version), Err(ProtocolError::Decode(_))));
        // 知らない種別
        let unknown = encode_map(vec![
            (Value::from("type"), Value::from("dance")),
            (Value::from("v"), Value::from(PROTOCOL_VERSION)),
        ]);
        assert!(matches!(decode(&unknown), Err(ProtocolError::Decode(_))));
        // 連想配列ではない
        let mut not_a_map = Vec::new();
        rmpv::encode::write_value(&mut not_a_map, &Value::from(3)).unwrap();
        assert!(matches!(decode(&not_a_map), Err(ProtocolError::Decode(_))));
        // 項目が足りない（`log` に `text` がない）
        let missing = encode_map(vec![
            (Value::from("type"), Value::from("log")),
            (Value::from("v"), Value::from(PROTOCOL_VERSION)),
        ]);
        assert!(matches!(decode(&missing), Err(ProtocolError::Decode(_))));
    }

    #[test]
    fn unknown_extra_fields_are_ignored() {
        // 後の版で項目が増えても、今の版は読める
        let value = Value::Map(vec![
            (Value::from("type"), Value::from("log")),
            (Value::from("text"), Value::from("hi")),
            (Value::from("level"), Value::from("info")),
            (Value::from("v"), Value::from(PROTOCOL_VERSION)),
        ]);
        let mut body = Vec::new();
        rmpv::encode::write_value(&mut body, &value).unwrap();
        assert_eq!(decode(&body).unwrap(), Message::Log { text: "hi".into() });
    }
}
