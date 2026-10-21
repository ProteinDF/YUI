use super::gpu_types::Vertex;
use std::f32::consts::PI;

/// 緯度経度分割によるマスター球メッシュ生成（半径 1.0）
pub(crate) fn create_sphere_mesh(sectors: usize, stacks: usize) -> (Vec<Vertex>, Vec<u16>) {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();

    for i in 0..=stacks {
        let v = i as f32 / stacks as f32;
        let phi = v * PI;

        for j in 0..=sectors {
            let u = j as f32 / sectors as f32;
            let theta = u * PI * 2.0;

            let x = phi.sin() * theta.cos();
            let y = phi.cos();
            let z = phi.sin() * theta.sin();

            vertices.push(Vertex {
                position: [x, y, z],
                normal: [x, y, z],
            });
        }
    }

    for i in 0..stacks {
        let k1 = (i * (sectors + 1)) as u16;
        let k2 = k1 + sectors as u16 + 1;

        for j in 0..sectors {
            let s1 = k1 + j as u16;
            let s2 = k2 + j as u16;

            // 外側から見て反時計回り（CCW）になる順序（パイプラインは Ccw を表、Back をカリングする）
            if i != 0 {
                indices.push(s1);
                indices.push(s1 + 1);
                indices.push(s2);
            }
            if i != (stacks - 1) {
                indices.push(s1 + 1);
                indices.push(s2 + 1);
                indices.push(s2);
            }
        }
    }

    (vertices, indices)
}

/// マスター円柱メッシュ生成（底面 z=-1.0, 上面 z=1.0, 半径 1.0）
pub(crate) fn create_cylinder_mesh(sectors: usize) -> (Vec<Vertex>, Vec<u16>) {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();

    // 側面
    for i in 0..=sectors {
        let u = i as f32 / sectors as f32;
        let theta = u * PI * 2.0;
        let x = theta.cos();
        let y = theta.sin();

        // 下点 (z = -1)
        vertices.push(Vertex {
            position: [x, y, -1.0],
            normal: [x, y, 0.0],
        });
        // 上点 (z = 1)
        vertices.push(Vertex {
            position: [x, y, 1.0],
            normal: [x, y, 0.0],
        });
    }

    for i in 0..sectors {
        let base = (i * 2) as u16;
        // 外側から見て反時計回り（CCW）になる順序
        indices.push(base);
        indices.push(base + 2);
        indices.push(base + 1);

        indices.push(base + 2);
        indices.push(base + 3);
        indices.push(base + 1);
    }

    (vertices, indices)
}

#[cfg(test)]
mod winding_tests {
    use super::*;
    use crate::viewport::gpu_types::Vertex;

    /// 三角形の反時計回り（CCW）の面法線が、メッシュの外側を向いている三角形の割合を返す。
    /// 面の重心から見て、法線と「重心から軸（原点または z 軸）への外向き方向」の内積が正なら外向き。
    fn outward_fraction(
        vertices: &[Vertex],
        indices: &[u16],
        outward: impl Fn([f32; 3]) -> [f32; 3],
    ) -> f32 {
        let (mut outward_count, mut total) = (0, 0);
        for tri in indices.chunks(3) {
            let p: Vec<[f32; 3]> = tri.iter().map(|&i| vertices[i as usize].position).collect();
            let u = [p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]];
            let v = [p[2][0] - p[0][0], p[2][1] - p[0][1], p[2][2] - p[0][2]];
            let n = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            if n.iter().all(|c| c.abs() < 1e-9) {
                continue; // 退化した三角形（極）
            }
            let c = [
                (p[0][0] + p[1][0] + p[2][0]) / 3.0,
                (p[0][1] + p[1][1] + p[2][1]) / 3.0,
                (p[0][2] + p[1][2] + p[2][2]) / 3.0,
            ];
            let o = outward(c);
            total += 1;
            if n[0] * o[0] + n[1] * o[1] + n[2] * o[2] > 0.0 {
                outward_count += 1;
            }
        }
        outward_count as f32 / total as f32
    }

    #[test]
    fn test_sphere_triangles_face_outward() {
        let (v, i) = create_sphere_mesh(16, 12);
        let f = outward_fraction(&v, &i, |c| c);
        assert!(f > 0.99, "only {f} of the sphere triangles face outward");
    }

    #[test]
    fn test_cylinder_triangles_face_outward() {
        let (v, i) = create_cylinder_mesh(14);
        let f = outward_fraction(&v, &i, |c| [c[0], c[1], 0.0]);
        assert!(f > 0.99, "only {f} of the cylinder triangles face outward");
    }
}
