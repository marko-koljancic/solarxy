// The attribute-label overlay: per-point value/number labels expanded
// entirely on the GPU from storage buffers. One draw call covers three
// element kinds by vertex_index range: the first label_count*6 vertices
// are background chips, the next label_count*6 are anchor dots, and the
// rest are one quad per glyph from the packed glyph stream. Screen-space
// expansion follows the vs_point recipe (points_lines.wgsl): pixel
// offsets become NDC through the wireframe-params viewport size, scaled
// by clip.w so orthographic (w = 1) works unchanged.
//
// Depth: the host picks between two pipelines. In a wireframe-only pane
// nothing occludes, so labels draw over everything as they always have. In
// a shaded pane the pipeline depth-tests, and a label on a point facing away
// from the camera is hidden by the surface in front of it -- which is what
// makes a dense point cloud readable instead of showing you the far side of
// the object through the near side. `place` biases the anchor a fixed
// fraction of its distance TOWARD the eye before projecting, which is exact
// in screen space (the point slides along its own eye ray, so x and y do not
// move) and buys the tolerance the test needs: the anchor sits exactly ON
// the surface it labels, and its chip and glyphs spill over neighbouring
// pixels whose surface depth differs slightly.

struct Camera {
    view_pos: vec4<f32>,
    view_proj: mat4x4<f32>,
}
@group(0) @binding(0)
var<uniform> camera: Camera;

// Bound for its maintained viewport size, exactly like vs_point.
struct WireParams {
    color: vec4<f32>,
    width_px: f32,
    viewport_w: f32,
    viewport_h: f32,
    /// The point size, read only by `points_lines.wgsl`. Declared so the
    /// three views of this uniform agree; WGSL needs the size to match, not
    /// the names, but a slot called `_pad` in one file and a real field in
    /// another is exactly how a struct drifts.
    point_size_px: f32,
}
@group(1) @binding(0)
var<uniform> params: WireParams;

struct LabelParams {
    text_color: vec4<f32>,
    chip_color: vec4<f32>,
    dot_color: vec4<f32>,
    text_px: f32,
    advance_px: f32,
    dot_px: f32,
    text_gap_px: f32,
    chip_pad_x: f32,
    chip_pad_y: f32,
    chip_radius: f32,
    label_count: u32,
    // 1 when chips are drawn. Gates the chip AND shifts the dot and glyph
    // vertex ranges, so it must match `LabelResources::vertex_count`.
    chip_on: u32,
    // The Rust struct's trailing `[u32; 3]` padding, declared as three
    // SCALARS rather than a `vec3<u32>`.
    //
    // This is not a style choice. A `vec3<u32>` has 16-byte ALIGNMENT in
    // WGSL, so it would sit at offset 96 instead of 84 and make this struct
    // span 112 bytes against the Rust struct's 96. Bind-group layouts here
    // use `min_binding_size: None`, so that mismatch is not caught when the
    // bind group is built -- it fails at DRAW time, which invalidates the
    // encoder, and since the pane's encoder also carries the composite pass
    // the entire frame is discarded and the viewport goes black. Three
    // scalars keep the span at 96 and matching.
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}
@group(2) @binding(0)
var<uniform> lp: LabelParams;

// How far toward the eye the anchor is nudged before projecting, as a
// fraction of its distance. Must exceed the surface-depth variation across a
// label's own width (so a label does not clip against the surface it sits
// on) and stay well under an object's thickness (so a back-face label is
// still hidden by the near face). One percent clears both by orders of
// magnitude at any sane zoom.
const OCCLUSION_BIAS: f32 = 0.01;

struct Label {
    pos: vec3<f32>,
    glyph_count: u32,
}
@group(2) @binding(1)
var<storage, read> labels: array<Label>;

// Packed: bits 0..5 glyph id, 5..11 column, 11..32 label index.
@group(2) @binding(2)
var<storage, read> glyph_words: array<u32>;

