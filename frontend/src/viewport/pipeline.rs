use iced::widget::shader;
use wgpu::util::DeviceExt;

use super::gpu_types::*;
use super::mesh::*;
use crate::culling::Frustum;
use crate::lod::{ImpostorVertex, create_impostor_quad};
use crate::ribbon::RibbonVertex;
use crate::surface_render::SurfaceBatches;

const BG_SHADER_SOURCE: &str = include_str!("shaders/background.wgsl");
const SSAO_SHADER_SOURCE: &str = include_str!("shaders/ssao.wgsl");
/// SSAO の遮蔽の係数を置くテクスチャの形式（1 チャンネル、0〜1）
const AO_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;

const SHADER_SOURCE: &str = include_str!("shaders/main.wgsl");
const SELECTION_SHADER_SOURCE: &str = include_str!("shaders/selection.wgsl");

/// 分子図の識別子。GPU の資源（バッファ・ユニフォームなど）は、この識別子ごとに持つ。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ViewportId {
    /// 1つ目の分子図（視点は `AppState.camera` とつながる）
    Primary,
    /// 2つ目の分子図（視点はその画面だけのもの）
    Secondary,
}

/// 分子図ごとの GPU 資源（バッファ・ユニフォーム・バインドグループ）。
///
/// iced は同じ型の `shader::Pipeline` を 1 つしか作らないため、分子図を複数置くと、
/// 共有のバッファを `prepare` で上書きし合ってしまう。そこで、書き込まれるものはすべてここにまとめ、
/// 識別子（`ViewportId`）ごとに `ViewportPipeline::viewports` に持つ。
pub struct ViewportResources {
    pub(crate) background_bind_group: wgpu::BindGroup,
    pub(crate) background_uniform_buffer: wgpu::Buffer,
    /// SSAO のユニフォームと、深度テクスチャ・ユニフォームを束ねたもの（SSAO が有効なフレームだけ作る）
    pub(crate) ssao_uniform_buffer: wgpu::Buffer,
    pub(crate) ssao_bind_group: Option<wgpu::BindGroup>,
    /// ぼかしのパスの束ね（深度・ユニフォーム・遮蔽の係数のテクスチャ）
    pub(crate) ssao_blur_bind_group: Option<wgpu::BindGroup>,
    pub(crate) bind_group: wgpu::BindGroup,
    pub(crate) uniform_buffer: wgpu::Buffer,

    // 球（原子）用バッファ（カリング & 段階的LOD間接描画: Near/Mid）
    pub(crate) sphere_src_buffer: wgpu::Buffer,
    pub(crate) sphere_dst_buffer_near: wgpu::Buffer,
    pub(crate) sphere_indirect_buffer_near: wgpu::Buffer,
    pub(crate) sphere_dst_buffer_mid: wgpu::Buffer,
    pub(crate) sphere_indirect_buffer_mid: wgpu::Buffer,
    pub(crate) sphere_cull_uniform_buffer: wgpu::Buffer,
    pub(crate) sphere_cull_bind_group: Option<wgpu::BindGroup>,
    pub(crate) sphere_capacity: usize,
    pub(crate) sphere_total_count: u32,

    // スティック（結合）用バッファ（カリング & 間接描画: Nearのみ）
    pub(crate) stick_src_buffer: wgpu::Buffer,
    pub(crate) stick_dst_buffer: wgpu::Buffer,
    pub(crate) stick_indirect_buffer: wgpu::Buffer,
    pub(crate) stick_dst_mid_buffer: wgpu::Buffer,
    pub(crate) stick_indirect_mid_buffer: wgpu::Buffer,
    pub(crate) stick_cull_uniform_buffer: wgpu::Buffer,
    pub(crate) stick_cull_bind_group: Option<wgpu::BindGroup>,
    pub(crate) stick_capacity: usize,
    pub(crate) stick_total_count: u32,

    // リボンバッファ
    pub(crate) ribbon_vertex_buffer: wgpu::Buffer,
    pub(crate) ribbon_index_buffer: wgpu::Buffer,
    pub(crate) ribbon_vertex_capacity: usize,
    pub(crate) ribbon_index_capacity: usize,
    pub(crate) ribbon_index_count: u32,

    // 分子表面（Surface）用バッファ
    pub(crate) surface_opaque_vertex_buffer: wgpu::Buffer,
    pub(crate) surface_opaque_index_buffer: wgpu::Buffer,
    pub(crate) surface_opaque_vertex_capacity: usize,
    pub(crate) surface_opaque_index_capacity: usize,
    pub(crate) surface_opaque_index_count: u32,

    pub(crate) surface_transparent_vertex_buffer: wgpu::Buffer,
    pub(crate) surface_transparent_index_buffer: wgpu::Buffer,
    pub(crate) surface_transparent_vertex_capacity: usize,
    pub(crate) surface_transparent_index_capacity: usize,
    pub(crate) surface_transparent_index_count: u32,

    // 相互作用（破線円柱）用インスタンスバッファ
    pub(crate) interaction_instance_buffer: wgpu::Buffer,
    pub(crate) interaction_capacity: usize,
    pub(crate) interaction_total_count: u32,

    // 選択の強調（ハロー / グロー）用インスタンスバッファ（原子ごとに 1 つ）
    pub(crate) selection_instance_buffer: wgpu::Buffer,
    pub(crate) selection_capacity: usize,
    pub(crate) selection_total_count: u32,
}

