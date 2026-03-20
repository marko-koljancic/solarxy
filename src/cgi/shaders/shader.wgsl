struct Camera {
    view_pos: vec4<f32>,
    view_proj: mat4x4<f32>,
}
@group(1) @binding(0)
var<uniform> camera: Camera;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) tex_coords: vec2<f32>,
    @location(2) normal: vec3<f32>,
    @location(3) tangent: vec3<f32>,
    @location(4) bitangent: vec3<f32>,
}

struct InstanceInput {
    @location(5) model_matrix_0: vec4<f32>,
    @location(6) model_matrix_1: vec4<f32>,
    @location(7) model_matrix_2: vec4<f32>,
    @location(8) model_matrix_3: vec4<f32>,
    @location(9) normal_matrix_0: vec3<f32>,
    @location(10) normal_matrix_1: vec3<f32>,
    @location(11) normal_matrix_2: vec3<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
    @location(1) tangent_position: vec3<f32>,
    @location(2) tangent_view_position: vec3<f32>,
    @location(3) tbn_col0: vec3<f32>,
    @location(4) tbn_col1: vec3<f32>,
    @location(5) tbn_col2: vec3<f32>,
};

@vertex
fn vs_main(
    model: VertexInput,
    instance: InstanceInput,
) -> VertexOutput {
    let model_matrix = mat4x4<f32>(
        instance.model_matrix_0,
        instance.model_matrix_1,
        instance.model_matrix_2,
        instance.model_matrix_3,
    );

    let normal_matrix = mat3x3<f32>(
        instance.normal_matrix_0,
        instance.normal_matrix_1,
        instance.normal_matrix_2,
    );

    let world_normal = normalize(normal_matrix * model.normal);
    let world_tangent = normalize(normal_matrix * model.tangent);
    let world_bitangent = normalize(normal_matrix * model.bitangent);
    let tangent_matrix = transpose(mat3x3<f32>(
        world_tangent,
        world_bitangent,
        world_normal,
    ));

    let world_position = model_matrix * vec4<f32>(model.position, 1.0);

    var out: VertexOutput;
    out.clip_position = camera.view_proj * world_position;
    out.tex_coords = model.tex_coords;
    out.tangent_position = tangent_matrix * world_position.xyz;
    out.tangent_view_position = tangent_matrix * camera.view_pos.xyz;
    out.tbn_col0 = tangent_matrix[0];
    out.tbn_col1 = tangent_matrix[1];
    out.tbn_col2 = tangent_matrix[2];
    return out;
}

@group(0) @binding(0)
var t_diffuse: texture_2d<f32>;
@group(0) @binding(1)
var s_diffuse: sampler;
@group(0) @binding(2)
var t_normal: texture_2d<f32>;
@group(0) @binding(3)
var s_normal: sampler;

struct LightEntry {
    position: vec3<f32>,  // bytes 0-11; WGSL pads to 16 before next vec3
    color: vec3<f32>,     // bytes 16-27
    intensity: f32,       // bytes 28-31
}
struct LightsUniform { lights: array<LightEntry, 3>, }
@group(2) @binding(0)
var<uniform> lights: LightsUniform;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let object_color: vec4<f32> = textureSample(t_diffuse, s_diffuse, in.tex_coords);
    let object_normal: vec4<f32> = textureSample(t_normal, s_normal, in.tex_coords);

    let tbn = mat3x3<f32>(in.tbn_col0, in.tbn_col1, in.tbn_col2);
    let tangent_normal = object_normal.xyz * 2.0 - 1.0;

    let ambient = lights.lights[0].color * lights.lights[0].intensity * 0.1;

    var accumulated = vec3<f32>(0.0);
    for (var i = 0u; i < 3u; i++) {
        let light_dir = normalize(tbn * lights.lights[i].position - in.tangent_position);
        let view_dir = normalize(in.tangent_view_position - in.tangent_position);
        let half_dir = normalize(view_dir + light_dir);
        let diff = max(dot(tangent_normal, light_dir), 0.0);
        let spec = pow(max(dot(tangent_normal, half_dir), 0.0), 32.0);
        let i_f = lights.lights[i].intensity;
        accumulated += (lights.lights[i].color * diff + lights.lights[i].color * spec) * i_f;
    }

    let result = (ambient + accumulated) * object_color.xyz;
    return vec4<f32>(result, object_color.a);
}
