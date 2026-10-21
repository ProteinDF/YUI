//! 段階的LOD（Level of Detail）およびインポスター球（ビルボード）計算モジュール。
//!
//! architecture.md 4.2節「段階的LOD」に基づき、カメラからの距離に応じた
//! 2段階の表現（近距離=フルジオメトリ球棒、中距離=インポスター球）の振り分け判定と、
//! カメラ正対クワッド展開・法線再構成の幾何計算を提供します。

#![allow(dead_code)]

use bytemuck::{Pod, Zeroable};

/// 段階的LODの表現レベル
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LodTier {
    /// 近距離: フルジオメトリメッシュ（球棒）
    Near,
    /// 中距離: ビルボードインポスター（スプライト球、結合非表示）
    Mid,
}

/// 既定の近距離/中距離の境界しきい値距離（Å）
#[allow(dead_code)]
pub const DEFAULT_LOD_MID_DISTANCE: f32 = 65.0;

/// 距離の2乗としきい値の2乗から LOD レベルを判定する。
#[inline]
pub fn classify_lod_tier_sq(dist_sq: f32, threshold_sq: f32) -> LodTier {
    if dist_sq <= threshold_sq {
        LodTier::Near
    } else {
        LodTier::Mid
    }
}

/// インポスター用マスタークワッドの頂点データ（2Dオフセット [-1, 1]）
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable, PartialEq)]
pub struct ImpostorVertex {
    pub position: [f32; 2],
}

impl ImpostorVertex {
    #[allow(dead_code)]
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<ImpostorVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[wgpu::VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x2,
            }],
        }
    }
}

/// 単位インポスタークワッド（4頂点、6インデックス）を生成する。
///
/// 頂点座標は局所平面上の [-1, -1] 〜 [1, 1]。
pub fn create_impostor_quad() -> (Vec<ImpostorVertex>, Vec<u16>) {
    let vertices = vec![
        ImpostorVertex {
            position: [-1.0, -1.0],
        }, // 0: 左下
        ImpostorVertex {
            position: [1.0, -1.0],
        }, // 1: 右下
        ImpostorVertex {
            position: [1.0, 1.0],
        }, // 2: 右上
        ImpostorVertex {
            position: [-1.0, 1.0],
        }, // 3: 左上
    ];

    let indices = vec![
        0, 1, 2, // 第1三角形
        2, 3, 0, // 第2三角形
    ];

    (vertices, indices)
}

/// カメラ正対ビルボード頂点のワールド座標展開（CPU側シミュレーション・テスト用）
#[inline]
pub fn calculate_billboard_vertex(
    center: [f32; 3],
    radius: f32,
    quad_offset: [f32; 2],
    camera_right: [f32; 3],
    camera_up: [f32; 3],
) -> [f32; 3] {
    let dx = quad_offset[0] * radius;
    let dy = quad_offset[1] * radius;

    [
        center[0] + camera_right[0] * dx + camera_up[0] * dy,
        center[1] + camera_right[1] * dx + camera_up[1] * dy,
        center[2] + camera_right[2] * dx + camera_up[2] * dy,
    ]
}

/// クワッド内局所座標 `(u, v) in [-1, 1]^2` に対する円形クリップと球法線の再構成。
///
/// 円外（$u^2 + v^2 > 1.0$）の場合は `None`（シェーダーにおける `discard` 相当）。
/// 円内の場合はカメラ手前方向（+Z）を向く単位法線ベクトル `[u, v, sqrt(1 - u^2 - v^2)]` を返す。
#[inline]
pub fn evaluate_sphere_impostor(uv: [f32; 2]) -> Option<[f32; 3]> {
    let dist_sq = uv[0] * uv[0] + uv[1] * uv[1];
    if dist_sq > 1.0 {
        None
    } else {
        let nz = (1.0 - dist_sq).max(0.0).sqrt();
        Some([uv[0], uv[1], nz])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_lod_tier() {
        let threshold = 65.0;
        let threshold_sq = threshold * threshold;

        // 近距離（しきい値以下）
        assert_eq!(
            classify_lod_tier_sq(10.0 * 10.0, threshold_sq),
            LodTier::Near
        );
        assert_eq!(
            classify_lod_tier_sq(65.0 * 65.0, threshold_sq),
            LodTier::Near
        );

        // 中距離（しきい値超過）
        assert_eq!(
            classify_lod_tier_sq(65.1 * 65.1, threshold_sq),
            LodTier::Mid
        );
        assert_eq!(
            classify_lod_tier_sq(150.0 * 150.0, threshold_sq),
            LodTier::Mid
        );
    }

    #[test]
    fn test_impostor_quad_geometry() {
        let (vertices, indices) = create_impostor_quad();
        assert_eq!(vertices.len(), 4);
        assert_eq!(indices.len(), 6);

        // 4隅の座標の整合性
        assert_eq!(vertices[0].position, [-1.0, -1.0]);
        assert_eq!(vertices[1].position, [1.0, -1.0]);
        assert_eq!(vertices[2].position, [1.0, 1.0]);
        assert_eq!(vertices[3].position, [-1.0, 1.0]);
    }

    #[test]
    fn test_billboard_vertex_offset() {
        let center = [10.0, 20.0, 30.0];
        let radius = 2.0;
        let right = [1.0, 0.0, 0.0];
        let up = [0.0, 1.0, 0.0];

        // 中心
        let p_center = calculate_billboard_vertex(center, radius, [0.0, 0.0], right, up);
        assert_eq!(p_center, center);

        // 右上隅 (+2.0, +2.0)
        let p_top_right = calculate_billboard_vertex(center, radius, [1.0, 1.0], right, up);
        assert_eq!(p_top_right, [12.0, 22.0, 30.0]);
    }

    #[test]
    fn test_sphere_impostor_evaluation() {
        // 中心: 法線は [0, 0, 1]
        let norm_center = evaluate_sphere_impostor([0.0, 0.0]).expect("Center should be inside");
        assert_eq!(norm_center, [0.0, 0.0, 1.0]);

        // 境界上 (1.0, 0.0): 法線は [1, 0, 0]
        let norm_right =
            evaluate_sphere_impostor([1.0, 0.0]).expect("Boundary point should be inside");
        assert!((norm_right[0] - 1.0).abs() < 1e-6);
        assert!(norm_right[2].abs() < 1e-6);

        // 円外 (1.0, 1.0): dist_sq = 2.0 -> discard (None)
        assert!(evaluate_sphere_impostor([1.0, 1.0]).is_none());
        assert!(evaluate_sphere_impostor([0.8, 0.8]).is_none()); // 0.64 + 0.64 = 1.28 > 1.0
    }
}
