// Scene pipelines for non-triangle topologies (0.8.0 Enabler A).
//
// Lines: a plain 1 px hardware line-list, unlit, flat white or per-vertex
// color (matching the overlay precedent).
//
// Points: WebGPU rasterizes point-list primitives at a fixed single pixel
// with no size control, so points draw as camera-facing quads expanded in
// the vertex shader from `vertex_index` (decision M-6), following the
// `edge_wire.wgsl` screen-space expansion precedent: positions are pulled
// from the mesh's edge-geometry storage buffer, whose padded w slot
// carries the point's packed sRGB8 color (bit-preserved as u32; white when
// the mesh has no color lane). Unlit by vertex color per decision M-8.

struct Camera {
    view_pos: vec4<f32>,
    view_proj: mat4x4<f32>,
}
@group(0) @binding(0)
var<uniform> camera: Camera;

struct InstanceInput {
    @location(5) model_matrix_0: vec4<f32>,
    @location(6) model_matrix_1: vec4<f32>,
    @location(7) model_matrix_2: vec4<f32>,
    @location(8) model_matrix_3: vec4<f32>,
    @location(9) normal_matrix_0: vec3<f32>,
    @location(10) normal_matrix_1: vec3<f32>,
    @location(11) normal_matrix_2: vec3<f32>,
}

fn model_matrix_of(instance: InstanceInput) -> mat4x4<f32> {
    return mat4x4<f32>(
        instance.model_matrix_0,
        instance.model_matrix_1,
        instance.model_matrix_2,
        instance.model_matrix_3,
    );
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
}

// ---- Lines: indexed line-list over the regular mesh vertex buffer. ----

@vertex
fn vs_line(
    @location(0) position: vec3<f32>,
    instance: InstanceInput,
) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = camera.view_proj * model_matrix_of(instance) * vec4(position, 1.0);
    out.color = vec4(1.0);
    return out;
}

@vertex
fn vs_line_colored(
    @location(0) position: vec3<f32>,
    @location(12) color: vec4<f32>,
    instance: InstanceInput,
) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = camera.view_proj * model_matrix_of(instance) * vec4(position, 1.0);
    out.color = vec4(color.rgb, 1.0);
    return out;
}

// ---- Points: shader-expanded camera-facing quads. ----

// The wireframe-params uniform, bound for its maintained viewport size
// (the same buffer the edge wireframe uses; color/width are not read).
struct WireParams {
    color: vec4<f32>,
    width_px: f32,
    viewport_w: f32,
    viewport_h: f32,
    point_size_px: f32,
}
@group(1) @binding(0)
var<uniform> params: WireParams;

// The mesh's padded-position storage buffer viewed as point data: xyz is
// the position, the fourth 16-byte-aligned slot is the packed sRGB8 color
// (declared u32 here so NaN-pattern float loads never occur).
struct PointDatum {
    pos: vec3<f32>,
    color: u32,
}
@group(2) @binding(0)
var<storage, read> points: array<PointDatum>;

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + vec3(0.055)) / 1.055, vec3(2.4));
    return select(hi, lo, c <= vec3(0.04045));
}

@vertex
fn vs_point(@builtin(vertex_index) vid: u32, instance: InstanceInput) -> VertexOutput {
    let point = points[vid / 6u];
    let corner_id = vid % 6u;
    // Two CCW triangles over the quad corners (-0.5..0.5).
    var corners = array<vec2<f32>, 6>(
        vec2(-0.5, -0.5), vec2(0.5, -0.5), vec2(-0.5, 0.5),
        vec2(-0.5, 0.5), vec2(0.5, -0.5), vec2(0.5, 0.5),
    );
    let corner = corners[corner_id];

    var clip = camera.view_proj * model_matrix_of(instance) * vec4(point.pos, 1.0);
    let offset_ndc =
        corner * params.point_size_px * 2.0 / vec2(params.viewport_w, params.viewport_h);
    clip = vec4(clip.xy + offset_ndc * clip.w, clip.zw);

    var out: VertexOutput;
    out.clip_position = clip;
    let srgb = unpack4x8unorm(point.color);
    out.color = vec4(srgb_to_linear(srgb.rgb), 1.0);
    return out;
}

@fragment
fn fs_unlit(in: VertexOutput) -> @location(0) vec4<f32> {
    return in.color;
}

// Selection-outline mask variant (R8 target): solid white silhouette.
@fragment
fn fs_mask(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4(1.0);
}
