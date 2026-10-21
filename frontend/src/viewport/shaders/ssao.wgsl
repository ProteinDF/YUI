// SSAO（画面空間の環境遮蔽）。シーンを描いたあとの、別の描画パスで、深度テクスチャだけを読み、
// 隅や隙間を暗くする係数を求める。2 段: (1) `fs_ao` が、係数を、遮蔽の専用のテクスチャに描く
// （ピクセルごとに探す向きをずらすので、そのままだと細かい粒が残る）。(2) `fs_blur` が、深度が近い
// 隣のピクセルだけを混ぜて（輪郭をまたがない）粒をなじませ、乗算のブレンド（出力 × 今の色）で重ねる。
// 色は読まないので、分子図の外（ほかの UI）や背景（深度 1.0）には影響しない。
// 設定の render.ssao_strength が 0 のときは、このパスは動かない。

struct SsaoUniforms {
    // 投影の near と far（Å）。線形の奥行きを求める
    near: f32,
    far: f32,
    // 縦・横の視野角の半分の tan（横 = 縦 × 縦横比）
    tan_half_y: f32,
    tan_half_x: f32,
    // 分子図の矩形（物理ピクセル）: x, y, 幅, 高さ
    pane: vec4<f32>,
    // 遮蔽を探す半径（Å）と、強さ（0〜1）
    radius: f32,
    strength: f32,
    _pad: vec2<f32>,
};

// 深度テクスチャ（Depth32Float）を、フィルターなしの float として読む。`texture_depth_2d` からの
// `textureLoad` は、GL バックエンド（ブラウザの WebGL2 や Mesa のソフトウェア描画）で使えない。
@group(0) @binding(0)
var depth_tex: texture_2d<f32>;
@group(0) @binding(1)
var<uniform> u: SsaoUniforms;
// `fs_ao` が描いた遮蔽の係数（`fs_blur` だけが読む。`fs_ao` のパスでは、これは描き先なので束ねない）
@group(0) @binding(2)
var ao_tex: texture_2d<f32>;

const SAMPLES: i32 = 24;
// 面の向きとのなす角の sin（法線方向の成分 / 距離）が、この範囲で、遮蔽の度合いが 0 から 1 になる。
// 下限より小さい（ほぼ面と平行か、面の下にある）点は遮蔽しないので、平らな面や出っ張りは暗くならない。
const FACING_MIN: f32 = 0.1;
const FACING_MAX: f32 = 0.5;
// 遮蔽の割合に掛ける倍率（深い隙間で、強さ 1 のとき、かなり暗くする）
const GAIN: f32 = 3.0;
// 探す半径の画面上の大きさ（ピクセル）の範囲
const MIN_RADIUS_PX: f32 = 2.0;
const MAX_RADIUS_PX: f32 = 96.0;
const GOLDEN_ANGLE: f32 = 2.3999632;

@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> @builtin(position) vec4<f32> {
    var pos = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    return vec4<f32>(pos[idx], 0.0, 1.0);
}

// 深度バッファの値（0..1）から、カメラからの視線方向の距離（Å）
fn linear_depth(d: f32) -> f32 {
    return u.near * u.far / (u.far - d * (u.far - u.near));
}

fn load_depth(px: vec2<i32>) -> f32 {
    let dims = vec2<i32>(textureDimensions(depth_tex));
    let c = clamp(px, vec2<i32>(0, 0), dims - vec2<i32>(1, 1));
    return textureLoad(depth_tex, c, 0).r;
}

// ピクセルの視点座標系（右手系、-z が前）の位置。背景（深度 1.0）なら w = 0
fn view_pos(px: vec2<i32>) -> vec4<f32> {
    let d = load_depth(px);
    if (d >= 1.0) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }
    let z = linear_depth(d);
    let uv = (vec2<f32>(px) + vec2<f32>(0.5, 0.5) - u.pane.xy) / u.pane.zw;
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    return vec4<f32>(ndc.x * u.tan_half_x * z, ndc.y * u.tan_half_y * z, -z, 1.0);
}

// 隣のピクセルのうち、奥行きの差が小さい方を使って、面の法線を求める（輪郭をまたがないため）
fn view_normal(px: vec2<i32>, center: vec3<f32>) -> vec3<f32> {
    let r = view_pos(px + vec2<i32>(1, 0));
    let l = view_pos(px + vec2<i32>(-1, 0));
    let t = view_pos(px + vec2<i32>(0, -1));
    let b = view_pos(px + vec2<i32>(0, 1));
    var dx = vec3<f32>(1.0, 0.0, 0.0);
    var dy = vec3<f32>(0.0, 1.0, 0.0);
    let have_r = r.w > 0.5;
    let have_l = l.w > 0.5;
    if (have_r && (!have_l || abs(r.z - center.z) < abs(l.z - center.z))) {
        dx = r.xyz - center;
    } else if (have_l) {
        dx = center - l.xyz;
    }
    let have_t = t.w > 0.5;
    let have_b = b.w > 0.5;
    if (have_t && (!have_b || abs(t.z - center.z) < abs(b.z - center.z))) {
        dy = t.xyz - center;
    } else if (have_b) {
        dy = center - b.xyz;
    }
    var n = normalize(cross(dx, dy));
    // カメラ（原点）のほうを向ける
    if (dot(n, -center) < 0.0) {
        n = -n;
    }
    return n;
}

