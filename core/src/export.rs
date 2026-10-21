//! 表示中のビューを SVG（ベクター）として書き出す。
//!
//! 何を描くか（表示・非表示、配色、表示中のモデル）は呼び出し側が決め、`SvgScene` として渡す。
//! ここは、3D の点を 2D に投影し、奥から順に並べて SVG の文字列にするだけで、ファイルには触れない
//! （ブラウザ版でも使えるようにするため。保存は呼び出し側が行う）。
//! 書き出すのは原子（円）、結合（線）、注釈の文字、相互作用の線（破線）と、三角形のメッシュ
//! （リボン、分子表面、CUBE の等値面）。メッシュの三角形は、ひとつずつ塗りつぶした多角形にし、
//! 画面と同じ光で 1 色に陰影をつける（なめらかな陰影のグラデーションは使わない）。

/// 書き出す原子（球）
#[derive(Debug, Clone, PartialEq)]
pub struct SvgAtom {
    pub position: [f32; 3],
    /// 描く球の半径（Å）
    pub radius: f32,
    pub color: [f32; 4],
}

/// 書き出す結合（太さを持つ線分）
#[derive(Debug, Clone, PartialEq)]
pub struct SvgBond {
    pub start: [f32; 3],
    pub end: [f32; 3],
    /// スティックの半径（Å）
    pub radius: f32,
    pub color: [f32; 4],
    /// 破線で描くか（非共有結合相互作用の線）。破線は端を丸めず、間を空ける。
    pub dashed: bool,
}

/// 書き出す注釈の文字
#[derive(Debug, Clone, PartialEq)]
pub struct SvgLabel {
    pub position: [f32; 3],
    pub text: String,
    pub color: [f32; 4],
}

/// 書き出す三角形のメッシュ（リボン、分子表面、CUBE の等値面）。頂点ごとに法線と色（RGBA）を持つ。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SvgMesh {
    pub positions: Vec<[f32; 3]>,
    /// 頂点の法線（外向きの単位ベクトル）
    pub normals: Vec<[f32; 3]>,
    pub colors: Vec<[f32; 4]>,
    /// 3 つずつで 1 枚の三角形
    pub indices: Vec<u32>,
    /// 背面（法線がカメラと反対を向く三角形）を省くか。閉じた不透明な面（分子表面）はファイルを
    /// 小さくするために省く。リボンと半透明の面は、両面が見えるので省かない。
    pub cull_back: bool,
}

/// 書き出す要素の一覧
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SvgScene {
    pub atoms: Vec<SvgAtom>,
    pub bonds: Vec<SvgBond>,
    pub labels: Vec<SvgLabel>,
    /// 三角形のメッシュ（リボン、分子表面、CUBE の等値面）
    pub meshes: Vec<SvgMesh>,
    /// カメラの位置（メッシュの背面を省くのに使う）
    pub eye: [f32; 3],
    /// 背景色
    pub background: [f32; 4],
}

/// 書き出す三角形の数の上限（ファイルが大きくなりすぎないように）。背面を省いたあとの数で数える。
pub const MAX_SVG_TRIANGLES: usize = 500_000;

/// 画面の光の向き（`ViewportPrimitive::prepare` が使う世界座標の固定の向き）と、陰影の係数
/// （`main.wgsl` の `fs_main`: `色 * (0.35 + 0.65 * max(dot(n, l), 0))`）
const LIGHT_DIRECTION: [f32; 3] = [0.5, 0.8, 0.6];
const AMBIENT: f32 = 0.35;
const DIFFUSE: f32 = 0.65;

/// 投影した点（画面の座標と、カメラからの奥行き）
struct Projected {
    x: f32,
    y: f32,
    /// クリップ座標の w（透視投影では、カメラからの視線方向の距離）。大きいほど奥。
    depth: f32,
}

/// `view_proj` は列優先（`view_proj[列][行]`）。`w <= 0`（カメラの背面）の点は `None`。
fn project(m: &[[f32; 4]; 4], p: [f32; 3], width: f32, height: f32) -> Option<Projected> {
    let row = |r: usize| m[0][r] * p[0] + m[1][r] * p[1] + m[2][r] * p[2] + m[3][r];
    let (cx, cy, cw) = (row(0), row(1), row(3));
    if cw <= 0.0 {
        return None;
    }
    Some(Projected {
        x: (cx / cw * 0.5 + 0.5) * width,
        y: (1.0 - (cy / cw * 0.5 + 0.5)) * height,
        depth: cw,
    })
}