@group(2) @binding(3)
var atlas: texture_2d<f32>;
@group(2) @binding(4)
var atlas_samp: sampler;

// The atlas contract with labels.rs / gen_glyph_atlas.rs.
const ATLAS_COLS: f32 = 5.0;
const ATLAS_ROWS: f32 = 5.0;
const CELL_W_PX: f32 = 48.0;
const CELL_H_PX: f32 = 64.0;
const EM_PX: f32 = 40.0;
const PEN_X_PX: f32 = 4.0;
const BASELINE_Y_PX: f32 = 44.0;
const SDF_SPREAD_PX: f32 = 6.0;
// The text block's vertical centering: baseline sits below the anchor
// midline by roughly a third of the em (the DOM's line-box centering).
const BASELINE_DROP: f32 = 0.34;
// The dot halo: a 2px CSS ring at 30 percent alpha (the box-shadow).
const HALO_CSS_PX: f32 = 2.0;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    // 0 = chip, 1 = dot, 2 = glyph.
    @location(0) @interpolate(flat) mode: u32,
    // chip/dot: px coords relative to the element center; glyph: atlas uv.
    @location(1) local: vec2<f32>,
    // chip: half extents + radius; dot: core radius + halo width; glyph:
    // screen px per SDF texel in x (for the AA width).
    @location(2) @interpolate(flat) extra: vec3<f32>,
}

// Two CCW triangles over the unit quad (0..1).
fn corner_of(id: u32) -> vec2<f32> {
    var corners = array<vec2<f32>, 6>(
        vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0),
        vec2(0.0, 1.0), vec2(1.0, 0.0), vec2(1.0, 1.0),
    );
    return corners[id];
}

// Anchors a screen-space quad (px offsets, +y down) onto a world point.
//
// The anchor slides along its own eye ray by OCCLUSION_BIAS first. That is a
// no-op in screen space (a point on the eye ray projects to the same pixel
// however far along it sits) and a depth bias everywhere else, so it is free
// when the pipeline ignores depth and exactly what is needed when it does not.
fn place(world: vec3<f32>, offset_px: vec2<f32>) -> vec4<f32> {
    let toward_eye = camera.view_pos.xyz - world;
    var clip = camera.view_proj * vec4(world + toward_eye * OCCLUSION_BIAS, 1.0);
    let ndc = offset_px * vec2(2.0, -2.0) / vec2(params.viewport_w, params.viewport_h);
    return vec4(clip.xy + ndc * clip.w, clip.zw);
}

