//! 視錐台平面抽出および球・バウンディングボリュームの交差判定モジュール。
//!
//! architecture.md 4.2節「GPU駆動レンダリング」の視錐台カリング（Frustum Culling）に基づき、
//! カメラの view_proj 行列から 6 平面（Gribb-Hartmann 法、wgpu NDC 深度 [0, 1] 適合）を
//! 抽出し、球（原子・結合）との交差判定を行います。
//! この CPU 側ロジックは単体テストで検証され、WGSL Compute Shader 実装の数学的基準となります。

use crate::math::Mat4;

/// 3次元空間における平面（Ax + By + Cz + D = 0）
///
/// 法線 (A, B, C) は単位ベクトルとして正規化されており、
/// 任意の点 P に対する符号付き距離は `A*Px + B*Py + C*Pz + D` で求められます。
/// 平面の内側（視錐台内部方向）を法線の正の向きと定義します。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrustumPlane {
    pub normal: [f32; 3],
    pub d: f32,
}

impl FrustumPlane {
    /// 平面方程式の係数 (a, b, c, d) から法線を正規化した平面を作成する。
    pub fn from_coefficients(a: f32, b: f32, c: f32, d: f32) -> Self {
        let length = (a * a + b * b + c * c).sqrt();
        if length > 1e-6 {
            let inv = 1.0 / length;
            Self {
                normal: [a * inv, b * inv, c * inv],
                d: d * inv,
            }
        } else {
            Self {
                normal: [0.0, 0.0, 0.0],
                d: 0.0,
            }
        }
    }

    /// 点 `p` との符号付き距離を計算する（正: 内側、0: 平面上、負: 外側）。
    #[inline]
    pub fn distance_to_point(&self, p: [f32; 3]) -> f32 {
        self.normal[0] * p[0] + self.normal[1] * p[1] + self.normal[2] * p[2] + self.d
    }

    /// WGSL Uniform / Storage バッファ用の `[f32; 4]` 配列として取得する。
    #[inline]
    #[allow(dead_code)]
    pub fn as_vec4(&self) -> [f32; 4] {
        [self.normal[0], self.normal[1], self.normal[2], self.d]
    }
}

/// 視錐台の 6 平面（Left, Right, Bottom, Top, Near, Far）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frustum {
    pub planes: [FrustumPlane; 6],
}

impl Frustum {
    /// 4x4 view_proj 行列（列優先 column-major）から 6 つの視錐台平面を抽出する。
    ///
    /// wgpu (Metal / Vulkan / DirectX 12) の NDC 深度空間 `[0, 1]` に適合:
    /// - Left:   x' + w' >= 0
    /// - Right:  w' - x' >= 0
    /// - Bottom: y' + w' >= 0
    /// - Top:    w' - y' >= 0
    /// - Near:   z' >= 0
    /// - Far:    w' - z' >= 0
    pub fn from_view_proj(m: &Mat4) -> Self {
        // 列優先行列 m において、クリップ空間座標 (x', y', z', w') の各成分の行ベクトル係数:
        // row_0: [m[0], m[4], m[8],  m[12]] -> x'
        // row_1: [m[1], m[5], m[9],  m[13]] -> y'
        // row_2: [m[2], m[6], m[10], m[14]] -> z'
        // row_3: [m[3], m[7], m[11], m[15]] -> w'

        // 0: Left (w' + x' >= 0)
        let left =
            FrustumPlane::from_coefficients(m[3] + m[0], m[7] + m[4], m[11] + m[8], m[15] + m[12]);

        // 1: Right (w' - x' >= 0)
        let right =
            FrustumPlane::from_coefficients(m[3] - m[0], m[7] - m[4], m[11] - m[8], m[15] - m[12]);

        // 2: Bottom (w' + y' >= 0)
        let bottom =
            FrustumPlane::from_coefficients(m[3] + m[1], m[7] + m[5], m[11] + m[9], m[15] + m[13]);

        // 3: Top (w' - y' >= 0)
        let top =
            FrustumPlane::from_coefficients(m[3] - m[1], m[7] - m[5], m[11] - m[9], m[15] - m[13]);

        // 4: Near (z' >= 0, wgpu depth is [0, 1])
        let near = FrustumPlane::from_coefficients(m[2], m[6], m[10], m[14]);

        // 5: Far (w' - z' >= 0)
        let far =
            FrustumPlane::from_coefficients(m[3] - m[2], m[7] - m[6], m[11] - m[10], m[15] - m[14]);

        Self {
            planes: [left, right, bottom, top, near, far],
        }
    }