/// 奥行き `depth` にある半径 `radius`（Å）の長さが、画面で何ピクセルになるか。
///
/// 透視投影の行列の `y` の行は、視線に垂直な方向 1 Å あたり `f = 1 / tan(fov / 2)` だけクリップ座標が動く
/// （回転は長さを変えないので、`view_proj` の `y` の行の xyz 成分のノルムが `f`）。
/// NDC は `-1..1` が画面の高さ全体なので、`ピクセル = radius * f / w * height / 2`。
pub fn screen_length(m: &[[f32; 4]; 4], radius: f32, depth: f32, height: f32) -> f32 {
    let f = (m[0][1] * m[0][1] + m[1][1] * m[1][1] + m[2][1] * m[2][1]).sqrt();
    radius * f / depth * height * 0.5
}

fn rgb(c: [f32; 4]) -> (u8, u8, u8) {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    (q(c[0]), q(c[1]), q(c[2]))
}

/// XML の文字として書けない文字を置き換える
fn escape_xml(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '&' => "&amp;".to_string(),
            '<' => "&lt;".to_string(),
            '>' => "&gt;".to_string(),
            '"' => "&quot;".to_string(),
            '\'' => "&apos;".to_string(),
            c => c.to_string(),
        })
        .collect()
}

enum Shape {
    Atom {
        x: f32,
        y: f32,
        r: f32,
        c: [f32; 4],
    },
    Bond {
        a: (f32, f32),
        b: (f32, f32),
        w: f32,
        c: [f32; 4],
        dashed: bool,
    },
    /// メッシュの三角形（陰影をつけた色）
    Triangle {
        p: [(f32, f32); 3],
        c: [f32; 4],
    },
}

fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// `scene` のメッシュの三角形を、投影・陰影づけして、不透明なものと半透明なものに分けて加える。
/// 背面を省いたあとの三角形が `MAX_SVG_TRIANGLES` を超えるときは、エラーにする。
fn collect_triangles(
    scene: &SvgScene,
    m: &[[f32; 4]; 4],
    width: f32,
    height: f32,
    opaque: &mut Vec<(f32, Shape)>,
    translucent: &mut Vec<(f32, Shape)>,
) -> Result<(), String> {
    let len = (dot3(LIGHT_DIRECTION, LIGHT_DIRECTION)).sqrt();
    let light = [
        LIGHT_DIRECTION[0] / len,
        LIGHT_DIRECTION[1] / len,
        LIGHT_DIRECTION[2] / len,
    ];
    let mut count = 0usize;
    for mesh in &scene.meshes {
        let n = mesh
            .positions
            .len()
            .min(mesh.normals.len())
            .min(mesh.colors.len());
        let projected: Vec<Option<Projected>> = mesh.positions[..n]
            .iter()
            .map(|&p| project(m, p, width, height))
            .collect();
        for tri in mesh.indices.as_chunks::<3>().0 {
            let ids = [tri[0] as usize, tri[1] as usize, tri[2] as usize];
            if ids.iter().any(|&i| i >= n) {
                continue;
            }
            let (Some(a), Some(b), Some(c)) = (
                projected[ids[0]].as_ref(),
                projected[ids[1]].as_ref(),
                projected[ids[2]].as_ref(),
            ) else {
                continue; // どれかの頂点がカメラの背面にある
            };
            // 面の法線は、頂点の法線の平均
            let mut nrm = [0.0f32; 3];
            let mut center = [0.0f32; 3];
            for &i in &ids {
                for k in 0..3 {
                    nrm[k] += mesh.normals[i][k];
                    center[k] += mesh.positions[i][k] / 3.0;
                }
            }
            let nl = dot3(nrm, nrm).sqrt();
            if nl > 1e-9 {
                nrm = [nrm[0] / nl, nrm[1] / nl, nrm[2] / nl];
            }
            if mesh.cull_back {
                let to_eye = [
                    scene.eye[0] - center[0],
                    scene.eye[1] - center[1],
                    scene.eye[2] - center[2],
                ];
                if dot3(nrm, to_eye) <= 0.0 {
                    continue;
                }
            }
            count += 1;
            if count > MAX_SVG_TRIANGLES {
                return Err(format!(
                    "The surfaces and ribbons have more than {MAX_SVG_TRIANGLES} triangles, which makes too large an SVG. Hide the surface, or use a coarser one (render: surface_grid_spacing in the settings file), or export a PNG."
                ));
            }
            let shade = AMBIENT + DIFFUSE * dot3(nrm, light).max(0.0);
            let mut col = [0.0f32; 4];
            for &i in &ids {
                for (acc, v) in col.iter_mut().zip(mesh.colors[i]) {
                    *acc += v / 3.0;
                }
            }
            let lit = [col[0] * shade, col[1] * shade, col[2] * shade, col[3]];
            let shape = Shape::Triangle {
                p: [(a.x, a.y), (b.x, b.y), (c.x, c.y)],
                c: lit,
            };
            let depth = (a.depth + b.depth + c.depth) / 3.0;
            if lit[3] < 0.999 {
                translucent.push((depth, shape));
            } else {
                opaque.push((depth, shape));
            }
        }
    }
    Ok(())
}

