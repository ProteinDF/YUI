// 選択の強調（ハローとグロー）。選ばれている原子ごとに、原子の中心を向くビルボードを 1 枚描く。
// どちらも、シーンの深度で深度テストし（深度は書かない）、手前の原子に隠される。

struct CameraUniforms {
    view_proj: mat4x4<f32>,
    light_dir: vec4<f32>,
    camera_right: vec4<f32>,
    camera_up: vec4<f32>,
    style: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> camera: CameraUniforms;

struct QuadInput {
    @location(0) position: vec2<f32>,
};

struct InstanceInput {
    @location(2) model_0: vec4<f32>,
    @location(3) model_1: vec4<f32>,
    @location(4) model_2: vec4<f32>,
    @location(5) model_3: vec4<f32>,
    @location(6) color: vec4<f32>,
};

struct SelectionVertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

// 強調の大きさの倍率は camera.style.z（設定の render.selection_size。既定は 1.0）。
// ハロー: 輪は、原子の半径の 1.0 倍から 1.0 + HALO_WIDTH * 倍率 倍まで。ビルボードの半幅は、輪の外側より
// HALO_MARGIN だけ大きくとる。
const HALO_WIDTH: f32 = 0.3;
const HALO_MARGIN: f32 = 0.2;
// グロー: ビルボードの半幅は、原子の半径の GLOW_SCALE * 倍率 倍。中心から縁へ、exp(-GLOW_FALLOFF * r^2) で弱くなる。
const GLOW_SCALE: f32 = 3.0;
const GLOW_FALLOFF: f32 = 3.5;
const GLOW_STRENGTH: f32 = 0.85;
// ビルボードを、カメラから見て原子の中心より奥へずらす量（半径に対する倍率）。
// 同じ平面に描かれる球のインポスター（中距離の球）に、強調が上書きされないようにする。
const PUSH_BACK: f32 = 0.5;

fn billboard(in: QuadInput, inst: InstanceInput, scale: f32) -> SelectionVertexOutput {
    let center = inst.model_3.xyz;
    let radius = length(inst.model_0.xyz);
    let towards_camera = normalize(cross(camera.camera_right.xyz, camera.camera_up.xyz));
    let world_pos = center
        + (in.position.x * camera.camera_right.xyz + in.position.y * camera.camera_up.xyz) * radius * scale
        - towards_camera * radius * PUSH_BACK;

    var out: SelectionVertexOutput;
    out.clip_position = camera.view_proj * vec4<f32>(world_pos, 1.0);
    out.uv = in.position;
    out.color = inst.color;
    return out;
}

fn halo_width() -> f32 {
    return HALO_WIDTH * camera.style.z;
}

fn halo_scale() -> f32 {
    return 1.0 + halo_width() + HALO_MARGIN;
}

@vertex
fn vs_halo(in: QuadInput, inst: InstanceInput) -> SelectionVertexOutput {
    return billboard(in, inst, halo_scale());
}

@vertex
fn vs_glow(in: QuadInput, inst: InstanceInput) -> SelectionVertexOutput {
    return billboard(in, inst, GLOW_SCALE * camera.style.z);
}

@fragment
fn fs_halo(in: SelectionVertexOutput) -> @location(0) vec4<f32> {
    // 原子の半径を 1 とした、中心からの距離
    let r = length(in.uv) * halo_scale();
    let outer = 1.0 + halo_width();
    let alpha = smoothstep(1.0, 1.04, r) * (1.0 - smoothstep(outer - 0.06, outer, r));
    if (alpha <= 0.0) {
        discard;
    }
    return vec4<f32>(in.color.rgb, alpha * in.color.a);
}

@fragment
fn fs_glow(in: SelectionVertexOutput) -> @location(0) vec4<f32> {
    let r2 = dot(in.uv, in.uv);
    if (r2 >= 1.0) {
        discard;
    }
    // 縁で 0 になるよう、縁の値を引いて 0 に揃える
    let edge = exp(-GLOW_FALLOFF);
    let intensity = max(exp(-GLOW_FALLOFF * r2) - edge, 0.0) / (1.0 - edge) * GLOW_STRENGTH * in.color.a;
    // 乗算済みアルファ（ブレンドは One / OneMinusSrcAlpha）。暗い背景では光って見え、明るい背景でも色が付く。
    return vec4<f32>(in.color.rgb * intensity, intensity);
}
