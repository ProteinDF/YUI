//! 画面なしの描画（オフスクリーン）。描画の準備ができた `ViewportPrimitive` を、画面とは別の
//! wgpu のデバイスでテクスチャに描き、画素を読み戻して、PNG にする。
//!
//! PNG の書き出し（File → Export Image (PNG)...）と、画面なしの描画テスト（`headless.rs`）が使う。
//! 描画先は `Rgba8UnormSrgb`（画面と同じ sRGB の見た目）で、読み戻しの行は 256 バイトの倍数に
//! そろえて取り、戻すときに詰める。

use iced::Rectangle;
use iced::widget::shader::{Pipeline, Primitive, Viewport};

use super::pipeline::ViewportPipeline;
use super::primitive::ViewportPrimitive;

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// 画面とは別に使う GPU（ソフトウェアのアダプターでもよい）
pub struct OffscreenGpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    /// アダプターの名前とバックエンド（表示・ログ用）
    pub info: String,
}

/// アダプターとデバイスを用意する。なければ、理由つきのエラー。
pub async fn request_gpu() -> Result<OffscreenGpu, String> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
        })
        .await
        .map_err(|e| format!("No graphics adapter was found: {e}"))?;
    let info = adapter.get_info();
    // 大きな画像のために、アダプターの限界まで使えるようにする
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            required_limits: adapter.limits(),
            ..wgpu::DeviceDescriptor::default()
        })
        .await
        .map_err(|e| format!("The graphics device could not be created: {e}"))?;
    Ok(OffscreenGpu {
        device,
        queue,
        info: format!("{} ({:?})", info.name, info.backend),
    })
}

/// `width` x `height` のテクスチャに描いて、RGBA（行は詰めてある）を返す。
pub async fn render_rgba(
    gpu: &OffscreenGpu,
    primitive: &ViewportPrimitive,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, String> {
    let mut pipeline = ViewportPipeline::new(&gpu.device, &gpu.queue, FORMAT);
    render_rgba_with(gpu, &mut pipeline, primitive, width, height).await
}

/// GPU とパイプラインを使い回して、何枚も描く（動画の書き出し。ネイティブ版のみ）
pub struct OffscreenRenderer {
    pub gpu: OffscreenGpu,
    pipeline: ViewportPipeline,
}

impl OffscreenRenderer {
    pub async fn new() -> Result<Self, String> {
        let gpu = request_gpu().await?;
        let pipeline = ViewportPipeline::new(&gpu.device, &gpu.queue, FORMAT);
        Ok(Self { gpu, pipeline })
    }

    /// `width` x `height` に描いて、RGBA（行は詰めてある）を返す
    pub async fn render(
        &mut self,
        primitive: &ViewportPrimitive,
        width: u32,
        height: u32,
    ) -> Result<Vec<u8>, String> {
        render_rgba_with(&self.gpu, &mut self.pipeline, primitive, width, height).await
    }
}

async fn render_rgba_with(
    gpu: &OffscreenGpu,
    pipeline: &mut ViewportPipeline,
    primitive: &ViewportPrimitive,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, String> {
    let max = gpu.device.limits().max_texture_dimension_2d;
    if width == 0 || height == 0 || width > max || height > max {
        return Err(format!(
            "The image size {width} x {height} is not supported (the limit is {max} px)"
        ));
    }
    let bounds = Rectangle::new(
        iced::Point::ORIGIN,
        iced::Size::new(width as f32, height as f32),
    );
    let viewport = Viewport::with_physical_size(iced::Size::new(width, height), 1.0);
    primitive.prepare(pipeline, &gpu.device, &gpu.queue, &bounds, &viewport);

    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("offscreen target"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let row_bytes = width * 4;
    let padded_row = row_bytes.div_ceil(align) * align;
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("offscreen readback"),
        size: padded_row as u64 * height as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    // 描画先を先に塗りつぶす（画面では下地が既にあるが、ここでは何もないため）
    drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("offscreen clear"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                // 背景を描かないとき（背景を透明にする書き出し）は、透明に塗りつぶす
                load: wgpu::LoadOp::Clear(if primitive.draw_background {
                    wgpu::Color::BLACK
                } else {
                    wgpu::Color::TRANSPARENT
                }),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    }));
    primitive.render(
        pipeline,
        &mut encoder,
        &view,
        &Rectangle {
            x: 0,
            y: 0,
            width,
            height,
        },
    );
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    gpu.queue.submit(Some(encoder.finish()));

    let slice = buffer.slice(..);
    let (tx, rx) = iced::futures::channel::oneshot::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    // ネイティブ版は、読み戻しが終わるまでここで待つ。ブラウザ版は、ブラウザが進めるので、待つだけ。
    #[cfg(not(target_arch = "wasm32"))]
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|e| format!("The graphics device failed while drawing: {e}"))?;
    rx.await
        .map_err(|_| "The image readback was cancelled".to_string())?
        .map_err(|e| format!("The image could not be read back: {e}"))?;
    let mapped = slice.get_mapped_range();
    let mut out = Vec::with_capacity((row_bytes * height) as usize);
    for row in mapped.chunks_exact(padded_row as usize) {
        out.extend_from_slice(&row[..row_bytes as usize]);
    }
    drop(mapped);
    buffer.unmap();
    Ok(out)
}

