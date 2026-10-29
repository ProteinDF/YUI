//! GPU を使うテストを、1 つずつ動かすためのロック（テストだけ）。
//!
//! テストは並列に動く。複数のスレッドが同時に wgpu のインスタンス（`iced_test` の描画器、画面なしの描画）を
//! 作ったり壊したりすると、Vulkan のローダー（`libvulkan.so.1`）の中で SIGSEGV になることがある。
//! GPU を使うテストは、この `lock()` を持っている間だけ動かす。

use std::sync::{Mutex, MutexGuard, PoisonError};

static GPU: Mutex<()> = Mutex::new(());

/// GPU を使う間、持っておく。ほかのテストが落ちて壊れたロックでも、そのまま使う。
pub fn lock() -> MutexGuard<'static, ()> {
    GPU.lock().unwrap_or_else(PoisonError::into_inner)
}
