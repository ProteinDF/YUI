
struct CameraUniforms {
    view_proj: mat4x4<f32>,
    light_dir: vec4<f32>,
    camera_right: vec4<f32>,
    camera_up: vec4<f32>,
    style: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> camera: CameraUniforms;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
};

struct InstanceInput {
    @location(2) model_0: vec4<f32>,
    @location(3) model_1: vec4<f32>,
    @location(4) model_2: vec4<f32>,
    @location(5) model_3: vec4<f32>,
    @location(6) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) color: vec4<f32>,
};

struct ImpostorVertexInput {
    @location(0) position: vec2<f32>,
};

struct ImpostorVertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) world_pos: vec3<f32>,
    @location(3) radius: f32,
};

// 球の表面の深度を書くため、色と一緒に frag_depth を返す。
struct ImpostorFragmentOutput {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
};

struct RibbonVertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
};

@vertex
fn vs_ribbon(in: RibbonVertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = camera.view_proj * vec4<f32>(in.position, 1.0);
    out.normal = normalize(in.normal);
    out.color = in.color;
    return out;
}

@vertex
fn vs_main(in: VertexInput, inst: InstanceInput) -> VertexOutput {
    let model = mat4x4<f32>(
        inst.model_0,
        inst.model_1,
        inst.model_2,
        inst.model_3,
    );
    let world_pos = model * vec4<f32>(in.position, 1.0);
    let normal_mat = mat3x3<f32>(model[0].xyz, model[1].xyz, model[2].xyz);
    let world_normal = normalize(normal_mat * in.normal);

    var out: VertexOutput;
    out.clip_position = camera.view_proj * world_pos;
    out.normal = world_normal;
    out.color = inst.color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let light = normalize(camera.light_dir.xyz);
    let diff = max(dot(in.normal, light), 0.0);
    let ambient = 0.35;
    let diffuse_factor = 0.65;
    let lit_color = in.color.rgb * (ambient + diffuse_factor * diff);
    return vec4<f32>(lit_color, in.color.a);
}

@vertex
fn vs_impostor(in: ImpostorVertexInput, inst: InstanceInput) -> ImpostorVertexOutput {
    let center = inst.model_3.xyz;
    let radius = length(inst.model_0.xyz);

    let world_pos = center + (in.position.x * camera.camera_right.xyz + in.position.y * camera.camera_up.xyz) * radius;

    var out: ImpostorVertexOutput;
    out.clip_position = camera.view_proj * vec4<f32>(world_pos, 1.0);
    out.uv = in.position;
    out.color = inst.color;
    out.world_pos = world_pos;
    out.radius = radius;
    return out;
}

@fragment
fn fs_impostor(in: ImpostorVertexOutput) -> ImpostorFragmentOutput {
    let d2 = dot(in.uv, in.uv);
    if (d2 > 1.0) {
        discard;
    }

    let nz = sqrt(max(0.0, 1.0 - d2));
    let camera_towards = normalize(cross(camera.camera_right.xyz, camera.camera_up.xyz));
    let normal = normalize(in.uv.x * camera.camera_right.xyz + in.uv.y * camera.camera_up.xyz + nz * camera_towards);

    let light = normalize(camera.light_dir.xyz);
    let diff = max(dot(normal, light), 0.0);
    let ambient = 0.35;
    let diffuse_factor = 0.65;
    let lit_color = in.color.rgb * (ambient + diffuse_factor * diff);

    // ビルボードは球の中心を通る平面なので、そのままだと深度が中心の値になり、
    // 手前に伸びるスティックが球の表面から突き抜けて見える。球の表面の深度を書く。
    let surface = in.world_pos + camera_towards * (nz * in.radius);
    let clip = camera.view_proj * vec4<f32>(surface, 1.0);

    var out: ImpostorFragmentOutput;
    out.color = vec4<f32>(lit_color, in.color.a);
    out.depth = clip.z / clip.w;
    return out;
}

struct InteractionVertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) dist: f32,
};