/// sRGB の 8 ビットの値 → 線形の値
fn srgb_to_linear(v: u8) -> f32 {
    let c = v as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// 線形の値 → sRGB の 8 ビットの値
fn linear_to_srgb(l: f32) -> u8 {
    let l = l.clamp(0.0, 1.0);
    let c = if l <= 0.003_130_8 {
        l * 12.92
    } else {
        1.055 * l.powf(1.0 / 2.4) - 0.055
    };
    (c * 255.0).round() as u8
}

/// `factor` x `factor` の画素を平均して縮小する（アンチエイリアスのため、大きく描いてから縮める）。
///
/// 平均は、線形の光で取る（sRGB の符号化された値のまま平均すると、縁が暗くなる）。幅と高さが
/// `factor` で割り切れないときの端は切り捨てる。`premultiplied` が `true` のときは、背景を描かずに
/// 透明の下地に描いた画像（色が、透明度を掛けた形になっている）として、縮めたあとに、色を透明度で
/// 割って、通常の（掛けていない）透明度つきの画像にする。`factor` が 1 でも、この変換は行う。
pub fn downsample_box(
    rgba: &[u8],
    width: u32,
    height: u32,
    factor: u32,
    premultiplied: bool,
) -> (Vec<u8>, u32, u32) {
    let factor = factor.max(1);
    let (w, h) = (width / factor, height / factor);
    let lut: Vec<f32> = (0..=255u8).map(srgb_to_linear).collect();
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    let n = (factor * factor) as f32;
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0.0f32; 3];
            let mut alpha = 0.0f32;
            for dy in 0..factor {
                for dx in 0..factor {
                    let i = (((y * factor + dy) * width + x * factor + dx) * 4) as usize;
                    for c in 0..3 {
                        sum[c] += lut[rgba[i + c] as usize];
                    }
                    alpha += rgba[i + 3] as f32;
                }
            }
            let a = alpha / n / 255.0; // 0..1
            let scale = if premultiplied {
                if a > 0.0 { 1.0 / (n * a) } else { 0.0 }
            } else {
                1.0 / n
            };
            out.extend(sum.iter().map(|&s| linear_to_srgb(s * scale)));
            out.push((a * 255.0).round() as u8);
        }
    }
    (out, w, h)
}

/// RGBA の画素列を PNG にする
pub fn encode_png(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|e| format!("The PNG could not be written: {e}"))?;
        writer
            .write_image_data(rgba)
            .map_err(|e| format!("The PNG could not be written: {e}"))?;
    }
    Ok(out)
}

/// 描いて、縮めて、PNG にする。大きく描いて縮める倍率は `supersample`（アダプターの限界を超えるなら、
/// 1 倍に下げる）。戻りは、PNG のバイト列と、画像の大きさ。
pub async fn render_png(
    primitive: ViewportPrimitive,
    width: u32,
    height: u32,
    supersample: u32,
) -> Result<(Vec<u8>, u32, u32), String> {
    let transparent = !primitive.draw_background;
    #[cfg(not(target_arch = "wasm32"))]
    {
        // 読み戻しの待ちで止まるので、専用のスレッドで行う
        tokio::task::spawn_blocking(move || {
            iced::futures::executor::block_on(render_png_inner(
                &primitive,
                width,
                height,
                supersample,
                transparent,
            ))
        })
        .await
        .map_err(|e| format!("The image could not be rendered: {e}"))?
    }
    #[cfg(target_arch = "wasm32")]
    {
        render_png_inner(&primitive, width, height, supersample, transparent).await
    }
}

/// 大きく描く倍率。要求の倍率から、アダプターのテクスチャの一辺の限界と、画素数の上限
/// （`MAX_RENDER_PIXELS`。メモリを使いすぎないため）に収まるまで、1 ずつ下げる（最小 1）。
pub fn fit_supersample(width: u32, height: u32, requested: u32, max_dimension: u32) -> u32 {
    let mut factor = requested.max(1);
    while factor > 1
        && (width.max(height) as u64 * factor as u64 > max_dimension as u64
            || width as u64 * height as u64 * (factor as u64).pow(2) > MAX_RENDER_PIXELS)
    {
        factor -= 1;
    }
    factor
}