@vertex
fn vs_label(@builtin(vertex_index) vid: u32) -> VertexOutput {
    let n = lp.label_count;
    // Where the dot run begins: after the chips when they are drawn, at zero
    // when they are not. Mirrors `LabelResources::vertex_count`.
    let dot_base = n * 6u * lp.chip_on;
    let glyph_base = dot_base + n * 6u;
    var out: VertexOutput;

    if vid < dot_base {
        // Chip: spans the text run plus padding, vertically centered.
        let label = labels[vid / 6u];
        let c = corner_of(vid % 6u);
        let w = f32(label.glyph_count) * lp.advance_px;
        let x0 = lp.text_gap_px - lp.chip_pad_x;
        let x1 = lp.text_gap_px + w + lp.chip_pad_x;
        let half = vec2((x1 - x0) * 0.5, lp.text_px * 0.5 + lp.chip_pad_y);
        // A glyphless label (points mode off, no lane) keeps a degenerate
        // chip: half extents collapse when glyph_count is 0 upstream.
        let center = vec2((x0 + x1) * 0.5, 0.0);
        let px = center + (c - vec2(0.5)) * half * 2.0;
        out.clip_position = place(label.pos, select(px, vec2(0.0), label.glyph_count == 0u));
        out.mode = 0u;
        out.local = (c - vec2(0.5)) * half * 2.0;
        out.extra = vec3(half, lp.chip_radius);
    } else if vid < glyph_base {
        // Dot: centered on the anchor, sized for core plus halo.
        let label = labels[(vid - dot_base) / 6u];
        let c = corner_of(vid % 6u);
        let halo = HALO_CSS_PX * lp.dot_px / 6.0;
        let r = lp.dot_px * 0.5 + halo;
        let px = (c - vec2(0.5)) * r * 2.0;
        out.clip_position = place(label.pos, px);
        out.mode = 1u;
        out.local = px;
        out.extra = vec3(lp.dot_px * 0.5, halo, 0.0);
    } else {
        // Glyph: one atlas cell, positioned by its column in the run.
        let g = (vid - glyph_base) / 6u;
        let word = glyph_words[g];
        let glyph = word & 31u;
        let col = (word >> 5u) & 63u;
        let label = labels[word >> 11u];
        let c = corner_of(vid % 6u);

        let s = lp.text_px / EM_PX;
        let pen_x = lp.text_gap_px + f32(col) * lp.advance_px;
        let baseline_y = BASELINE_DROP * lp.text_px;
        let x0 = pen_x - PEN_X_PX * s;
        let y0 = baseline_y - BASELINE_Y_PX * s;
        let px = vec2(x0, y0) + c * vec2(CELL_W_PX, CELL_H_PX) * s;
        out.clip_position = place(label.pos, px);
        out.mode = 2u;
        let cell = vec2(f32(glyph % 5u), f32(glyph / 5u));
        out.local = (cell + c) / vec2(ATLAS_COLS, ATLAS_ROWS);
        out.extra = vec3(s, 0.0, 0.0);
    }
    return out;
}

@fragment
fn fs_label(in: VertexOutput) -> @location(0) vec4<f32> {
    // WGSL uniformity: `in.mode` is non-uniform, so every derivative op
    // (textureSample's implicit derivatives, fwidth) MUST run outside the
    // mode branches -- Chrome's Tint rejects the pipeline otherwise (naga
    // accepts it, so only the browser build would break). Compute all
    // three candidate coverages unconditionally, then select.

    // Chip: rounded-rect SDF over the local px frame.
    let chip_q = abs(in.local) - (in.extra.xy - vec2(in.extra.z));
    let chip_d = length(max(chip_q, vec2(0.0))) + min(max(chip_q.x, chip_q.y), 0.0) - in.extra.z;
    let chip_aa = max(fwidth(chip_d), 0.5);
    let chip_alpha = (1.0 - smoothstep(-chip_aa, chip_aa, chip_d)) * lp.chip_color.a;

    // Dot: solid core plus the translucent halo ring.
    let dot_r = length(in.local);
    let dot_aa = max(fwidth(dot_r), 0.5);
    let core = 1.0 - smoothstep(in.extra.x - dot_aa, in.extra.x + dot_aa, dot_r);
    let halo_edge = in.extra.x + in.extra.y;
    let halo = (1.0 - smoothstep(halo_edge - dot_aa, halo_edge + dot_aa, dot_r)) * 0.3;
    let dot_alpha = max(core, halo) * lp.dot_color.a;

    // Glyph: decode the SDF (0.5 is the outline) into screen px. For chip
    // and dot fragments `in.local` is a px coordinate, not a uv; the
    // clamped sample it produces is computed and discarded by the select.
    let sampled = textureSample(atlas, atlas_samp, in.local).r;
    let glyph_d = (sampled - 0.5) * 2.0 * SDF_SPREAD_PX * in.extra.x;
    let glyph_aa = max(fwidth(glyph_d), 0.35);
    let glyph_alpha = smoothstep(-glyph_aa, glyph_aa, glyph_d) * lp.text_color.a;

    var color = lp.text_color.rgb;
    var alpha = glyph_alpha;
    if in.mode == 0u {
        color = lp.chip_color.rgb;
        alpha = chip_alpha;
    } else if in.mode == 1u {
        color = lp.dot_color.rgb;
        alpha = dot_alpha;
    }
    if alpha <= 0.003 {
        discard;
    }
    return vec4(color, alpha);
}