@vertex
fn vs_interaction(in: VertexInput, inst: InstanceInput) -> InteractionVertexOutput {
    let model = mat4x4<f32>(
        inst.model_0,
        inst.model_1,
        inst.model_2,
        inst.model_3,
    );
    let world_pos = model * vec4<f32>(in.position, 1.0);
    let normal_mat = mat3x3<f32>(model[0].xyz, model[1].xyz, model[2].xyz);
    let world_normal = normalize(normal_mat * in.normal);

    // 円柱の半長（Z方向のスケーリング長）
    let half_len = length(model[2].xyz);
    // 底面 (z = -1.0) からの実空間距離（Å）: z は -1.0 .. 1.0 なので (z + 1.0) * half_len で 0.0 .. 2 * half_len
    let dist = (in.position.z + 1.0) * half_len;

    var out: InteractionVertexOutput;
    out.clip_position = camera.view_proj * world_pos;
    out.normal = world_normal;
    out.color = inst.color;
    out.dist = dist;
    return out;
}

@fragment
fn fs_interaction(in: InteractionVertexOutput) -> @location(0) vec4<f32> {
    // 破線パターン: 描く長さ camera.style.x、空白の長さ camera.style.y（Å。設定の render: で変えられる。
    // 既定は 0.35 Å と 0.25 Å で、1 周期は 0.60 Å）
    let dash = camera.style.x;
    let period = max(dash + camera.style.y, 0.01);
    let phase = in.dist - floor(in.dist / period) * period;
    if (phase > dash) {
        discard;
    }

    let light = normalize(camera.light_dir.xyz);
    let diff = max(dot(in.normal, light), 0.0);
    let ambient = 0.35;
    let diffuse_factor = 0.65;
    let lit_color = in.color.rgb * (ambient + diffuse_factor * diff);
    return vec4<f32>(lit_color, in.color.a);
}

// ----------------------------------------------------------------------------
// Compute Shader: 視錐台カリング & 段階的LODインスタンス・コンパクション
// ----------------------------------------------------------------------------

struct FrustumUniforms {
    planes: array<vec4<f32>, 6>,
    camera_pos: vec4<f32>,
    lod_threshold_sq: f32,
    total_instances: u32,
    is_stick: u32,
    _pad0: u32,
};

struct InstanceRawData {
    model_0: vec4<f32>,
    model_1: vec4<f32>,
    model_2: vec4<f32>,
    model_3: vec4<f32>,
    color: vec4<f32>,
};

struct DrawIndexedIndirectArgs {
    index_count: u32,
    instance_count: atomic<u32>,
    first_index: u32,
    base_vertex: i32,
    first_instance: u32,
};

@group(0) @binding(0)
var<uniform> frustum: FrustumUniforms;

@group(0) @binding(1)
var<storage, read> src_instances: array<InstanceRawData>;

@group(0) @binding(2)
var<storage, read_write> dst_instances_near: array<InstanceRawData>;

@group(0) @binding(3)
var<storage, read_write> indirect_args_near: DrawIndexedIndirectArgs;

@group(0) @binding(4)
var<storage, read_write> dst_instances_mid: array<InstanceRawData>;

@group(0) @binding(5)
var<storage, read_write> indirect_args_mid: DrawIndexedIndirectArgs;

@compute @workgroup_size(64)
fn cs_cull(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let index = global_id.x;
    if (index >= frustum.total_instances) {
        return;
    }

    let inst = src_instances[index];
    let center = inst.model_3.xyz;
    let r2 = dot(inst.model_0.xyz, inst.model_0.xyz) +
             dot(inst.model_1.xyz, inst.model_1.xyz) +
             dot(inst.model_2.xyz, inst.model_2.xyz);
    let radius = sqrt(r2);

    // 6枚の視錐台平面（法線正規化済み）との符号付き距離を判定
    var visible = true;
    for (var i = 0u; i < 6u; i = i + 1u) {
        let plane = frustum.planes[i];
        let dist = dot(plane.xyz, center) + plane.w;
        if (dist < -radius) {
            visible = false;
            break;
        }
    }

    if (!visible) {
        return;
    }

    // カメラからの二乗距離によるLOD階層判定
    let diff = center - frustum.camera_pos.xyz;
    let dist_sq = dot(diff, diff);

    if (dist_sq <= frustum.lod_threshold_sq) {
        // 近距離 (Near Tier): フルジオメトリ（球またはスティック）
        let out_idx = atomicAdd(&indirect_args_near.instance_count, 1u);
        dst_instances_near[out_idx] = inst;
    } else if (frustum.is_stick == 0u) {
        // 中距離 (Mid Tier): 球のみインポスターとして書き出し（スティックは非表示）
        let out_idx = atomicAdd(&indirect_args_mid.instance_count, 1u);
        dst_instances_mid[out_idx] = inst;
    }
}