/// 一度に描く画素数の上限（RGBA 8 ビットで約 256 MB。深度と読み戻しにも同じ大きさが要る）
const MAX_RENDER_PIXELS: u64 = 64_000_000;

async fn render_png_inner(
    primitive: &ViewportPrimitive,
    width: u32,
    height: u32,
    supersample: u32,
    transparent: bool,
) -> Result<(Vec<u8>, u32, u32), String> {
    let gpu = request_gpu().await?;
    let max = gpu.device.limits().max_texture_dimension_2d;
    let factor = fit_supersample(width, height, supersample, max);
    let rgba = render_rgba(&gpu, primitive, width * factor, height * factor)
        .await
        .map_err(|e| format!("{e} (adapter: {})", gpu.info))?;
    let (small, w, h) = downsample_box(&rgba, width * factor, height * factor, factor, transparent);
    Ok((encode_png(&small, w, h)?, w, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_downsample_averages_in_linear_light_and_trims_the_edge() {
        // 黒と白（R だけ）の 2 つずつ: 線形で平均すると 0.5 → sRGB で 188（符号化された値の平均 128 ではない）
        let px = |v: u8| [v, 100, 200, 255];
        let mut rgba = Vec::new();
        for v in [0u8, 255, 255, 0] {
            rgba.extend(px(v));
        }
        let (out, w, h) = downsample_box(&rgba, 2, 2, 2, false);
        assert_eq!((w, h), (1, 1));
        assert_eq!(out, vec![188, 100, 200, 255]);
        // 倍率 1 は、値を変えない（往復で同じ）
        assert_eq!(downsample_box(&rgba, 2, 2, 1, false).0, rgba);
        // 3x3 を 2 倍で縮めると 1x1（端は切り捨て）
        let big = vec![10u8; 3 * 3 * 4];
        let (_, w, h) = downsample_box(&big, 3, 3, 2, false);
        assert_eq!((w, h), (1, 1));
    }

    #[test]
    fn test_transparent_background_pixels_are_unpremultiplied() {
        // 2x2: 不透明な赤（255,0,0,255）が 1 つと、透明（0,0,0,0）が 3 つ。縮めると、半分の被覆ではなく
        // 4 分の 1 の被覆の赤: 色は赤のまま、透明度が 64 になる
        let mut rgba = vec![0u8; 16];
        rgba[..4].copy_from_slice(&[255, 0, 0, 255]);
        let (out, _, _) = downsample_box(&rgba, 2, 2, 2, true);
        assert_eq!(out, vec![255, 0, 0, 64]);
        // 全部透明なら、色は 0（0 で割らない）
        let (out, _, _) = downsample_box(&[0u8; 16], 2, 2, 2, true);
        assert_eq!(out, vec![0, 0, 0, 0]);
        // 1 倍でも、色を透明度で割る: GPU が透明度を掛けた (64, 0, 0, 128) 相当 → 色は大きくなる
        let (out, _, _) = downsample_box(&[100, 0, 0, 128], 1, 1, 1, true);
        assert!(out[0] > 100 && out[3] == 128, "{out:?}");
    }

    #[test]
    fn test_supersample_is_lowered_to_fit_the_limits() {
        assert_eq!(fit_supersample(2000, 1500, 2, 16384), 2);
        // 画素数の上限（6400 万画素）: 4000x3000 の 2 倍は 4800 万画素で収まり、3 倍は 1 億画素で収まらない
        assert_eq!(fit_supersample(4000, 3000, 2, 16384), 2);
        assert_eq!(fit_supersample(4000, 3000, 3, 16384), 2);
        assert_eq!(fit_supersample(2000, 1500, 3, 16384), 3);
        assert_eq!(fit_supersample(3000, 2250, 3, 16384), 3);
        assert_eq!(fit_supersample(6000, 4500, 2, 16384), 1);
        // 一辺の限界
        assert_eq!(fit_supersample(2000, 1500, 3, 4000), 2);
        assert_eq!(fit_supersample(2000, 1500, 0, 16384), 1);
    }

    #[test]
    fn test_png_round_trips_the_pixels() {
        let rgba: Vec<u8> = (0..4 * 3 * 4).map(|i| (i * 7) as u8).collect();
        let bytes = encode_png(&rgba, 4, 3).unwrap();
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buf).unwrap();
        assert_eq!((info.width, info.height), (4, 3));
        assert_eq!(&buf[..info.buffer_size()], &rgba[..]);
    }
}