/// wgpu パイプライン、共通メッシュ、および分子図ごとの資源（`viewports`）
pub struct ViewportPipeline {
    // --- すべての分子図で共有するもの ---
    pub(crate) render_pipeline: wgpu::RenderPipeline,
    pub(crate) ribbon_render_pipeline: wgpu::RenderPipeline,
    pub(crate) surface_transparent_render_pipeline: wgpu::RenderPipeline,
    pub(crate) background_pipeline: wgpu::RenderPipeline,
    pub(crate) interaction_render_pipeline: wgpu::RenderPipeline,
    /// 選択の強調: ハロー（輪）とグロー（にじむ光）
    pub(crate) selection_halo_pipeline: wgpu::RenderPipeline,
    pub(crate) selection_glow_pipeline: wgpu::RenderPipeline,
    pub(crate) camera_bind_group_layout: wgpu::BindGroupLayout,
    pub(crate) background_bind_group_layout: wgpu::BindGroupLayout,
    /// SSAO: シーンのあとに、深度から求めた遮蔽の係数を乗算で重ねる全画面のパス
    /// 1 段目: 遮蔽の係数を、専用のテクスチャ（`ao_texture_view`）に描く
    pub(crate) ssao_pipeline: wgpu::RenderPipeline,
    pub(crate) ssao_bind_group_layout: wgpu::BindGroupLayout,
    /// 2 段目: 深度が近い隣とだけ混ぜて（ぼかし）、今の色に掛ける
    pub(crate) ssao_blur_pipeline: wgpu::RenderPipeline,
    pub(crate) ssao_blur_bind_group_layout: wgpu::BindGroupLayout,

    // インポスター球用パイプライン & クワッドメッシュ
    pub(crate) impostor_render_pipeline: wgpu::RenderPipeline,
    pub(crate) impostor_quad_vertex_buffer: wgpu::Buffer,
    pub(crate) impostor_quad_index_buffer: wgpu::Buffer,
    pub(crate) impostor_quad_index_count: u32,

    // マスターメッシュ
    pub(crate) sphere_vertex_buffer: wgpu::Buffer,
    pub(crate) sphere_index_buffer: wgpu::Buffer,
    pub(crate) sphere_index_count: u32,

    pub(crate) cylinder_vertex_buffer: wgpu::Buffer,
    pub(crate) cylinder_index_buffer: wgpu::Buffer,
    pub(crate) cylinder_index_count: u32,

    // 深度テクスチャ（描画パスごとに 1.0 でクリアするので、すべての分子図で共有できる）
    pub(crate) depth_texture: Option<wgpu::Texture>,
    pub(crate) depth_texture_view: Option<wgpu::TextureView>,
    pub(crate) depth_size: (u32, u32),
    // SSAO の遮蔽の係数（深度と同じ大きさ）。1 段目が描き、2 段目（ぼかし）が読む
    pub(crate) ao_texture_view: Option<wgpu::TextureView>,

    // 視錐台カリング Compute Pipeline
    pub(crate) cull_pipeline: wgpu::ComputePipeline,
    pub(crate) cull_bind_group_layout: wgpu::BindGroupLayout,

    // --- 分子図ごとのもの ---
    pub(crate) viewports: std::collections::HashMap<ViewportId, ViewportResources>,
    /// 分子図ごとの資源をいつ作り、いつ解放するかの記録
    ledger: ResourceLedger,
}

#[allow(clippy::too_many_arguments)] // wgpu のバッファ（コンピュート・インダイレクト）バインディングに必要な引数を直接受け取る設計を維持
fn create_cull_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniform_buf: &wgpu::Buffer,
    src_buf: &wgpu::Buffer,
    dst_buf_near: &wgpu::Buffer,
    indirect_buf_near: &wgpu::Buffer,
    dst_buf_mid: &wgpu::Buffer,
    indirect_buf_mid: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Frustum Culling & LOD Bind Group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: src_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: dst_buf_near.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: indirect_buf_near.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: dst_buf_mid.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: indirect_buf_mid.as_entire_binding(),
            },
        ],
    })
}

