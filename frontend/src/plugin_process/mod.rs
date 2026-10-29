//! 子プロセスとして動くプラグイン（architecture.md §6.4）。
//!
//! `protocol` は YUI とプラグインの間のメッセージ（msgpack、長さ付き）、`child` は子プロセスの起動・
//! 通信・終了、`python` は Python のプラグインを動かすランナー、`convert` は値の変換である。
//! プラグインの読み込みと実行（`plugins.rs`）が使う。

pub mod child;
pub mod convert;
pub mod protocol;
pub mod python;
