//! コマンドの識別子・引数と、コマンドの登録簿。
//!
//! コマンドは、独自の言語ではない。識別子（`file.new`、`viewport.show_as` など）は既存の操作に付けた名前で、
//! 引数はシリアライズできるデータ（JSON の値）である。登録簿は、識別子から、既存の操作を表す
//! アプリのメッセージ（`Message::MenuAction(..)` など）を引き当てる。文字列を解析して実行する仕組みはない。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::app::Message;

/// コマンドの呼び出し（識別子と引数）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandInvocation {
    pub id: String,
    #[serde(default)]
    pub args: serde_json::Value,
}

impl CommandInvocation {
    /// 引数のあるコマンド
    pub fn new(id: &str, args: serde_json::Value) -> Self {
        Self {
            id: id.to_string(),
            args,
        }
    }

    /// 引数のないコマンド
    pub fn plain(id: &str) -> Self {
        Self::new(id, serde_json::Value::Null)
    }
}

/// コマンドを実行できなかった理由（どちらも、アプリを止めない）
#[derive(Debug, Clone, PartialEq)]
pub enum CommandError {
    /// 登録されていない識別子
    Unknown(String),
    /// 引数が正しくない
    BadArgs { id: String, reason: String },
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CommandError::Unknown(id) => write!(f, "unknown command '{id}'"),
            CommandError::BadArgs { id, reason } => {
                write!(f, "invalid arguments for command '{id}': {reason}")
            }
        }
    }
}

type Handler = Box<dyn Fn(&serde_json::Value) -> Result<Message, String>>;

/// コマンドの登録簿。識別子と引数から、既存の操作（アプリのメッセージ）を引き当てる。
#[derive(Default)]
pub struct CommandRegistry {
    handlers: BTreeMap<String, Handler>,
}

impl CommandRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// コマンドを登録する。同じ識別子がすでにあれば、置き換えて `true` を返す。
    pub fn register(
        &mut self,
        id: &str,
        handler: impl Fn(&serde_json::Value) -> Result<Message, String> + 'static,
    ) -> bool {
        self.handlers
            .insert(id.to_string(), Box::new(handler))
            .is_some()
    }

    /// 引数のないコマンドを、決まったメッセージに橋渡しして登録する。
    pub fn register_plain(&mut self, id: &str, message: impl Fn() -> Message + 'static) {
        self.register(id, move |_| Ok(message()));
    }

    #[allow(dead_code)] // プラグインが登録済みか調べる受け口（テストで使う）
    pub fn contains(&self, id: &str) -> bool {
        self.handlers.contains_key(id)
    }

    /// 登録されている識別子（並び順）
    #[allow(dead_code)] // 一覧表示などの受け口（テストで使う）
    pub fn ids(&self) -> Vec<&str> {
        self.handlers.keys().map(String::as_str).collect()
    }

    /// 呼び出しを、実行するメッセージに引き当てる。未登録や引数の誤りは、エラーを返す（パニックしない）。
    pub fn resolve(&self, invocation: &CommandInvocation) -> Result<Message, CommandError> {
        let handler = self
            .handlers
            .get(&invocation.id)
            .ok_or_else(|| CommandError::Unknown(invocation.id.clone()))?;
        handler(&invocation.args).map_err(|reason| CommandError::BadArgs {
            id: invocation.id.clone(),
            reason,
        })
    }
}

/// 引数（JSON の値）を型に読み込む
pub(super) fn parse_args<T: serde::de::DeserializeOwned>(
    args: &serde_json::Value,
) -> Result<T, String> {
    serde_json::from_value(args.clone()).map_err(|e| e.to_string())
}
