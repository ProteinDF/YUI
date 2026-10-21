use super::rotation_between;
use crate::math::mat4_mul;
use bytemuck::{Pod, Zeroable};

/// 間接描画引数バッファ用構造体（wgpu::util::DrawIndexedIndirectArgs 互換、20バイト）
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct DrawIndexedIndirectArgs {
    pub index_count: u32,
    pub instance_count: u32,
    pub first_index: u32,
    pub base_vertex: i32,
    pub first_instance: u32,
}

/// 視錐台カリング & 段階的LOD用ユニフォーム（6平面 + カメラ座標 + LOD境界 + インスタンス数）
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct FrustumUniforms {
    pub planes: [[f32; 4]; 6],
    pub camera_pos: [f32; 4],
    pub lod_threshold_sq: f32,
    pub total_instances: u32,
    pub is_stick: u32,
    pub _pad0: u32,
}

/// 頂点データ（マスターメッシュ用）
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
}

impl Vertex {
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 3]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x3,
                },
            ],
        }
    }
}

/// インスタンスデータ（4x4 変換行列 + RGBAカラー）
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct InstanceRaw {
    pub model_0: [f32; 4],
    pub model_1: [f32; 4],
    pub model_2: [f32; 4],
    pub model_3: [f32; 4],
    pub color: [f32; 4],
}

impl InstanceRaw {
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<InstanceRaw>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: 16,
                    shader_location: 3,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: 32,
                    shader_location: 4,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: 48,
                    shader_location: 5,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: 64,
                    shader_location: 6,
                    format: wgpu::VertexFormat::Float32x4,
                },
            ],
        }
    }

    /// 平行移動と均等スケーリングからインスタンス行列を生成（球用）
    pub fn from_sphere(pos: [f32; 3], radius: f32, color: [f32; 4]) -> Self {
        #[rustfmt::skip]
        let m = [
            radius, 0.0, 0.0, 0.0,
            0.0, radius, 0.0, 0.0,
            0.0, 0.0, radius, 0.0,
            pos[0], pos[1], pos[2], 1.0,
        ];
        Self {
            model_0: [m[0], m[1], m[2], m[3]],
            model_1: [m[4], m[5], m[6], m[7]],
            model_2: [m[8], m[9], m[10], m[11]],
            model_3: [m[12], m[13], m[14], m[15]],
            color,
        }
    }

    /// 2点間を結ぶ円柱のインスタンス行列を生成（スティック用）
    pub fn from_cylinder(p1: [f32; 3], p2: [f32; 3], radius: f32, color: [f32; 4]) -> Self {
        let dir = [p2[0] - p1[0], p2[1] - p1[1], p2[2] - p1[2]];
        let len = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
        let mid = [
            (p1[0] + p2[0]) * 0.5,
            (p1[1] + p2[1]) * 0.5,
            (p1[2] + p2[2]) * 0.5,
        ];

        let z_axis = [0.0, 0.0, 1.0];
        let d = if len > 1e-6 {
            [dir[0] / len, dir[1] / len, dir[2] / len]
        } else {
            z_axis
        };

        // Z軸から d への回転行列（Rodriguesの回転公式相当）
        let rot = rotation_between(z_axis, d);

        // スケーリング: x, y は radius、z は len * 0.5 (メッシュが -1〜1 の場合)
        let half_len = len * 0.5;
        let scale = [
            radius, 0.0, 0.0, 0.0, 0.0, radius, 0.0, 0.0, 0.0, 0.0, half_len, 0.0, 0.0, 0.0, 0.0,
            1.0,
        ];

        let trans = [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, mid[0], mid[1], mid[2], 1.0,
        ];

        let rot_scale = mat4_mul(&rot, &scale);
        let model = mat4_mul(&trans, &rot_scale);

        Self {
            model_0: [model[0], model[1], model[2], model[3]],
            model_1: [model[4], model[5], model[6], model[7]],
            model_2: [model[8], model[9], model[10], model[11]],
            model_3: [model[12], model[13], model[14], model[15]],
            color,
        }
    }
}

/// カメラ・ライティング用ユニフォーム
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub(crate) struct CameraUniforms {
    pub(crate) view_proj: [f32; 16],
    pub(crate) light_dir: [f32; 4],
    pub(crate) camera_right: [f32; 4],
    pub(crate) camera_up: [f32; 4],
    /// 描画の大きさの設定（`user_settings::RenderSettings`）: x = 相互作用の破線の描く長さ（Å）、
    /// y = 破線の空白の長さ（Å）、z = 選択の強調の大きさの倍率。w は未使用
    pub(crate) style: [f32; 4],
}

/// SSAO のユニフォーム（`shaders/ssao.wgsl` の `SsaoUniforms` と同じ並び）
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub(crate) struct SsaoUniforms {
    pub(crate) near: f32,
    pub(crate) far: f32,
    pub(crate) tan_half_y: f32,
    pub(crate) tan_half_x: f32,
    /// 分子図の矩形（物理ピクセル）: x, y, 幅, 高さ
    pub(crate) pane: [f32; 4],
    pub(crate) radius: f32,
    pub(crate) strength: f32,
    pub(crate) _pad: [f32; 2],
}

/// 背景クリア描画用ユニフォーム（カラーのみ）
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub(crate) struct BackgroundUniforms {
    pub(crate) color: [f32; 4],
}