impl shader::Pipeline for ViewportPipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Ball-and-stick and Ribbon Shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_SOURCE.into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Camera Bind Group Layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Viewport Pipeline Layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Ball-and-stick Render Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Vertex::desc(), InstanceRaw::desc()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // リボン（カートゥーン）用レンダーパイプライン
        let ribbon_render_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Ribbon Render Pipeline"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_ribbon"),
                    buffers: &[RibbonVertex::desc()],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: None, // 両面描画
                    polygon_mode: wgpu::PolygonMode::Fill,
                    unclipped_depth: false,
                    conservative: false,
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: true,
                    depth_compare: wgpu::CompareFunction::LessEqual,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            });

        // 半透明分子表面用レンダーパイプライン（深度書き込みOFF、アルファブレンド、両面描画）
        let surface_transparent_render_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Surface Transparent Render Pipeline"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_ribbon"),
                    buffers: &[RibbonVertex::desc()],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: None, // 両面描画
                    polygon_mode: wgpu::PolygonMode::Fill,
                    unclipped_depth: false,
                    conservative: false,
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: false, // 深度書き込み無効
                    depth_compare: wgpu::CompareFunction::LessEqual,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            });

        // 相互作用（破線円柱）用レンダーパイプライン
        let interaction_render_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Interaction Render Pipeline"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_interaction"),
                    buffers: &[Vertex::desc(), InstanceRaw::desc()],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_interaction"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: None, // 両面描画（破線の内側も見えるように）
                    polygon_mode: wgpu::PolygonMode::Fill,
                    unclipped_depth: false,
                    conservative: false,
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: true,
                    depth_compare: wgpu::CompareFunction::LessEqual,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            });

        // インポスター球用レンダーパイプライン
        let impostor_render_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Sphere Impostor Render Pipeline"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_impostor"),
                    buffers: &[ImpostorVertex::desc(), InstanceRaw::desc()],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_impostor"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: None,
                    polygon_mode: wgpu::PolygonMode::Fill,
                    unclipped_depth: false,
                    conservative: false,
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: true,
                    depth_compare: wgpu::CompareFunction::LessEqual,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            });

        // 選択の強調（ハロー / グロー）のパイプライン。インポスターと同じクワッドとインスタンスを使い、
        // シーンの深度で深度テストするが、深度は書かない。
        let selection_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Selection Highlight Shader"),
            source: wgpu::ShaderSource::Wgsl(SELECTION_SHADER_SOURCE.into()),
        });
        let make_selection_pipeline = |label: &str, vs: &str, fs: &str, blend: wgpu::BlendState| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &selection_shader,
                    entry_point: Some(vs),
                    buffers: &[ImpostorVertex::desc(), InstanceRaw::desc()],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &selection_shader,
                    entry_point: Some(fs),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(blend),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: None,
                    polygon_mode: wgpu::PolygonMode::Fill,
                    unclipped_depth: false,
                    conservative: false,
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: false,
                    depth_compare: wgpu::CompareFunction::LessEqual,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };
        let selection_halo_pipeline = make_selection_pipeline(
            "Selection Halo Render Pipeline",
            "vs_halo",
            "fs_halo",
            wgpu::BlendState::ALPHA_BLENDING,
        );
        let selection_glow_pipeline = make_selection_pipeline(
            "Selection Glow Render Pipeline",
            "vs_glow",
            "fs_glow",
            // 乗算済みアルファ
            wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                    operation: wgpu::BlendOperation::Add,
                },
            },
        );

        let bg_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Background Fill Shader"),
            source: wgpu::ShaderSource::Wgsl(BG_SHADER_SOURCE.into()),
        });

        let bg_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Background Bind Group Layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        let bg_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Background Pipeline Layout"),
            bind_group_layouts: &[&bg_bind_group_layout],
            push_constant_ranges: &[],
        });

        let background_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Background Render Pipeline"),
            layout: Some(&bg_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &bg_shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &bg_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // SSAO: 深度テクスチャ（読むだけ）とユニフォーム。出力は、今の色に掛ける係数（乗算のブレンド）
        let ssao_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("SSAO Shader"),
            source: wgpu::ShaderSource::Wgsl(SSAO_SHADER_SOURCE.into()),
        });
        let ssao_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("SSAO Bind Group Layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            // Depth32Float は、フィルターなしの float として読める（GL でも動く）
                            sample_type: wgpu::TextureSampleType::Float { filterable: false },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });
        let ssao_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("SSAO Pipeline Layout"),
            bind_group_layouts: &[&ssao_bind_group_layout],
            push_constant_ranges: &[],
        });
        let ssao_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("SSAO Render Pipeline"),
            layout: Some(&ssao_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &ssao_shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &ssao_shader,
                entry_point: Some("fs_ao"),
                // 係数を、そのまま書く（混ぜない）。1 チャンネルで足りる
                targets: &[Some(wgpu::ColorTargetState {
                    format: AO_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // ぼかし: 1 段目の結果も読むので、束ねが 1 つ多い（1 段目のパスでは、これは描き先なので別の束ね）
        let ssao_blur_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("SSAO Blur Bind Group Layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: false },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: false },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                ],
            });
        let ssao_blur_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("SSAO Blur Pipeline Layout"),
                bind_group_layouts: &[&ssao_blur_bind_group_layout],
                push_constant_ranges: &[],
            });
        let ssao_blur_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("SSAO Blur Render Pipeline"),
            layout: Some(&ssao_blur_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &ssao_shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &ssao_shader,
                entry_point: Some("fs_blur"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    // 色: 出力 × 今の色（暗くするだけ）。アルファは、今のまま
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::Zero,
                            dst_factor: wgpu::BlendFactor::Src,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::Zero,
                            dst_factor: wgpu::BlendFactor::One,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // マスターインポスタークワッドメッシュ
        let (quad_verts, quad_indices) = create_impostor_quad();
        let impostor_quad_vertex_buffer =
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Impostor Quad Vertex Buffer"),
                contents: bytemuck::cast_slice(&quad_verts),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let impostor_quad_index_buffer =
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Impostor Quad Index Buffer"),
                contents: bytemuck::cast_slice(&quad_indices),
                usage: wgpu::BufferUsages::INDEX,
            });
        let impostor_quad_index_count = quad_indices.len() as u32;

        // マスター球メッシュ
        let (sphere_verts, sphere_indices) = create_sphere_mesh(16, 12);
        let sphere_vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Sphere Vertex Buffer"),
            contents: bytemuck::cast_slice(&sphere_verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let sphere_index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Sphere Index Buffer"),
            contents: bytemuck::cast_slice(&sphere_indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        // マスター円柱メッシュ
        let (cyl_verts, cyl_indices) = create_cylinder_mesh(14);
        let cylinder_vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Cylinder Vertex Buffer"),
            contents: bytemuck::cast_slice(&cyl_verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let cylinder_index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Cylinder Index Buffer"),
            contents: bytemuck::cast_slice(&cyl_indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        // 視錐台カリング Compute Pipeline の初期化
        let cull_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Frustum Culling Bind Group Layout"),
                entries: &[
                    // binding 0: uniform FrustumUniforms
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    // binding 1: storage src_instances (read-only)
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    // binding 2: storage dst_instances (read_write)
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: false },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    // binding 3: storage indirect_args_near (read_write)
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: false },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    // binding 4: storage dst_instances_mid (read_write)
                    wgpu::BindGroupLayoutEntry {
                        binding: 4,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: false },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    // binding 5: storage indirect_args_mid (read_write)
                    wgpu::BindGroupLayoutEntry {
                        binding: 5,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: false },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });

        let cull_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Frustum Culling Pipeline Layout"),
            bind_group_layouts: &[&cull_bind_group_layout],
            push_constant_ranges: &[],
        });

        let cull_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Frustum Culling Compute Pipeline"),
            layout: Some(&cull_pipeline_layout),
            module: &shader,
            entry_point: Some("cs_cull"),
            compilation_options: Default::default(),
            cache: None,
        });

        Self {
            render_pipeline,
            ribbon_render_pipeline,
            surface_transparent_render_pipeline,
            background_pipeline,
            interaction_render_pipeline,
            selection_halo_pipeline,
            selection_glow_pipeline,
            camera_bind_group_layout: bind_group_layout,
            background_bind_group_layout: bg_bind_group_layout,
            ssao_pipeline,
            ssao_bind_group_layout,
            ssao_blur_pipeline,
            ssao_blur_bind_group_layout,
            impostor_render_pipeline,
            impostor_quad_vertex_buffer,
            impostor_quad_index_buffer,
            impostor_quad_index_count,
            sphere_vertex_buffer,
            sphere_index_buffer,
            sphere_index_count: sphere_indices.len() as u32,
            cylinder_vertex_buffer,
            cylinder_index_buffer,
            cylinder_index_count: cyl_indices.len() as u32,
            depth_texture: None,
            depth_texture_view: None,
            depth_size: (0, 0),
            ao_texture_view: None,
            cull_pipeline,
            cull_bind_group_layout,
            viewports: std::collections::HashMap::new(),
            ledger: ResourceLedger::default(),
        }
    }

    /// フレームの終わりに、使われなかった分子図の資源を解放する
    /// （2つ目の分子図を閉じたとき、GPU のバッファを持ち続けないため）。
    fn trim(&mut self) {
        for id in self.ledger.trim() {
            self.viewports.remove(&id);
        }
    }
}

