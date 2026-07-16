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

struct GridUniform {
    cell_size: f32,
    color_r: f32,
    color_g: f32,
    color_b: f32,
    // 0 = XZ ground (default), 1 = XY, 2 = YZ. Chosen per pane so an
    // orthographic elevation view gets a face-on grid, not an edge-on hairline.
    plane: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}
@group(1) @binding(0) var<uniform> grid: GridUniform;

struct VertexInput {
    @location(0) position: vec3<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    // The two in-plane grid coordinates (s, t) for the line pattern.
    @location(0) grid2d: vec2<f32>,
    // The true world position, for the camera-distance fade.
    @location(1) world_pos: vec3<f32>,
}

// The quad is built flat in local XZ (position.y is a tiny offset). Reinterpret
// its two spanning coords (s = x, t = z) into the selected world plane, keeping
// the small offset on the perpendicular axis.
fn place(pos: vec3<f32>) -> vec3<f32> {
    let s = pos.x;
    let t = pos.z;
    let perp = pos.y;
    if grid.plane == 1u {          // XY (front / back): s -> X, t -> Y
        return vec3<f32>(s, t, perp);
    } else if grid.plane == 2u {   // YZ (left / right): s -> Y, t -> Z
        return vec3<f32>(perp, s, t);
    }
    return vec3<f32>(s, perp, t);  // XZ ground (default): s -> X, t -> Z
}

@vertex
fn vs_grid(model: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    let world = place(model.position);
    out.clip_position = camera.view_proj * vec4<f32>(world, 1.0);
    out.grid2d = vec2<f32>(model.position.x, model.position.z);
    out.world_pos = world;
    return out;
}

@fragment
fn fs_grid(in: VertexOutput) -> @location(0) vec4<f32> {
    let ds = min(fract(in.grid2d.x / grid.cell_size), 1.0 - fract(in.grid2d.x / grid.cell_size));
    let dt = min(fract(in.grid2d.y / grid.cell_size), 1.0 - fract(in.grid2d.y / grid.cell_size));
    let alpha = 1.0 - smoothstep(0.0, 0.008, min(ds, dt));
    if alpha < 0.01 {
        discard;
    }

    let dist = distance(camera.view_pos.xyz, in.world_pos);
    let fade = 1.0 - smoothstep(grid.cell_size * 30.0, grid.cell_size * 60.0, dist);

    var color = vec3<f32>(grid.color_r, grid.color_g, grid.color_b);

    // World-axis lines (Blender-style) on the elevation-view grids only: the
    // line where a grid coord is ~0 is a world axis, colored by the axis it runs
    // along. Plane 0 (the XZ ground, shown in perspective and top/bottom) keeps
    // its established monochrome look, so those views stay pixel-identical.
    if grid.plane != 0u {
        let red = vec3<f32>(0.85, 0.22, 0.28);
        let green = vec3<f32>(0.42, 0.72, 0.15);
        let blue = vec3<f32>(0.22, 0.44, 0.86);
        var s_col = red;   // s axis color
        var t_col = blue;  // t axis color
        if grid.plane == 1u {          // XY: s -> X (red), t -> Y (green)
            s_col = red;
            t_col = green;
        } else {                       // YZ: s -> Y (green), t -> Z (blue)
            s_col = green;
            t_col = blue;
        }
        // A little wider than a normal line so the axis reads clearly.
        let axis_half = grid.cell_size * 0.06;
        // The line at t == 0 runs along s -> color it the s-axis color; the
        // line at s == 0 runs along t -> the t-axis color.
        if abs(in.grid2d.y) < axis_half {
            color = s_col;
        }
        if abs(in.grid2d.x) < axis_half {
            color = t_col;
        }
    }

    return vec4<f32>(color, alpha * 0.75 * fade);
}
