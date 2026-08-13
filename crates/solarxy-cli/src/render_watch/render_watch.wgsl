// The live window's whole shader: one triangle covering the window, the
// picture sampled into its rectangle, and a checker on the canvas around it.
//
// A triangle rather than a quad because two triangles meeting across the
// diagonal give the sampler a seam to be inconsistent about, and one that is
// larger than the screen has no diagonal at all. The vertices come from the
// index rather than from a buffer, so the window needs no geometry.
//
// One draw serves both the picture and its canvas. The sample happens
// unconditionally, because WGSL wants it in uniform control flow, and the
// fragment then selects. The checker reads raw window coordinates, which is
// what pins it to the glass: it cannot pan, cannot zoom, and cannot show
// through the picture, which is written with alpha one.

struct View {
    // The image rectangle in window pixels: left, top, width, height.
    rect: vec4<f32>,
    // The window size in pixels in xy; zw unused.
    window: vec4<f32>,
};

struct Vertex {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> Vertex {
    // (0,0), (2,0), (0,2) in screen space: a triangle twice the size of the
    // window, whose visible third is exactly it.
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    var out: Vertex;
    // Screen space runs down and clip space runs up, so the y is flipped
    // here rather than anywhere else.
    out.clip = vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
    out.uv = uv;
    return out;
}

@group(0) @binding(0) var picture: texture_2d<f32>;
@group(0) @binding(1) var picture_sampler: sampler;
@group(0) @binding(2) var<uniform> view: View;

// The canvas's two greys, linear, dark enough that any picture reads
// brighter than what it sits on.
const CHECKER_A: f32 = 0.013;
const CHECKER_B: f32 = 0.022;
// The checker cell edge in window pixels.
const CHECKER_CELL: f32 = 8.0;

@fragment
fn fs_main(in: Vertex) -> @location(0) vec4<f32> {
    // The window-space pixel, top-left origin, matching the cursor space the
    // view rectangle is computed in.
    let p = in.uv * view.window.xy;
    let local = (p - view.rect.xy) / max(view.rect.zw, vec2<f32>(1e-6));
    // The texture is sRGB and the surface is sRGB, so the hardware decodes on
    // the read and encodes on the write and nothing here has to know about it.
    let sampled = textureSample(
        picture,
        picture_sampler,
        clamp(local, vec2<f32>(0.0), vec2<f32>(1.0)),
    );
    let inside = all(local >= vec2<f32>(0.0)) && all(local < vec2<f32>(1.0));
    let cell = i32(floor(p.x / CHECKER_CELL) + floor(p.y / CHECKER_CELL)) & 1;
    let grey = select(CHECKER_A, CHECKER_B, cell == 1);
    let canvas = vec4<f32>(grey, grey, grey, 1.0);
    return select(canvas, vec4<f32>(sampled.rgb, 1.0), inside);
}