/// `scene` を `view_proj`・`width`・`height`（ピクセル）で見たときの SVG を文字列で返す。
///
/// 原子と結合は、まとめて奥（`w` が大きい）から順に並べて描く（画家のアルゴリズム）。
/// 結合の奥行きは、両端の `w` の平均。カメラの背面にある原子、片方の端が背面にある結合は描かない。
pub fn render_svg(
    scene: &SvgScene,
    view_proj: [[f32; 4]; 4],
    width: f32,
    height: f32,
) -> Result<String, String> {
    if !(width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0) {
        return Err(format!("Invalid image size: {width} x {height}"));
    }
    let m = &view_proj;
    let mut shapes: Vec<(f32, Shape)> = Vec::new();

    for atom in &scene.atoms {
        if let Some(p) = project(m, atom.position, width, height) {
            let r = screen_length(m, atom.radius, p.depth, height).max(0.5);
            shapes.push((
                p.depth,
                Shape::Atom {
                    x: p.x,
                    y: p.y,
                    r,
                    c: atom.color,
                },
            ));
        }
    }
    for bond in &scene.bonds {
        if let (Some(a), Some(b)) = (
            project(m, bond.start, width, height),
            project(m, bond.end, width, height),
        ) {
            let depth = (a.depth + b.depth) * 0.5;
            let w = (screen_length(m, bond.radius, depth, height) * 2.0).max(0.5);
            shapes.push((
                depth,
                Shape::Bond {
                    a: (a.x, a.y),
                    b: (b.x, b.y),
                    w,
                    c: bond.color,
                    dashed: bond.dashed,
                },
            ));
        }
    }
    // メッシュの三角形。不透明なものは原子・結合といっしょに並べ、半透明なものは、画面と同じように、
    // それらの上に、奥から順に重ねる
    let mut translucent: Vec<(f32, Shape)> = Vec::new();
    collect_triangles(scene, m, width, height, &mut shapes, &mut translucent)?;
    let far_to_near = |a: &(f32, Shape), b: &(f32, Shape)| {
        b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal)
    };
    shapes.sort_by(far_to_near);
    translucent.sort_by(far_to_near);
    shapes.extend(translucent);

    let (br, bg, bb) = rgb(scene.background);
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {width} {height}\" width=\"{width}\" height=\"{height}\">\n  <rect width=\"100%\" height=\"100%\" fill=\"rgb({br},{bg},{bb})\"/>\n"
    );
    for (_, shape) in &shapes {
        match shape {
            Shape::Triangle { p, c } => {
                let (cr, cg, cb) = rgb(*c);
                let points = format!(
                    "{:.1},{:.1} {:.1},{:.1} {:.1},{:.1}",
                    p[0].0, p[0].1, p[1].0, p[1].1, p[2].0, p[2].1
                );
                if c[3] < 0.999 {
                    // 半透明は、重なった縁が濃くならないよう、輪郭の線を引かない
                    svg.push_str(&format!(
                        "  <polygon points=\"{points}\" fill=\"#{cr:02x}{cg:02x}{cb:02x}\" fill-opacity=\"{:.2}\"/>\n",
                        c[3]
                    ));
                } else {
                    // 隣の多角形との間に、細い隙間が見えないよう、同じ色の細い線を引く
                    svg.push_str(&format!(
                        "  <polygon points=\"{points}\" fill=\"#{cr:02x}{cg:02x}{cb:02x}\" stroke=\"#{cr:02x}{cg:02x}{cb:02x}\" stroke-width=\"0.6\" stroke-linejoin=\"round\"/>\n"
                    ));
                }
            }
            Shape::Atom { x, y, r, c } => {
                let (cr, cg, cb) = rgb(*c);
                svg.push_str(&format!(
                    "  <circle cx=\"{x:.2}\" cy=\"{y:.2}\" r=\"{r:.2}\" fill=\"rgb({cr},{cg},{cb})\" fill-opacity=\"{:.2}\" stroke=\"#000000\" stroke-width=\"0.5\"/>\n",
                    c[3]
                ));
            }
            Shape::Bond { a, b, w, c, dashed } => {
                let (cr, cg, cb) = rgb(*c);
                // 破線は、線の太さに比例した長さ（細すぎても見えるよう、最小の長さをもつ）で、端は丸めない
                let style = if *dashed {
                    let dash = (w * 3.0).max(4.0);
                    let gap = (w * 2.0).max(3.0);
                    format!("stroke-dasharray=\"{dash:.2} {gap:.2}\" stroke-linecap=\"butt\"")
                } else {
                    "stroke-linecap=\"round\"".to_string()
                };
                svg.push_str(&format!(
                    "  <line x1=\"{:.2}\" y1=\"{:.2}\" x2=\"{:.2}\" y2=\"{:.2}\" stroke=\"rgb({cr},{cg},{cb})\" stroke-opacity=\"{:.2}\" stroke-width=\"{w:.2}\" {style}/>\n",
                    a.0, a.1, b.0, b.1, c[3]
                ));
            }
        }
    }
    // 注釈の文字は、いちばん手前に描く
    for label in &scene.labels {
        if let Some(p) = project(m, label.position, width, height) {
            let (cr, cg, cb) = rgb(label.color);
            svg.push_str(&format!(
                "  <text x=\"{:.2}\" y=\"{:.2}\" fill=\"rgb({cr},{cg},{cb})\" font-family=\"sans-serif\" font-size=\"12\" text-anchor=\"middle\" font-weight=\"bold\">{}</text>\n",
                p.x,
                p.y - 15.0,
                escape_xml(&label.text)
            ));
        }
    }
    svg.push_str("</svg>\n");
    Ok(svg)
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: f32 = 800.0;
    const H: f32 = 600.0;
    /// 縦の視野角 90°（f = 1）
    const F: f32 = 1.0;

    /// カメラは原点にあって -z を向く。`view_proj[列][行]`。z = -d の点で w = d。
    fn view_proj() -> [[f32; 4]; 4] {
        let aspect = W / H;
        [
            [F / aspect, 0.0, 0.0, 0.0],
            [0.0, F, 0.0, 0.0],
            [0.0, 0.0, -1.0, -1.0],
            [0.0, 0.0, -0.2, 0.0],
        ]
    }

    fn atom(z: f32, radius: f32) -> SvgAtom {
        SvgAtom {
            position: [0.0, 0.0, z],
            radius,
            color: [0.0, 1.0, 0.0, 1.0],
        }
    }

    fn count(svg: &str, tag: &str) -> usize {
        svg.matches(&format!("<{tag} ")).count()
    }

    /// z = `z` の平面にある、1 枚の三角形のメッシュ。法線は `normal`、色は `color`（RGBA）
    fn triangle(z: f32, normal: [f32; 3], color: [f32; 4], cull_back: bool) -> SvgMesh {
        SvgMesh {
            positions: vec![[-1.0, -1.0, z], [1.0, -1.0, z], [0.0, 1.0, z]],
            normals: vec![normal; 3],
            colors: vec![color; 3],
            indices: vec![0, 1, 2],
            cull_back,
        }
    }

    fn polygons(svg: &str) -> Vec<&str> {
        svg.lines().filter(|l| l.contains("<polygon")).collect()
    }

    #[test]
    fn test_a_mesh_triangle_is_projected_and_lit_like_the_screen() {
        // カメラは原点で -z を向く。法線が +z（カメラ向き）の三角形を、深さ 10 に置く
        let scene = SvgScene {
            meshes: vec![triangle(
                -10.0,
                [0.0, 0.0, 1.0],
                [1.0, 0.0, 0.0, 1.0],
                false,
            )],
            eye: [0.0, 0.0, 0.0],
            ..Default::default()
        };
        let svg = render_svg(&scene, view_proj(), W, H).unwrap();
        let polys = polygons(&svg);
        assert_eq!(polys.len(), 1, "{svg}");
        // 頂点 (-1,-1) は、x = (-1 * 0.75 / 10 * 0.5 + 0.5) * 800 = 370、y = (1 - (-1/10*0.5+0.5)) * 600 = 330
        assert!(
            polys[0].contains("points=\"370.0,330.0 430.0,330.0 400.0,270.0\""),
            "{}",
            polys[0]
        );
        // 光は (0.5, 0.8, 0.6)/√1.25 で、法線 +z との内積は 0.6 / 1.118 = 0.5367。
        // 陰影 = 0.35 + 0.65 * 0.5367 = 0.6989。赤 255 * 0.6989 = 178.2 → b2
        assert!(polys[0].contains("fill=\"#b20000\""), "{}", polys[0]);
        // 不透明な三角形は、隙間が見えないよう、同じ色の細い線を引く。半透明には引かない
        assert!(polys[0].contains("stroke=\"#b20000\""));
        let glass = SvgScene {
            meshes: vec![triangle(
                -10.0,
                [0.0, 0.0, 1.0],
                [1.0, 0.0, 0.0, 0.5],
                false,
            )],
            ..Default::default()
        };
        let svg = render_svg(&glass, view_proj(), W, H).unwrap();
        let p = polygons(&svg)[0].to_string();
        assert!(
            p.contains("fill-opacity=\"0.50\"") && !p.contains("stroke"),
            "{p}"
        );
        // 光と反対を向く面も、環境光の 0.35 で描く（画面と同じ。法線は裏返さない）
        let dark = SvgScene {
            meshes: vec![triangle(
                -10.0,
                [0.0, 0.0, -1.0],
                [1.0, 0.0, 0.0, 1.0],
                false,
            )],
            ..Default::default()
        };
        // 255 * 0.35 = 89.25 → 59
        assert!(
            render_svg(&dark, view_proj(), W, H)
                .unwrap()
                .contains("#590000")
        );
    }

    #[test]
    fn test_back_faces_are_dropped_only_when_asked_and_behind_the_camera_is_dropped() {
        let eye = [0.0, 0.0, 0.0];
        let back = |cull| SvgScene {
            // 法線 -z は、-z 側にいるカメラから見て、反対を向く（背面）
            meshes: vec![triangle(
                -10.0,
                [0.0, 0.0, -1.0],
                [0.0, 1.0, 0.0, 1.0],
                cull,
            )],
            eye,
            ..Default::default()
        };
        assert_eq!(
            polygons(&render_svg(&back(true), view_proj(), W, H).unwrap()).len(),
            0
        );
        assert_eq!(
            polygons(&render_svg(&back(false), view_proj(), W, H).unwrap()).len(),
            1
        );
        // カメラの背面（z > 0）にある三角形は、描かない
        let behind = SvgScene {
            meshes: vec![triangle(10.0, [0.0, 0.0, 1.0], [0.0, 1.0, 0.0, 1.0], false)],
            ..Default::default()
        };
        assert_eq!(
            polygons(&render_svg(&behind, view_proj(), W, H).unwrap()).len(),
            0
        );
        // 範囲外の添字や、足りない頂点のデータは、無視する（止まらない）
        let broken = SvgScene {
            meshes: vec![SvgMesh {
                positions: vec![[0.0, 0.0, -5.0]],
                normals: vec![[0.0, 0.0, 1.0]],
                colors: vec![[1.0; 4]],
                indices: vec![0, 1, 2, 7],
                cull_back: false,
            }],
            ..Default::default()
        };
        assert_eq!(
            polygons(&render_svg(&broken, view_proj(), W, H).unwrap()).len(),
            0
        );
    }

    #[test]
    fn test_translucent_triangles_are_drawn_over_the_opaque_shapes_far_to_near() {
        let scene = SvgScene {
            // 半透明の面（手前 z = -5 と、奥 z = -20）と、不透明な原子（その中間 z = -10）
            meshes: vec![
                triangle(-5.0, [0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 0.5], false),
                triangle(-20.0, [0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 0.5], false),
            ],
            atoms: vec![atom(-10.0, 1.0)],
            ..Default::default()
        };
        let svg = render_svg(&scene, view_proj(), W, H).unwrap();
        let circle = svg.find("<circle").unwrap();
        let polys: Vec<usize> = svg.match_indices("<polygon").map(|(i, _)| i).collect();
        assert_eq!(polys.len(), 2);
        // 半透明の三角形は、どちらも原子より後（上）。その中では、奥（赤）が先、手前（青）が後
        assert!(polys[0] > circle && polys[1] > circle, "{svg}");
        // 奥（z = -20、赤）が先、手前（z = -5、青）が後
        let red = svg.find("#b20000").unwrap();
        let blue = svg.find("#0000b2").unwrap();
        assert!(red < blue, "{svg}");
    }

    #[test]
    fn test_too_many_triangles_is_an_error_with_a_hint() {
        let n = MAX_SVG_TRIANGLES + 1;
        let mesh = SvgMesh {
            positions: vec![[-1.0, -1.0, -10.0], [1.0, -1.0, -10.0], [0.0, 1.0, -10.0]],
            normals: vec![[0.0, 0.0, 1.0]; 3],
            colors: vec![[1.0; 4]; 3],
            indices: (0..n).flat_map(|_| [0u32, 1, 2]).collect(),
            cull_back: false,
        };
        let scene = SvgScene {
            meshes: vec![mesh],
            ..Default::default()
        };
        let err = render_svg(&scene, view_proj(), W, H).unwrap_err();
        assert!(
            err.contains("surface_grid_spacing") && err.contains("PNG"),
            "{err}"
        );
    }

    #[test]
    fn test_one_atom_is_centered_with_the_perspective_radius() {
        let scene = SvgScene {
            atoms: vec![atom(-10.0, 2.0)],
            ..Default::default()
        };
        let svg = render_svg(&scene, view_proj(), W, H).unwrap();
        // 半径 = 2 * f / w * H / 2 = 2 * 1 / 10 * 300 = 60
        assert!(
            svg.contains("cx=\"400.00\" cy=\"300.00\" r=\"60.00\""),
            "{svg}"
        );
        // 2 倍遠いと、半分の大きさ
        let far = SvgScene {
            atoms: vec![atom(-20.0, 2.0)],
            ..Default::default()
        };
        assert!(
            render_svg(&far, view_proj(), W, H)
                .unwrap()
                .contains("r=\"30.00\"")
        );
    }

    #[test]
    fn test_farther_shapes_are_written_first() {
        let mut near = atom(-5.0, 1.0);
        near.color = [1.0, 0.0, 0.0, 1.0];
        let mut far = atom(-30.0, 1.0);
        far.color = [0.0, 0.0, 1.0, 1.0];
        // 手前を先に入れても、奥が先に書かれる
        let scene = SvgScene {
            atoms: vec![near, far],
            // 中間の奥行きの結合
            bonds: vec![SvgBond {
                start: [-1.0, 0.0, -15.0],
                end: [1.0, 0.0, -15.0],
                radius: 0.2,
                color: [1.0, 1.0, 1.0, 1.0],
                dashed: false,
            }],
            ..Default::default()
        };
        let svg = render_svg(&scene, view_proj(), W, H).unwrap();
        let blue = svg.find("rgb(0,0,255)").unwrap();
        let line = svg.find("<line ").unwrap();
        let red = svg.find("rgb(255,0,0)").unwrap();
        assert!(blue < line && line < red, "{svg}");
    }

    #[test]
    fn test_shapes_behind_the_camera_are_dropped() {
        let scene = SvgScene {
            atoms: vec![atom(-10.0, 1.0), atom(5.0, 1.0)],
            bonds: vec![SvgBond {
                start: [0.0, 0.0, -10.0],
                end: [0.0, 0.0, 5.0], // 片方の端が背面
                radius: 0.1,
                color: [1.0; 4],
                dashed: false,
            }],
            labels: vec![SvgLabel {
                position: [0.0, 0.0, 5.0],
                text: "behind".into(),
                color: [1.0; 4],
            }],
            ..Default::default()
        };
        let svg = render_svg(&scene, view_proj(), W, H).unwrap();
        assert_eq!(count(&svg, "circle"), 1);
        assert_eq!(count(&svg, "line"), 0);
        assert!(!svg.contains("behind"));
    }

    #[test]
    fn test_output_is_well_formed_and_counts_match() {
        let scene = SvgScene {
            atoms: vec![atom(-10.0, 1.0), atom(-12.0, 1.0), atom(-14.0, 1.0)],
            bonds: vec![SvgBond {
                start: [0.0, 0.0, -10.0],
                end: [0.0, 0.0, -12.0],
                radius: 0.1,
                color: [0.7, 0.7, 0.7, 1.0],
                dashed: false,
            }],
            labels: vec![SvgLabel {
                position: [0.0, 0.0, -10.0],
                text: "A<B & \"C\"".into(),
                color: [1.0; 4],
            }],
            background: [0.1, 0.1, 0.1, 1.0],
            ..Default::default()
        };
        let svg = render_svg(&scene, view_proj(), W, H).unwrap();
        assert!(svg.starts_with("<svg "));
        assert!(svg.trim_end().ends_with("</svg>"));
        assert_eq!(count(&svg, "circle"), 3);
        assert_eq!(count(&svg, "line"), 1);
        assert_eq!(count(&svg, "text"), 1);
        assert!(svg.contains("A&lt;B &amp; &quot;C&quot;"));
        assert!(svg.contains("fill=\"rgb(26,26,26)\""));
        // 文字列中の < は、タグの開始と一致する（本文に生の < や & が残らない）
        assert_eq!(svg.matches('<').count(), svg.matches('>').count());
    }

    #[test]
    fn test_invalid_size_is_an_error() {
        assert!(render_svg(&SvgScene::default(), view_proj(), 0.0, H).is_err());
        assert!(render_svg(&SvgScene::default(), view_proj(), W, f32::NAN).is_err());
    }

    #[test]
    fn test_dashed_lines_have_a_dash_pattern_and_flat_ends() {
        let line = |dashed: bool| SvgBond {
            start: [-1.0, 0.0, -10.0],
            end: [1.0, 0.0, -10.0],
            radius: 0.05,
            color: [1.0, 0.85, 0.1, 1.0],
            dashed,
        };
        let svg = |dashed: bool| {
            let scene = SvgScene {
                bonds: vec![line(dashed)],
                ..Default::default()
            };
            render_svg(&scene, view_proj(), W, H).unwrap()
        };
        let solid = svg(false);
        assert!(solid.contains("stroke-linecap=\"round\"") && !solid.contains("dasharray"));
        let dashed = svg(true);
        // 半径 0.05 Å、奥行き 10 → 太さは 2 * 0.05 * 1 / 10 * 300 = 3 px。最小の長さ（4 と 3）が効く
        assert!(dashed.contains("stroke-width=\"3.00\""), "{dashed}");
        assert!(
            dashed.contains("stroke-dasharray=\"9.00 6.00\""),
            "{dashed}"
        );
        assert!(dashed.contains("stroke-linecap=\"butt\"") && !dashed.contains("round"));
        // 細い線では、最小の長さで、見えなくならない
        let thin = SvgScene {
            bonds: vec![SvgBond {
                radius: 0.005,
                ..line(true)
            }],
            ..Default::default()
        };
        let thin = render_svg(&thin, view_proj(), W, H).unwrap();
        assert!(thin.contains("stroke-dasharray=\"4.00 3.00\""), "{thin}");
    }
}
