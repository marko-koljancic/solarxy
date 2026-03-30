struct GradientColors {
    top: vec4<f32>,
    bottom: vec4<f32>,
}
@group(0) @binding(0) var<uniform> colors: GradientColors;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_background(@builtin(vertex_index) id: u32) -> VertexOutput {
    let uv = vec2<f32>(f32((id << 1u) & 2u), f32(id & 2u));
    var out: VertexOutput;
    out.position = vec4<f32>(uv * 2.0 - 1.0, 1.0, 1.0);
    out.uv = uv;
    return out;
}

@fragment
fn fs_background(in: VertexOutput) -> @location(0) vec4<f32> {
    return mix(colors.top, colors.bottom, in.uv.y);
}
