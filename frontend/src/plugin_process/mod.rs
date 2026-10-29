//! 子プロセスとして動くプラグイン（architecture.md §6.4）。
//!
//! `protocol` は YUI とプラグインの間のメッセージ（msgpack、長さ付き）、`child` は子プロセスの起動・
//! 通信・終了である。プラグインの読み込み（`plugins.rs`）は、次の段階でここに切り替える。
#![allow(dead_code)] // `plugins.rs` が使い始めるまでは、テストからだけ使われる

pub mod child;
pub mod protocol;