    /// 球（中心 `center`、半径 `radius`）が視錐台と交差している（可視である）かを判定する。
    ///
    /// 6 つの平面のうち 1 つでも距離が `-radius` 未満であれば完全に外側（不可視）。
    #[inline]
    pub fn intersects_sphere(&self, center: [f32; 3], radius: f32) -> bool {
        for plane in &self.planes {
            if plane.distance_to_point(center) < -radius {
                return false;
            }
        }
        true
    }

    /// WGSL の `array<vec4<f32>, 6>` 互換の平面パラメータ配列を取得する。
    #[allow(dead_code)]
    #[allow(clippy::wrong_self_convention)] // Frustum の参照からシェーダー用配列への変換インターフェースとして&selfを維持
    pub fn to_shader_planes(&self) -> [[f32; 4]; 6] {
        [
            self.planes[0].as_vec4(),
            self.planes[1].as_vec4(),
            self.planes[2].as_vec4(),
            self.planes[3].as_vec4(),
            self.planes[4].as_vec4(),
            self.planes[5].as_vec4(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::{look_at_rh, mat4_mul, perspective_rh};

    #[test]
    fn test_frustum_sphere_visibility() {
        // カメラ設定: 原点 [0, 0, 10] から -Z 方向 [0, 0, 0] を向く
        let eye = [0.0, 0.0, 10.0];
        let target = [0.0, 0.0, 0.0];
        let up = [0.0, 1.0, 0.0];
        let view = look_at_rh(eye, target, up);
        // fov 90度（半角45度、アスペクト比 1.0、z_near = 1.0, z_far = 100.0）
        let proj = perspective_rh(std::f32::consts::FRAC_PI_2, 1.0, 1.0, 100.0);
        let view_proj = mat4_mul(&proj, &view);

        let frustum = Frustum::from_view_proj(&view_proj);

        // 1. 明らかに視錐台の中心付近にある球（可視）
        assert!(frustum.intersects_sphere([0.0, 0.0, 5.0], 0.5));
        assert!(frustum.intersects_sphere([0.0, 0.0, 0.0], 0.5));

        // 2. カメラの背後にある球（Near平面の外側: 不可視）
        assert!(!frustum.intersects_sphere([0.0, 0.0, 12.0], 0.5));

        // 3. Far 平面のさらに向こう側にある球（不可視）
        assert!(!frustum.intersects_sphere([0.0, 0.0, -110.0], 0.5));

        // 4. 明らかに右側・左側・上側・下側の外にある球（不可視）
        assert!(!frustum.intersects_sphere([50.0, 0.0, 5.0], 0.5)); // Right外
        assert!(!frustum.intersects_sphere([-50.0, 0.0, 5.0], 0.5)); // Left外
        assert!(!frustum.intersects_sphere([0.0, 50.0, 5.0], 0.5)); // Top外
        assert!(!frustum.intersects_sphere([0.0, -50.0, 5.0], 0.5)); // Bottom外

        // 5. 境界付近のケース:
        // z = 5.0 において、eye からの距離は 5.0。fov 90度（半角45度）なので、視野幅の限界は約 5.0。
        // [5.5, 0.0, 5.0] は中心が視野外だが、半径 1.0 の球なら視野と交差する
        assert!(
            frustum.intersects_sphere([5.5, 0.0, 5.0], 1.0),
            "Sphere intersecting right plane should be visible"
        );
        // 同じ中心 [5.5, 0.0, 5.0] で半径 0.1 なら視野の外（不可視）
        assert!(
            !frustum.intersects_sphere([5.5, 0.0, 5.0], 0.1),
            "Small sphere outside right plane should be culled"
        );
    }
}