// ピクセルごとに角度をずらして、縞（バンディング）を目立たなくする
fn noise(px: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(px, vec2<f32>(0.06711056, 0.00583715))));
}

@fragment
fn fs_ao(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let px = vec2<i32>(frag.xy);
    let c = view_pos(px);
    if (c.w < 0.5) {
        return vec4<f32>(1.0, 1.0, 1.0, 1.0);
    }
    let center = c.xyz;
    let n = view_normal(px, center);

    // 探す半径（Å）が、この奥行きで何ピクセルか
    let z = -center.z;
    let px_per_unit = u.pane.w * 0.5 / (u.tan_half_y * z);
    let radius_px = clamp(u.radius * px_per_unit, MIN_RADIUS_PX, MAX_RADIUS_PX);

    let spin = noise(frag.xy) * 6.2831853;
    var occlusion = 0.0;
    for (var i = 0; i < SAMPLES; i = i + 1) {
        let fi = f32(i);
        let a = fi * GOLDEN_ANGLE + spin;
        let r = sqrt((fi + 0.5) / f32(SAMPLES)) * radius_px;
        let off = vec2<i32>(i32(round(cos(a) * r)), i32(round(sin(a) * r)));
        let s = view_pos(px + off);
        if (s.w < 0.5) {
            continue;
        }
        let v = s.xyz - center;
        let dist = length(v);
        if (dist < 1e-4 || dist > u.radius) {
            continue;
        }
        // 面より手前（法線の側）に出ている点だけが、遮蔽する。近いほど強く、半径の端で 0
        let facing = dot(n, v) / dist;
        let falloff = 1.0 - smoothstep(0.5 * u.radius, u.radius, dist);
        occlusion = occlusion + smoothstep(FACING_MIN, FACING_MAX, facing) * falloff;
    }
    let ao = clamp(1.0 - u.strength * GAIN * occlusion / f32(SAMPLES), 0.0, 1.0);
    return vec4<f32>(ao, ao, ao, 1.0);
}

// ぼかす範囲（片側のピクセル数）と、重みの広がり（ピクセル）
const BLUR_RADIUS: i32 = 2;
const BLUR_SIGMA: f32 = 1.5;
// 奥行きの差が、探す半径のこの割合（Å）より大きい隣は、ほとんど混ぜない
const BLUR_DEPTH_TOLERANCE: f32 = 0.25;

fn load_ao(px: vec2<i32>) -> f32 {
    let dims = vec2<i32>(textureDimensions(ao_tex));
    let c = clamp(px, vec2<i32>(0, 0), dims - vec2<i32>(1, 1));
    return textureLoad(ao_tex, c, 0).r;
}

// 遮蔽の係数を、深度が近い隣のピクセルとだけ混ぜる（バイラテラル）。輪郭の向こう側の遮蔽は混ざらない
@fragment
fn fs_blur(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let px = vec2<i32>(frag.xy);
    let d0 = load_depth(px);
    if (d0 >= 1.0) {
        return vec4<f32>(1.0, 1.0, 1.0, 1.0);
    }
    let z0 = linear_depth(d0);
    let tolerance = max(BLUR_DEPTH_TOLERANCE * u.radius, 1e-3);
    var sum = 0.0;
    var weights = 0.0;
    for (var dy = -BLUR_RADIUS; dy <= BLUR_RADIUS; dy = dy + 1) {
        for (var dx = -BLUR_RADIUS; dx <= BLUR_RADIUS; dx = dx + 1) {
            let q = px + vec2<i32>(dx, dy);
            let d = load_depth(q);
            if (d >= 1.0) {
                continue;
            }
            let dz = (linear_depth(d) - z0) / tolerance;
            let dist2 = f32(dx * dx + dy * dy);
            let w = exp(-dist2 / (2.0 * BLUR_SIGMA * BLUR_SIGMA)) * exp(-dz * dz);
            sum = sum + w * load_ao(q);
            weights = weights + w;
        }
    }
    // 中心のピクセルの重みは 1 なので、weights は 0 にならない
    let ao = clamp(sum / weights, 0.0, 1.0);
    return vec4<f32>(ao, ao, ao, 1.0);
}