/// 共通メッシュのインデックス数（分子図ごとの間接描画の引数に使う）
#[derive(Debug, Clone, Copy)]
pub(crate) struct MeshCounts {
    pub(crate) sphere_index_count: u32,
    pub(crate) impostor_quad_index_count: u32,
    pub(crate) cylinder_index_count: u32,
}

impl ViewportPipeline {
    /// 指定した分子図の資源がなければ作り、今のフレームで使うものとして記録する。
    pub(crate) fn ensure_viewport(&mut self, id: ViewportId, device: &wgpu::Device) {
        if self.ledger.touch(id) || !self.viewports.contains_key(&id) {
            let resources = ViewportResources::new(
                device,
                &self.camera_bind_group_layout,
                &self.background_bind_group_layout,
                &self.cull_bind_group_layout,
            );
            self.viewports.insert(id, resources);
        }
    }

    pub(crate) fn mesh_counts(&self) -> MeshCounts {
        MeshCounts {
            sphere_index_count: self.sphere_index_count,
            impostor_quad_index_count: self.impostor_quad_index_count,
            cylinder_index_count: self.cylinder_index_count,
        }
    }

    pub(crate) fn update_depth_texture(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        if self.depth_size == (width, height) && self.depth_texture.is_some() {
            return;
        }

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Viewport Depth Texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            // SSAO が、描いたあとの深度を読む
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        // 遮蔽の係数のテクスチャ: 1 段目の描き先で、2 段目の読み元
        let ao_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Viewport SSAO Texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: AO_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        self.ao_texture_view =
            Some(ao_texture.create_view(&wgpu::TextureViewDescriptor::default()));

        self.depth_texture = Some(texture);
        self.depth_texture_view = Some(view);
        self.depth_size = (width, height);
    }
}

