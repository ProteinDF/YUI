//! メニューモデルとコマンド（architecture.md 8.7節）。
//!
//! メニューの項目は、処理そのものではなく、コマンドの識別子と引数（`CommandInvocation`）を持つデータである。
//! 画面はメニューモデル（`MenuItem`）から組み立て、押された項目のコマンドは `CommandRegistry` が
//! 既存の操作（`MenuAction` / `ContextMenuAction` など）に引き当てる。
//! メニューの場所ごとの受け口（`MenuRegistry::contribute`）に項目を足せるので、将来プラグインが項目を足せる。

mod builtin;
pub mod command;
mod model;
pub mod placement;
mod render;

pub use builtin::{register_builtin_menus, register_menu_commands};
pub use command::{CommandInvocation, CommandRegistry};
pub use model::{MenuContext, MenuItem, MenuRegistry, MenuTarget, location};
pub use render::{Anchor, Surface, popup_menu};

/// 組み込みのメニューの項目を、指定した場所について組み立てる（テスト用の近道）
#[cfg(test)]
pub fn builtin_items(location: &str, target: Option<MenuTarget>) -> Vec<MenuItem> {
    let mut menus = MenuRegistry::new();
    register_builtin_menus(&mut menus);
    menus.build(
        location,
        &MenuContext {
            has_structure: true,
            target,
            ..MenuContext::default()
        },
    )
}
