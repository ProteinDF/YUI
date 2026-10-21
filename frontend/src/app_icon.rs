//! アプリのアイコン（デスクトップ版のウィンドウのアイコン）。`packaging/icons/icon-256.png` を埋め込んで、RGBA に復号する。
//! ブラウザ版のアイコンは `index.html` の favicon（Trunk が `icon-64.png` をコピーする）。

/// ウィンドウのアイコン。復号できなければ `None`（既定のアイコンのまま）。
#[cfg(not(target_arch = "wasm32"))]
pub fn window_icon() -> Option<iced::window::Icon> {
    const PNG: &[u8] = include_bytes!("../../packaging/icons/icon-128.png");
    let mut reader = png::Decoder::new(std::io::Cursor::new(PNG))
        .read_info()
        .ok()?;
    let mut buffer = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buffer).ok()?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return None;
    }
    buffer.truncate(info.buffer_size());
    iced::window::icon::from_rgba(buffer, info.width, info.height).ok()
}

/// ブラウザ版には、ウィンドウのアイコンがない（favicon を使う）。
#[cfg(target_arch = "wasm32")]
pub fn window_icon() -> Option<iced::window::Icon> {
    None
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn test_window_icon_decodes_the_bundled_png() {
        assert!(super::window_icon().is_some());
    }
}