impl ViewportResources {
    fn new(
        device: &wgpu::Device,
        bind_group_layout: &wgpu::BindGroupLayout,
        bg_bind_group_layout: &wgpu::BindGroupLayout,
        cull_bind_group_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Camera Uniform Buffer"),
            size: std::mem::size_of::<CameraUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Camera Bind Group"),
            layout: bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        let background_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Background Uniform Buffer"),
            size: std::mem::size_of::<BackgroundUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let ssao_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("SSAO Uniform Buffer"),
            size: std::mem::size_of::<SsaoUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let background_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Background Bind Group"),
            layout: bg_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: background_uniform_buffer.as_entire_binding(),
            }],
        });

        // 初期リボンバッファ
        let ribbon_vertex_capacity = 256;
        let ribbon_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Ribbon Dynamic Vertex Buffer"),
            size: (std::mem::size_of::<RibbonVertex>() * ribbon_vertex_capacity) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let ribbon_index_capacity = 512;
        let ribbon_index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Ribbon Dynamic Index Buffer"),
            size: (std::mem::size_of::<u32>() * ribbon_index_capacity) as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // 初期分子表面バッファ（不透明 & 半透明）
        let surface_opaque_vertex_capacity = 256;
        let surface_opaque_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Surface Opaque Dynamic Vertex Buffer"),
            size: (std::mem::size_of::<RibbonVertex>() * surface_opaque_vertex_capacity) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let surface_opaque_index_capacity = 512;
        let surface_opaque_index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Surface Opaque Dynamic Index Buffer"),
            size: (std::mem::size_of::<u32>() * surface_opaque_index_capacity) as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let surface_transparent_vertex_capacity = 256;
        let surface_transparent_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Surface Transparent Dynamic Vertex Buffer"),
            size: (std::mem::size_of::<RibbonVertex>() * surface_transparent_vertex_capacity)
                as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let surface_transparent_index_capacity = 512;
        let surface_transparent_index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Surface Transparent Dynamic Index Buffer"),
            size: (std::mem::size_of::<u32>() * surface_transparent_index_capacity) as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // 初期相互作用バッファ
        let interaction_capacity = 256;
        let interaction_instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Interaction Dynamic Instance Buffer"),
            size: (std::mem::size_of::<InstanceRaw>() * interaction_capacity) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // 選択の強調用バッファ
        let selection_capacity = 64;
        let selection_instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Selection Highlight Instance Buffer"),
            size: (std::mem::size_of::<InstanceRaw>() * selection_capacity) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // 初期カリング・間接描画バッファ
        let initial_capacity = 64;

        // 球用バッファ
        let sphere_cull_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Sphere Frustum Culling Uniform Buffer"),
            size: std::mem::size_of::<FrustumUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sphere_src_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Sphere Source Instance Buffer"),
            size: (std::mem::size_of::<InstanceRaw>() * initial_capacity) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sphere_dst_buffer_near = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Sphere Culled Instance Buffer Near"),
            size: (std::mem::size_of::<InstanceRaw>() * initial_capacity) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::VERTEX,
            mapped_at_creation: false,
        });
        let sphere_indirect_buffer_near = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Sphere Draw Indirect Buffer Near"),
            size: std::mem::size_of::<DrawIndexedIndirectArgs>() as u64,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::INDIRECT
                | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sphere_dst_buffer_mid = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Sphere Culled Instance Buffer Mid"),
            size: (std::mem::size_of::<InstanceRaw>() * initial_capacity) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::VERTEX,
            mapped_at_creation: false,
        });
        let sphere_indirect_buffer_mid = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Sphere Draw Indirect Buffer Mid"),
            size: std::mem::size_of::<DrawIndexedIndirectArgs>() as u64,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::INDIRECT
                | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sphere_cull_bind_group = Some(create_cull_bind_group(
            device,
            cull_bind_group_layout,
            &sphere_cull_uniform_buffer,
            &sphere_src_buffer,
            &sphere_dst_buffer_near,
            &sphere_indirect_buffer_near,
            &sphere_dst_buffer_mid,
            &sphere_indirect_buffer_mid,
        ));

        // スティック用バッファ
        let stick_cull_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Stick Frustum Culling Uniform Buffer"),
            size: std::mem::size_of::<FrustumUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let stick_src_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Stick Source Instance Buffer"),
            size: (std::mem::size_of::<InstanceRaw>() * initial_capacity) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let stick_dst_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Stick Culled Instance Buffer"),
            size: (std::mem::size_of::<InstanceRaw>() * initial_capacity) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::VERTEX,
            mapped_at_creation: false,
        });
        let stick_indirect_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Stick Draw Indirect Buffer"),
            size: std::mem::size_of::<DrawIndexedIndirectArgs>() as u64,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::INDIRECT
                | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let stick_dst_mid_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Stick Dummy Culled Mid Buffer"),
            size: std::mem::size_of::<InstanceRaw>() as u64,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let stick_indirect_mid_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Stick Dummy Draw Indirect Mid Buffer"),
            size: std::mem::size_of::<DrawIndexedIndirectArgs>() as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let stick_cull_bind_group = Some(create_cull_bind_group(
            device,
            cull_bind_group_layout,
            &stick_cull_uniform_buffer,
            &stick_src_buffer,
            &stick_dst_buffer,
            &stick_indirect_buffer,
            &stick_dst_mid_buffer,
            &stick_indirect_mid_buffer,
        ));

        Self {
            background_bind_group,
            background_uniform_buffer,
            ssao_uniform_buffer,
            ssao_bind_group: None,
            ssao_blur_bind_group: None,
            bind_group,
            uniform_buffer,
            sphere_src_buffer,
            sphere_dst_buffer_near,
            sphere_indirect_buffer_near,
            sphere_dst_buffer_mid,
            sphere_indirect_buffer_mid,
            sphere_cull_uniform_buffer,
            sphere_cull_bind_group,
            sphere_capacity: initial_capacity,
            sphere_total_count: 0,
            stick_src_buffer,
            stick_dst_buffer,
            stick_indirect_buffer,
            stick_dst_mid_buffer,
            stick_indirect_mid_buffer,
            stick_cull_uniform_buffer,
            stick_cull_bind_group,
            stick_capacity: initial_capacity,
            stick_total_count: 0,
            ribbon_vertex_buffer,
            ribbon_index_buffer,
            ribbon_vertex_capacity,
            ribbon_index_capacity,
            ribbon_index_count: 0,
            surface_opaque_vertex_buffer,
            surface_opaque_index_buffer,
            surface_opaque_vertex_capacity,
            surface_opaque_index_capacity,
            surface_opaque_index_count: 0,
            surface_transparent_vertex_buffer,
            surface_transparent_index_buffer,
            surface_transparent_vertex_capacity,
            surface_transparent_index_capacity,
            surface_transparent_index_count: 0,
            interaction_instance_buffer,
            interaction_capacity,
            interaction_total_count: 0,
            selection_instance_buffer,
            selection_capacity,
            selection_total_count: 0,
        }
    }

    /// 選択の強調のインスタンス（選ばれている原子ごとに 1 つ）をバッファへ書く
    pub(crate) fn update_selection_buffers(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        instances: &[InstanceRaw],
    ) {
        self.selection_total_count = instances.len() as u32;
        if instances.is_empty() {
            return;
        }

        if instances.len() > self.selection_capacity {
            let new_capacity = instances.len().max(self.selection_capacity * 2);
            self.selection_instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Selection Highlight Instance Buffer"),
                size: (std::mem::size_of::<InstanceRaw>() * new_capacity) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.selection_capacity = new_capacity;
        }

        queue.write_buffer(
            &self.selection_instance_buffer,
            0,
            bytemuck::cast_slice(instances),
        );
    }

    pub(crate) fn update_interaction_buffers(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        instances: &[InstanceRaw],
    ) {
        self.interaction_total_count = instances.len() as u32;
        if instances.is_empty() {
            return;
        }

        if instances.len() > self.interaction_capacity {
            let new_capacity = instances.len().max(self.interaction_capacity * 2);
            self.interaction_instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Interaction Dynamic Instance Buffer"),
                size: (std::mem::size_of::<InstanceRaw>() * new_capacity) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.interaction_capacity = new_capacity;
        }

        queue.write_buffer(
            &self.interaction_instance_buffer,
            0,
            bytemuck::cast_slice(instances),
        );
    }

    pub(crate) fn update_ribbon_buffers(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        vertices: &[RibbonVertex],
        indices: &[u32],
    ) {
        self.ribbon_index_count = indices.len() as u32;
        if vertices.is_empty() || indices.is_empty() {
            return;
        }

        if vertices.len() > self.ribbon_vertex_capacity {
            let new_capacity = vertices.len().max(self.ribbon_vertex_capacity * 2);
            self.ribbon_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Ribbon Dynamic Vertex Buffer"),
                size: (std::mem::size_of::<RibbonVertex>() * new_capacity) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.ribbon_vertex_capacity = new_capacity;
        }

        if indices.len() > self.ribbon_index_capacity {
            let new_capacity = indices.len().max(self.ribbon_index_capacity * 2);
            self.ribbon_index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Ribbon Dynamic Index Buffer"),
                size: (std::mem::size_of::<u32>() * new_capacity) as u64,
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.ribbon_index_capacity = new_capacity;
        }

        queue.write_buffer(
            &self.ribbon_vertex_buffer,
            0,
            bytemuck::cast_slice(vertices),
        );
        queue.write_buffer(&self.ribbon_index_buffer, 0, bytemuck::cast_slice(indices));
    }

    pub(crate) fn update_surface_buffers(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batches: &SurfaceBatches,
    ) {
        self.surface_opaque_index_count = batches.opaque_indices.len() as u32;
        if !batches.opaque_vertices.is_empty() && !batches.opaque_indices.is_empty() {
            if batches.opaque_vertices.len() > self.surface_opaque_vertex_capacity {
                let new_capacity = batches
                    .opaque_vertices
                    .len()
                    .max(self.surface_opaque_vertex_capacity * 2);
                self.surface_opaque_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("Surface Opaque Dynamic Vertex Buffer"),
                    size: (std::mem::size_of::<RibbonVertex>() * new_capacity) as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                self.surface_opaque_vertex_capacity = new_capacity;
            }

            if batches.opaque_indices.len() > self.surface_opaque_index_capacity {
                let new_capacity = batches
                    .opaque_indices
                    .len()
                    .max(self.surface_opaque_index_capacity * 2);
                self.surface_opaque_index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("Surface Opaque Dynamic Index Buffer"),
                    size: (std::mem::size_of::<u32>() * new_capacity) as u64,
                    usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                self.surface_opaque_index_capacity = new_capacity;
            }

            queue.write_buffer(
                &self.surface_opaque_vertex_buffer,
                0,
                bytemuck::cast_slice(&batches.opaque_vertices),
            );
            queue.write_buffer(
                &self.surface_opaque_index_buffer,
                0,
                bytemuck::cast_slice(&batches.opaque_indices),
            );
        }

        self.surface_transparent_index_count = batches.transparent_indices.len() as u32;
        if !batches.transparent_vertices.is_empty() && !batches.transparent_indices.is_empty() {
            if batches.transparent_vertices.len() > self.surface_transparent_vertex_capacity {
                let new_capacity = batches
                    .transparent_vertices
                    .len()
                    .max(self.surface_transparent_vertex_capacity * 2);
                self.surface_transparent_vertex_buffer =
                    device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("Surface Transparent Dynamic Vertex Buffer"),
                        size: (std::mem::size_of::<RibbonVertex>() * new_capacity) as u64,
                        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });
                self.surface_transparent_vertex_capacity = new_capacity;
            }

            if batches.transparent_indices.len() > self.surface_transparent_index_capacity {
                let new_capacity = batches
                    .transparent_indices
                    .len()
                    .max(self.surface_transparent_index_capacity * 2);
                self.surface_transparent_index_buffer =
                    device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("Surface Transparent Dynamic Index Buffer"),
                        size: (std::mem::size_of::<u32>() * new_capacity) as u64,
                        usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });
                self.surface_transparent_index_capacity = new_capacity;
            }

            queue.write_buffer(
                &self.surface_transparent_vertex_buffer,
                0,
                bytemuck::cast_slice(&batches.transparent_vertices),
            );
            queue.write_buffer(
                &self.surface_transparent_index_buffer,
                0,
                bytemuck::cast_slice(&batches.transparent_indices),
            );
        }
    }

    #[allow(clippy::too_many_arguments)] // 共有の資源（レイアウト・メッシュ数）を借用の衝突なしに受け取るため、引数で渡す
    pub(crate) fn update_instances(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        cull_bind_group_layout: &wgpu::BindGroupLayout,
        counts: MeshCounts,
        spheres: &[InstanceRaw],
        sticks: &[InstanceRaw],
        frustum: &Frustum,
        camera_eye: [f32; 3],
    ) {
        let lod_threshold_sq =
            crate::lod::DEFAULT_LOD_MID_DISTANCE * crate::lod::DEFAULT_LOD_MID_DISTANCE;

        self.sphere_total_count = spheres.len() as u32;
        if !spheres.is_empty() {
            if spheres.len() > self.sphere_capacity {
                let new_cap = spheres.len().max(self.sphere_capacity * 2);
                self.sphere_src_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("Sphere Source Instance Buffer"),
                    size: (std::mem::size_of::<InstanceRaw>() * new_cap) as u64,
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                self.sphere_dst_buffer_near = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("Sphere Culled Instance Buffer Near"),
                    size: (std::mem::size_of::<InstanceRaw>() * new_cap) as u64,
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::VERTEX,
                    mapped_at_creation: false,
                });
                self.sphere_dst_buffer_mid = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("Sphere Culled Instance Buffer Mid"),
                    size: (std::mem::size_of::<InstanceRaw>() * new_cap) as u64,
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::VERTEX,
                    mapped_at_creation: false,
                });
                self.sphere_cull_bind_group = Some(create_cull_bind_group(
                    device,
                    cull_bind_group_layout,
                    &self.sphere_cull_uniform_buffer,
                    &self.sphere_src_buffer,
                    &self.sphere_dst_buffer_near,
                    &self.sphere_indirect_buffer_near,
                    &self.sphere_dst_buffer_mid,
                    &self.sphere_indirect_buffer_mid,
                ));
                self.sphere_capacity = new_cap;
            }
            queue.write_buffer(&self.sphere_src_buffer, 0, bytemuck::cast_slice(spheres));

            // 近距離・中距離それぞれの間接描画引数の初期化
            let sphere_args_near = DrawIndexedIndirectArgs {
                index_count: counts.sphere_index_count,
                instance_count: 0,
                first_index: 0,
                base_vertex: 0,
                first_instance: 0,
            };
            queue.write_buffer(
                &self.sphere_indirect_buffer_near,
                0,
                bytemuck::bytes_of(&sphere_args_near),
            );

            let sphere_args_mid = DrawIndexedIndirectArgs {
                index_count: counts.impostor_quad_index_count,
                instance_count: 0,
                first_index: 0,
                base_vertex: 0,
                first_instance: 0,
            };
            queue.write_buffer(
                &self.sphere_indirect_buffer_mid,
                0,
                bytemuck::bytes_of(&sphere_args_mid),
            );

            // 視錐台 & LOD ユニフォームの更新（is_stick = 0）
            let sphere_frustum_uniforms = FrustumUniforms {
                planes: frustum.to_shader_planes(),
                camera_pos: [camera_eye[0], camera_eye[1], camera_eye[2], 1.0],
                lod_threshold_sq,
                total_instances: self.sphere_total_count,
                is_stick: 0,
                _pad0: 0,
            };
            queue.write_buffer(
                &self.sphere_cull_uniform_buffer,
                0,
                bytemuck::bytes_of(&sphere_frustum_uniforms),
            );
        }

        self.stick_total_count = sticks.len() as u32;
        if !sticks.is_empty() {
            if sticks.len() > self.stick_capacity {
                let new_cap = sticks.len().max(self.stick_capacity * 2);
                self.stick_src_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("Stick Source Instance Buffer"),
                    size: (std::mem::size_of::<InstanceRaw>() * new_cap) as u64,
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                self.stick_dst_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("Stick Culled Instance Buffer"),
                    size: (std::mem::size_of::<InstanceRaw>() * new_cap) as u64,
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::VERTEX,
                    mapped_at_creation: false,
                });
                self.stick_cull_bind_group = Some(create_cull_bind_group(
                    device,
                    cull_bind_group_layout,
                    &self.stick_cull_uniform_buffer,
                    &self.stick_src_buffer,
                    &self.stick_dst_buffer,
                    &self.stick_indirect_buffer,
                    &self.stick_dst_mid_buffer,
                    &self.stick_indirect_mid_buffer,
                ));
                self.stick_capacity = new_cap;
            }
            queue.write_buffer(&self.stick_src_buffer, 0, bytemuck::cast_slice(sticks));

            // 間接描画引数の初期化
            let stick_args = DrawIndexedIndirectArgs {
                index_count: counts.cylinder_index_count,
                instance_count: 0,
                first_index: 0,
                base_vertex: 0,
                first_instance: 0,
            };
            queue.write_buffer(
                &self.stick_indirect_buffer,
                0,
                bytemuck::bytes_of(&stick_args),
            );

            // 視錐台 & LOD ユニフォームの更新（is_stick = 1）
            let stick_frustum_uniforms = FrustumUniforms {
                planes: frustum.to_shader_planes(),
                camera_pos: [camera_eye[0], camera_eye[1], camera_eye[2], 1.0],
                lod_threshold_sq,
                total_instances: self.stick_total_count,
                is_stick: 1,
                _pad0: 0,
            };
            queue.write_buffer(
                &self.stick_cull_uniform_buffer,
                0,
                bytemuck::bytes_of(&stick_frustum_uniforms),
            );
        }
    }
}

