// The live window's whole shader: one triangle covering clip space, and the
// picture sampled across it.
//
// A triangle rather than a quad because two triangles meeting across the
// diagonal give the sampler a seam to be inconsistent about, and one that is
// larger than the screen has no diagonal at all. The vertices come from the
// index rather than from a buffer, so the window needs no geometry.
//
// The letterbox is not here. It is the viewport the pass sets, so the picture's
// proportions are a rectangle on the screen rather than arithmetic in a shader
// that would have to agree with the rectangle anyway.

struct Vertex {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> Vertex {
    // (0,0), (2,0), (0,2) in texture space: a triangle twice the size of the
    // viewport, whose visible third is exactly it.
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    var out: Vertex;
    // Texture space runs down and clip space runs up, so the y is flipped
    // here rather than in the sampling.
    out.clip = vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
    out.uv = uv;
    return out;
}

@group(0) @binding(0) var picture: texture_2d<f32>;
@group(0) @binding(1) var picture_sampler: sampler;

@fragment
fn fs_main(in: Vertex) -> @location(0) vec4<f32> {
    // The texture is sRGB and the surface is sRGB, so the hardware decodes on
    // the read and encodes on the write and nothing here has to know about it.
    return textureSample(picture, picture_sampler, in.uv);
}
