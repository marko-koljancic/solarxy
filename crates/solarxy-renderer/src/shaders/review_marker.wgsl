
struct Camera {
    view_pos: vec4<f32>,
    view_proj: mat4x4<f32>,
    view: mat4x4<f32>,
    proj: mat4x4<f32>,
    inv_proj: mat4x4<f32>,
    near: f32,
    far: f32,
    inspection_mode: u32,
    texel_density_target: f32,
    material_override: u32,
    depth_near: f32,
    depth_far: f32,
}
@group(0) @binding(0) var<uniform> camera: Camera;

struct InstanceInput {
    @location(0) world_pos: vec3<f32>,
    @location(1) flags: u32,
}

struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) local_uv: vec2<f32>,
    @location(1) @interpolate(flat) category: u32,
    @location(2) @interpolate(flat) resolved: u32,
    @location(3) @interpolate(flat) selected: u32,
}

const MARKER_SIZE_NDC: f32 = 0.04;

@vertex
fn vs_marker(@builtin(vertex_index) vid: u32, inst: InstanceInput) -> VsOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(1.0, 1.0),
    );
    let corner = corners[vid];

    let center_clip = camera.view_proj * vec4<f32>(inst.world_pos, 1.0);
    let off = vec2<f32>(corner.x, corner.y) * MARKER_SIZE_NDC * center_clip.w;

    var out: VsOut;
    out.clip_pos = vec4<f32>(
        center_clip.x + off.x,
        center_clip.y + off.y,
        center_clip.z,
        center_clip.w,
    );
    out.local_uv = corner;
    out.category = inst.flags & 0xFu;
    out.resolved = (inst.flags >> 4u) & 0x1u;
    out.selected = (inst.flags >> 5u) & 0x1u;
    return out;
}

const CAT_INFO: u32 = 0u;
const CAT_WARNING: u32 = 1u;
const CAT_QUESTION: u32 = 2u;
const CAT_CHANGE: u32 = 3u;

const COLOR_INFO: vec3<f32> = vec3<f32>(0.36, 0.62, 1.00);
const COLOR_WARNING: vec3<f32> = vec3<f32>(1.00, 0.70, 0.24); 
const COLOR_QUESTION: vec3<f32> = vec3<f32>(0.63, 0.43, 1.00); 
const COLOR_CHANGE: vec3<f32> = vec3<f32>(0.24, 0.79, 0.48);

fn sd_circle(p: vec2<f32>, r: f32) -> f32 {
    return length(p) - r;
}

fn sd_triangle(p: vec2<f32>, r: f32) -> f32 {
    let k = sqrt(3.0);
    var q = vec2<f32>(abs(p.x) - r, p.y + r / k);
    if q.x + k * q.y > 0.0 {
        q = vec2<f32>(q.x - k * q.y, -k * q.x - q.y) / 2.0;
    }
    q.x -= clamp(q.x, -2.0 * r, 0.0);
    return -length(q) * sign(q.y);
}

fn sd_hexagon(p: vec2<f32>, r: f32) -> f32 {
    let k = vec3<f32>(-0.866025404, 0.5, 0.577350269);
    var q = abs(p);
    q -= 2.0 * min(dot(k.xy, q), 0.0) * k.xy;
    q -= vec2<f32>(clamp(q.x, -k.z * r, k.z * r), r);
    return length(q) * sign(q.y);
}

fn sd_diamond(p: vec2<f32>, r: f32) -> f32 {
    return abs(p.x) + abs(p.y) - r;
}

@fragment
fn fs_marker(in: VsOut) -> @location(0) vec4<f32> {
    let p = in.local_uv;
    let aa = max(fwidth(in.local_uv.x), 0.01);

    var d: f32;
    var fill: vec3<f32>;
    if in.category == CAT_INFO {
        d = sd_circle(p, 0.72);
        fill = COLOR_INFO;
    } else if in.category == CAT_WARNING {
        d = sd_triangle(p, 0.82);
        fill = COLOR_WARNING;
    } else if in.category == CAT_QUESTION {
        d = sd_hexagon(p, 0.72);
        fill = COLOR_QUESTION;
    } else {
        d = sd_diamond(p, 0.78);
        fill = COLOR_CHANGE;
    }

    let halo_thickness = 0.08;
    let inside = 1.0 - smoothstep(-aa, aa, d);
    let halo = (1.0 - smoothstep(-aa, aa, d + halo_thickness)) - inside;
    let halo_color = vec3<f32>(0.96, 0.96, 0.96);

    var rgb = mix(halo_color, fill, inside);
    var a = max(inside, halo);

    if in.selected != 0u {
        let ring_center = -halo_thickness * 0.5;
        let ring_band = 1.0 -
            smoothstep(0.02, 0.02 + aa, abs(d - ring_center));
        rgb = mix(rgb, vec3<f32>(0.20, 0.95, 1.00), ring_band * 0.85);
        a = max(a, ring_band * 0.85);
    }

    if in.resolved != 0u {
        a *= 0.45;
    }

    if a < 0.01 {
        discard;
    }
    return vec4<f32>(rgb, a);
}