/// 分子図ごとの資源をいつ作り、いつ解放するかの記録（GPU を使わずにテストできるよう、資源そのものから分けてある）。
#[derive(Debug, Default)]
pub(crate) struct ResourceLedger {
    live: std::collections::HashSet<ViewportId>,
    touched: std::collections::HashSet<ViewportId>,
}

impl ResourceLedger {
    /// 資源がなければ作るべきか（`true`）。今のフレームで使うものとして記録する。
    pub(crate) fn touch(&mut self, id: ViewportId) -> bool {
        self.touched.insert(id);
        self.live.insert(id)
    }

    /// フレームの終わり。使われなかった分子図の資源を解放し、解放したものを返す。
    pub(crate) fn trim(&mut self) -> Vec<ViewportId> {
        let touched = std::mem::take(&mut self.touched);
        let mut dropped: Vec<ViewportId> = self.live.difference(&touched).copied().collect();
        dropped.sort();
        self.live.retain(|id| touched.contains(id));
        dropped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WGSL を GPU なしで構文・型の検査にかける。パイプラインの作成は GPU がないと試せないが、
    /// シェーダーの書き間違いはここで見つかる。
    fn validate_wgsl(source: &str) -> Result<(), String> {
        let module = naga::front::wgsl::parse_str(source).map_err(|e| e.emit_to_string(source))?;
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .map(|_| ())
        .map_err(|e| format!("{e:?}"))
    }

    #[test]
    fn test_shaders_are_valid_wgsl() {
        for (name, source) in [
            ("main.wgsl", SHADER_SOURCE),
            ("background.wgsl", BG_SHADER_SOURCE),
            ("selection.wgsl", SELECTION_SHADER_SOURCE),
            ("ssao.wgsl", SSAO_SHADER_SOURCE),
        ] {
            if let Err(e) = validate_wgsl(source) {
                panic!("{name} is not valid WGSL:\n{e}");
            }
        }
    }

    #[test]
    fn test_impostor_fragment_shader_writes_the_sphere_surface_depth() {
        let module = naga::front::wgsl::parse_str(SHADER_SOURCE).expect("main.wgsl parses");
        let entry = module
            .entry_points
            .iter()
            .find(|e| e.name == "fs_impostor")
            .expect("fs_impostor exists");
        let result = entry.function.result.as_ref().expect("fs_impostor returns");
        let naga::TypeInner::Struct { members, .. } = &module.types[result.ty].inner else {
            panic!("fs_impostor must return a struct carrying frag_depth");
        };
        assert!(
            members.iter().any(|m| matches!(
                m.binding,
                Some(naga::Binding::BuiltIn(naga::BuiltIn::FragDepth))
            )),
            "fs_impostor must write frag_depth, or sticks poke through the spheres"
        );
    }

    #[test]
    fn test_selection_shader_has_the_entry_points_the_pipelines_use() {
        let module =
            naga::front::wgsl::parse_str(SELECTION_SHADER_SOURCE).expect("selection.wgsl parses");
        let names: Vec<&str> = module
            .entry_points
            .iter()
            .map(|e| e.name.as_str())
            .collect();
        for entry in ["vs_halo", "vs_glow", "fs_halo", "fs_glow"] {
            assert!(
                names.contains(&entry),
                "missing entry point {entry}: {names:?}"
            );
        }
    }

    #[test]
    fn test_resources_are_created_once_per_viewport() {
        let mut ledger = ResourceLedger::default();
        assert!(ledger.touch(ViewportId::Primary));
        assert!(!ledger.touch(ViewportId::Primary));
        assert!(ledger.touch(ViewportId::Secondary));
        assert!(ledger.trim().is_empty());
    }

    #[test]
    fn test_unused_viewport_resources_are_released_at_frame_end() {
        let mut ledger = ResourceLedger::default();
        ledger.touch(ViewportId::Primary);
        ledger.touch(ViewportId::Secondary);
        ledger.trim();

        // 次のフレームでは1つ目だけ描かれた（分割をオフにした）
        ledger.touch(ViewportId::Primary);
        assert_eq!(ledger.trim(), vec![ViewportId::Secondary]);

        // 再び分割すると、資源は作り直される
        assert!(ledger.touch(ViewportId::Secondary));
    }
}
