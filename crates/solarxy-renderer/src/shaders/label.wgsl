// The attribute-label overlay: per-point value/number labels expanded
// entirely on the GPU from storage buffers. One draw call covers three
// element kinds by vertex_index range: the first label_count*6 vertices
// are background chips, the next label_count*6 are anchor dots, and the
// rest are one quad per glyph from the packed glyph stream. Screen-space
// expansion follows the vs_point recipe (points_lines.wgsl): pixel
// offsets become NDC through the wireframe-params viewport size, scaled
// by clip.w so orthographic (w = 1) works unchanged. Overlay semantics:
// no depth test, matching the DOM pins this channel replaced.

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
}
@group(2) @binding(0)
var<uniform> lp: LabelParams;

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
fn place(world: vec3<f32>, offset_px: vec2<f32>) -> vec4<f32> {
    var clip = camera.view_proj * vec4(world, 1.0);
    let ndc = offset_px * vec2(2.0, -2.0) / vec2(params.viewport_w, params.viewport_h);
    return vec4(clip.xy + ndc * clip.w, clip.zw);
}

@vertex
fn vs_label(@builtin(vertex_index) vid: u32) -> VertexOutput {
    let n = lp.label_count;
    var out: VertexOutput;

    if vid < n * 6u {
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
    } else if vid < n * 12u {
        // Dot: centered on the anchor, sized for core plus halo.
        let label = labels[(vid - n * 6u) / 6u];
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
        let g = (vid - n * 12u) / 6u;
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
