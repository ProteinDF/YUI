//! 3Dグラフィックス用の軽量なベクトル・行列計算モジュール (column-major for WGSL)

pub type Vec3 = [f32; 3];
pub type Mat4 = [f32; 16];

#[inline]
pub fn vec3_sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

#[inline]
#[allow(dead_code)]
pub fn vec3_add(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

#[inline]
pub fn vec3_scale(v: Vec3, s: f32) -> Vec3 {
    [v[0] * s, v[1] * s, v[2] * s]
}

#[inline]
pub fn vec3_dot(a: Vec3, b: Vec3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

#[inline]
pub fn vec3_cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

#[inline]
pub fn vec3_length(v: Vec3) -> f32 {
    vec3_dot(v, v).sqrt()
}

#[inline]
pub fn vec3_normalize(v: Vec3) -> Vec3 {
    let len = vec3_length(v);
    if len > 1e-6 {
        vec3_scale(v, 1.0 / len)
    } else {
        [0.0, 0.0, 0.0]
    }
}

/// 4x4 行列の乗算 (a * b, 列優先 column-major)
pub fn mat4_mul(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut out = [0.0; 16];
    for col in 0..4 {
        for row in 0..4 {
            let mut sum = 0.0;
            for k in 0..4 {
                sum += a[k * 4 + row] * b[col * 4 + k];
            }
            out[col * 4 + row] = sum;
        }
    }
    out
}

/// 右手系ビュー行列 (LookAt)
pub fn look_at_rh(eye: Vec3, target: Vec3, up: Vec3) -> Mat4 {
    let f = vec3_normalize(vec3_sub(target, eye));
    let s = vec3_normalize(vec3_cross(f, up));
    let u = vec3_cross(s, f);

    [
        s[0],
        u[0],
        -f[0],
        0.0,
        s[1],
        u[1],
        -f[1],
        0.0,
        s[2],
        u[2],
        -f[2],
        0.0,
        -vec3_dot(s, eye),
        -vec3_dot(u, eye),
        vec3_dot(f, eye),
        1.0,
    ]
}

/// 右手系透視投影行列 (wgpu: depth [0, 1])
pub fn perspective_rh(fov_y_rad: f32, aspect: f32, z_near: f32, z_far: f32) -> Mat4 {
    let f = 1.0 / (fov_y_rad * 0.5).tan();
    let range_inv = 1.0 / (z_near - z_far);

    [
        f / aspect,
        0.0,
        0.0,
        0.0,
        0.0,
        f,
        0.0,
        0.0,
        0.0,
        0.0,
        z_far * range_inv,
        -1.0,
        0.0,
        0.0,
        z_near * z_far * range_inv,
        0.0,
    ]
}

/// ビューポート内のスクリーン座標（左上原点 [0, bounds.width], [0, bounds.height]）から、
/// ワールド空間におけるレイの始点 `origin` と正規化方向 `direction` を計算する。
pub fn screen_to_world_ray(
    cursor_pos: [f32; 2],
    viewport_size: [f32; 2],
    camera: &yui_core::state::CameraState,
) -> (Vec3, Vec3) {
    let width = viewport_size[0].max(1.0);
    let height = viewport_size[1].max(1.0);

    // 正規化デバイス座標 (NDC): x in [-1, 1], y in [-1, 1] (yは上が正)
    let ndc_x = (2.0 * cursor_pos[0] / width) - 1.0;
    let ndc_y = 1.0 - (2.0 * cursor_pos[1] / height);

    let aspect = width / height;
    let fov_rad = camera.fov.to_radians();
    let half_height = (fov_rad * 0.5).tan();
    let half_width = half_height * aspect;

    // カメラの基底ベクトル（LookAt と整合）
    let forward = vec3_normalize(vec3_sub(camera.target, camera.eye));
    let right = vec3_normalize(vec3_cross(forward, camera.up));
    let up = vec3_cross(right, forward);

    // レイ方向 = forward + ndc_x * half_width * right + ndc_y * half_height * up
    let ray_dir = [
        forward[0] + ndc_x * half_width * right[0] + ndc_y * half_height * up[0],
        forward[1] + ndc_x * half_width * right[1] + ndc_y * half_height * up[1],
        forward[2] + ndc_x * half_width * right[2] + ndc_y * half_height * up[2],
    ];

    (camera.eye, vec3_normalize(ray_dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vec3_math() {
        let a = [1.0, 0.0, 0.0];
        let b = [0.0, 1.0, 0.0];
        assert_eq!(vec3_cross(a, b), [0.0, 0.0, 1.0]);
        assert_eq!(vec3_dot(a, b), 0.0);
        assert_eq!(vec3_length(a), 1.0);
    }

    #[test]
    fn test_mat4_mul_identity() {
        #[rustfmt::skip]
        let ident = [
            1.0, 0.0, 0.0, 0.0,
            0.0, 1.0, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
            0.0, 0.0, 0.0, 1.0,
        ];
        let res = mat4_mul(&ident, &ident);
        assert_eq!(res, ident);
    }

    #[test]
    fn test_screen_to_world_ray() {
        let camera = yui_core::state::CameraState {
            eye: [0.0, 0.0, 10.0],
            target: [0.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            fov: 45.0,
        };
        let size = [800.0, 600.0];

        // 中心 (400, 300) -> レイ方向は [0, 0, -1] と一致するはず
        let (origin, dir_center) = screen_to_world_ray([400.0, 300.0], size, &camera);
        assert_eq!(origin, [0.0, 0.0, 10.0]);
        assert!((dir_center[0] - 0.0).abs() < 1e-5);
        assert!((dir_center[1] - 0.0).abs() < 1e-5);
        assert!((dir_center[2] - (-1.0)).abs() < 1e-5);

        // 右上 (800, 0) -> x > 0, y > 0, z < 0
        let (_, dir_tr) = screen_to_world_ray([800.0, 0.0], size, &camera);
        assert!(dir_tr[0] > 0.0);
        assert!(dir_tr[1] > 0.0);
        assert!(dir_tr[2] < 0.0);

        // 左下 (0, 600) -> x < 0, y < 0, z < 0
        let (_, dir_bl) = screen_to_world_ray([0.0, 600.0], size, &camera);
        assert!(dir_bl[0] < 0.0);
        assert!(dir_bl[1] < 0.0);
        assert!(dir_bl[2] < 0.0);
    }
}
