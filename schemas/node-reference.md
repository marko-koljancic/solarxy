← [Home](Home)

# Node Reference

Every node type in Solarxy Web, with its ports, parameters and defaults.

Every node also carries a `name` and a `description` parameter. They are common to all types, so the per-node tables below do not repeat them.

**This page is generated** from the node registry itself (`cargo run -p solarxy-graph --example gen_registry -- markdown`), so it cannot drift from the application. Do not edit it by hand: change the node's descriptor in `crates/solarxy-graph/src/nodes/` and regenerate.

77 node types across 15 categories.

## Contents

**Adjust**  
[Brightness / Contrast](#brightness_contrast) · [Gamma](#gamma) · [Hue / Saturation](#hue_saturation) · [Invert](#invert) · [Levels](#levels)

**Attribute**  
[Attribute Copy](#attribute_copy) · [Attribute Create](#attribute_create) · [Attribute Promote](#attribute_promote) · [Attribute Randomize](#attribute_randomize) · [Attribute Wrangle](#attribute_wrangle) · [Attribute from Image](#attribute_from_image) · [Compute Normals](#compute_normals) · [UV Project](#uv_project)

**Cameras**  
[Camera](#camera)

**Composite**  
[Blur](#blur) · [Height to Normal](#height_to_normal) · [Mix](#mix) · [Pack ORM](#pack_orm) · [Sharpen](#sharpen)

**Container**  
[Geo](#geo) · [Mat](#matnet) · [Tex](#texnet)

**Copy & Instance**  
[Array](#array) · [Copy to Points](#copy_to_points) · [Mirror](#mirror) · [Scatter](#scatter)

**Export**  
[Export Geometry](#geo_export) · [Export Image](#image_export) · [Render](#render)

**Generate**  
[Brick](#brick) · [Checker](#checker) · [Constant](#constant) · [Gradient](#gradient) · [Noise](#noise) · [Ramp](#ramp) · [Voronoi](#voronoi)

**Generators**  
[Box](#box) · [Circle](#circle) · [Cone](#cone) · [Cylinder](#cylinder) · [Line](#line) · [Plane](#plane) · [Sphere](#sphere) · [Torus](#torus) · [Torus Knot](#torus_knot)

**Import**  
[Import Image](#import_image) · [Import OBJ](#import_obj) · [Import PLY](#import_ply) · [Import STL](#import_stl) · [Import glTF](#import_gltf) · [Texture Reference](#tex_ref)

**Lights**  
[Ambient Light](#ambient_light) · [Directional Light](#directional_light) · [Environment](#environment) · [Hemisphere Light](#hemisphere_light) · [Point Light](#point_light) · [Rect Area Light](#rect_area_light) · [Spot Light](#spot_light)

**Shaders**  
[MatCap](#matcap) · [Material](#material) · [Mix Material](#mix_material) · [Principled](#principled) · [Toon](#toon) · [Unlit](#unlit)

**Topology**  
[Delete](#delete) · [Edges to Geo](#edges_to_geo) · [Merge](#merge) · [Points from Geo](#points_from_geo) · [Subdivide](#subdivide)

**Transform**  
[Displace](#displace) · [Transform](#transform)

**Utility**  
[Bounds](#bounds) · [Note](#note) · [Null](#null) · [Switch](#switch) · [Text](#text) · [Validate](#validate)

## Adjust

### Brightness / Contrast <a id="brightness_contrast"></a>

`brightness_contrast` · v1 · Adjust · placed inside a texture network

Palette search also matches: brightness, contrast, exposure.

Scales each RGB channel around a 0.5 pivot by Contrast, then adds Brightness on top. Both run through a 256-entry lookup table; alpha is untouched.

The quick tonal fix, one step blunter than `levels`: two knobs, no per-end control. Reach for it to nudge an imported image or to open up a `noise` before it becomes a mask. When you need the black and white points independently, or a midtone bend, go to `levels`.

The pivot is fixed at 0.5, so contrast leaves mid-gray exactly where it is and moves everything else around it. Contrast at -1 collapses the image to that pivot, flat gray plus whatever Brightness adds. Both controls clip against 0 and 1, and because the result is baked into a lookup table, a clipped highlight is gone for good rather than waiting for a later node to pull it back.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `image` | in | Image | required; The image to operate on. Being the default input, a drag from an upstream node's body wires here, and dropping this node on an existing wire splices it in. Whatever arrives is clamped to the working resolution (2048 px on the long edge) before the operator runs, so a large source cannot stall the cook. |
| `image` | out | Image | The resulting image: RGBA8, and never more than 2048 px on the long edge, because that is the resolution the texture context cooks at. Being the default output, a drag from the node's body wires from here. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `brightness` | float | `0.0` | -1 to 1 | Added to every channel after the contrast scale, where 0 is identity. It is a flat offset on the stored value, not an exposure multiply, so it lifts shadows exactly as much as highlights and clips at the ends. |
| `contrast` | float | `0.0` | -1 to 1 | Scales each channel away from the 0.5 pivot, where 0 is identity. The multiplier is 1 plus this value, so 1 doubles the spread and -1 flattens the image to mid-gray entirely. |

*Bypassed: passes `image` straight through.*

### Gamma <a id="gamma"></a>

`gamma` · v1 · Adjust · placed inside a texture network

Palette search also matches: gamma, curve.

Raises every RGB channel to the power 1/gamma through a 256-entry lookup table. 1 is identity, and alpha is untouched.

The single-knob midtone bend, and the same curve `levels` applies in the middle of its chain. Reach for it when the bend is all you want and for `levels` when you also need the black and white points. It is the usual correction between an image authored to be looked at and one about to be read as data, e.g. a height field on its way into `height_to_normal`.

The ends are pinned: 0 stays 0 and 1 stays 1 whatever the value, so this only redistributes the middle and can never clip. Above 1 brightens midtones and below 1 darkens them, which is the reverse of what someone thinking of gamma as a plain exponent expects -- the exponent actually applied is the reciprocal.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `image` | in | Image | required; The image to operate on. Being the default input, a drag from an upstream node's body wires here, and dropping this node on an existing wire splices it in. Whatever arrives is clamped to the working resolution (2048 px on the long edge) before the operator runs, so a large source cannot stall the cook. |
| `image` | out | Image | The resulting image: RGBA8, and never more than 2048 px on the long edge, because that is the resolution the texture context cooks at. Being the default output, a drag from the node's body wires from here. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `gamma` | float | `1.0` | 0.1 to 4 | The curve, applied as the power 1/gamma, where 1 is identity. Above 1 lifts midtones, below 1 deepens them; black and white stay put either way. Hard-floored at 0.1 so the curve cannot stand up vertical. |

*Bypassed: passes `image` straight through.*

### Hue / Saturation <a id="hue_saturation"></a>

`hue_saturation` · v1 · Adjust · placed inside a texture network

Palette search also matches: hue, saturation, hsl, color.

Converts each pixel to HSL, shifts the hue, multiplies saturation and lightness, and converts back. Alpha is untouched.

The color-side counterpart to `levels`. Reach for it to retint an imported albedo, to pull the color out of an image on the way to a mask (Saturation 0), or to make color variants of one texture network by shifting hue alone.

Saturation and Lightness are multipliers, not offsets, and that bites in two places: Lightness cannot lift a pure black pixel, because zero times anything is still zero, and Hue Shift does nothing whatever to a gray or white pixel, because a pixel with no saturation has no hue to shift. This is also the one adjust node with no lookup table behind it: it runs the HSL round trip per pixel.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `image` | in | Image | required; The image to operate on. Being the default input, a drag from an upstream node's body wires here, and dropping this node on an existing wire splices it in. Whatever arrives is clamped to the working resolution (2048 px on the long edge) before the operator runs, so a large source cannot stall the cook. |
| `image` | out | Image | The resulting image: RGBA8, and never more than 2048 px on the long edge, because that is the resolution the texture context cooks at. Being the default output, a drag from the node's body wires from here. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `hue` | float | `0.0` | -180 to 180 | Rotates the hue wheel by this many degrees; 0 is identity and the rotation wraps, so -180 and 180 land in the same place. It can do nothing to a pixel with no saturation: gray has no hue to rotate. |
| `saturation` | float | `1.0` | 0 to 2 | Multiplies HSL saturation, where 1 is identity and 0 desaturates to gray. Above 1 pushes toward pure hue, but the result clamps at 1, so heavy values flatten distinct colors into the same fully-saturated tone. |
| `lightness` | float | `1.0` | 0 to 2 | Multiplies HSL lightness, where 1 is identity and 0 goes to black. Because it multiplies rather than offsets, it can never lift a pure black pixel off zero, and it clamps at 1 on the way up. |

*Bypassed: passes `image` straight through.*

### Invert <a id="invert"></a>

`invert` · v1 · Adjust · placed inside a texture network

Palette search also matches: invert, negative.

Replaces every RGB channel with 255 minus its stored value. Alpha rides through untouched, and there are no parameters.

The mask flipper. Most of its use is between a `ramp` or `noise` and whatever consumes the mask, when the falloff points the wrong way, or ahead of a `mix` to swap which side of a blend a mask selects. It is also the quickest way to turn a height field into its own inverse before `height_to_normal`.

It inverts the stored sRGB-encoded bytes rather than linear light, so this is the photo-editor negative, not a photometric one. Applying it twice returns the original image exactly, which the imaging crate's tests pin.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `image` | in | Image | required; The image to operate on. Being the default input, a drag from an upstream node's body wires here, and dropping this node on an existing wire splices it in. Whatever arrives is clamped to the working resolution (2048 px on the long edge) before the operator runs, so a large source cannot stall the cook. |
| `image` | out | Image | The resulting image: RGBA8, and never more than 2048 px on the long edge, because that is the resolution the texture context cooks at. Being the default output, a drag from the node's body wires from here. |

*Bypassed: passes `image` straight through.*

### Levels <a id="levels"></a>

`levels` · v1 · Adjust · placed inside a texture network

Palette search also matches: levels, tone, remap, histogram.

Photoshop-style levels over the RGB channels: Input Black and Input White stretch to the full range, a midtone Gamma bends the curve, then Output Black and Output White compress the result back into a range.

The main tonal tool of a texture network. Reach for it to set the range of a `noise` or `ramp` before it becomes a mask, or to fix a flat imported image. `gamma` is the same midtone move with none of the range controls; `brightness_contrast` is the blunter, symmetric version.

It works on the stored sRGB-encoded bytes, the convention of 2D image editors, not on linear light, so the numbers match what Photoshop would show rather than what a shader would compute. The whole curve collapses into a 256-entry lookup table applied per channel, so the cost is the same whatever the settings, and alpha is never touched.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `image` | in | Image | required; The image to operate on. Being the default input, a drag from an upstream node's body wires here, and dropping this node on an existing wire splices it in. Whatever arrives is clamped to the working resolution (2048 px on the long edge) before the operator runs, so a large source cannot stall the cook. |
| `image` | out | Image | The resulting image: RGBA8, and never more than 2048 px on the long edge, because that is the resolution the texture context cooks at. Being the default output, a drag from the node's body wires from here. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `in_black` | float | `0.0` | 0 to 1 | Input at or below this maps to full black before the rest of the curve runs. Raising it crushes shadows and adds contrast. Push it past Input White and the two collapse into a hard threshold at this value rather than misbehaving. |
| `in_white` | float | `1.0` | 0 to 1 | Input at or above this maps to full white. Lowering it blows out highlights and adds contrast. It is always held a hair above Input Black, so the pair can never divide by zero; they threshold instead. |
| `gamma` | float | `1.0` | 0.1 to 4 | Bends the midtones after the input range is normalized: the value is raised to the power 1/gamma, so above 1 lifts midtones and below 1 deepens them. 1 is a straight line, and the black and white ends stay pinned whatever you set here. |
| `out_black` | float | `0.0` | 0 to 1 | Where full black lands once the curve has run. Raise it to lift the whole image off black, the usual way to fake a washed-out or hazy look. |
| `out_white` | float | `1.0` | 0 to 1 | Where full white lands once the curve has run. Lower it to pull the image off white. Set it below Output Black and the output range runs backwards, inverting the image with the rest of the curve still applied. |

*Bypassed: passes `image` straight through.*

## Attribute

### Attribute Copy <a id="attribute_copy"></a>

`attribute_copy` · v1 · Attribute · placed inside a geo

Palette search also matches: copy, rename, attribute, convert, cast, color.

Copies an attribute lane under a new name, optionally converting its type, in the point or primitive domain. With Delete Source it is a rename.

The headline use is feeding the reserved lanes: copy any vec3 into `color` and the geometry displays vertex-colored (w pads to 1.0, opaque); copy a vec3 into `N` to override normals. The other direction works too: narrow a `color` to a float magnitude lane and drive `displace` with it.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `geometry` | in | Geometry | required; The geometry carrying the lane. |
| `geometry` | out | Geometry | The cooked geometry. Being the default output, a drag from the node's body wires from here, and a bypass passes the input through it. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `source` | attributeName | `` |  | The lane to copy, resolved in the chosen domain. |
| `dest` | text | `` |  | The name the copy is written under, replacing any lane already there. Reserved names activate their contracts: `color` (vec4) displays as vertex color immediately. |
| `domain` | enum (point / primitive) | `point` |  | Which domain both names live in. Use `attribute_promote` to move a lane between domains. |
| `target_type` | enum (auto / float / vec2 / vec3 / vec4) | `auto` |  | The copy's type. Widening pads (a vec4's w fills with 1.0, the color case); narrowing to Float takes the magnitude; other narrowing drops trailing components. |
| `delete_source` | bool | `false` |  | Remove the source lane after copying, making this a rename. Deleting the reserved `N` or `uv` clears the mesh's FIXED normal/uv buffer when no map lane shadows it, which changes shading and texturing downstream. |

*Bypassed: passes `geometry` straight through.*

### Attribute Create <a id="attribute_create"></a>

`attribute_create` · v1 · Attribute · placed inside a geo

Palette search also matches: attribute, lane, constant, color, tag.

Writes a constant attribute lane onto every point of the input, replacing any lane already under that name. Attributes are named per-point values that ride the geometry through the graph; downstream nodes consume them by name.

The reserved names are where this shows immediately: write `color` as a vec4 and the geometry displays vertex-colored; write `N` as a vec3 and `copy_to_points` orients to it; `uv` (vec2) feeds texturing. Any other name is free-form data for your own downstream use.

Writing a reserved name with the wrong type is legal but inert, and the node warns rather than guessing. For seeded per-point variation instead of a constant, reach for `attribute_randomize`.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `geometry` | in | Geometry | required; The geometry the lane is written onto. |
| `geometry` | out | Geometry | The cooked geometry. Being the default output, a drag from the node's body wires from here, and a bypass passes the input through it. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `attr_name` | attributeName | `value` |  | The lane's name. Free-form names are yours to consume downstream; the reserved names carry contracts: `color` (vec4) drives vertex-color display, `N` (vec3) is the point normal copies orient to, `uv` (vec2) the texture coordinate, `pscale` (float) a per-point scale reserved for later. |
| `type` | enum (float / vec2 / vec3 / vec4) | `float` |  | The lane's component count. Pick the type a reserved name's consumers expect (vec4 for `color`); the value parameter below follows the choice. |
| `value_float` | float | `1.0` |  | shown only while `type` is `float`; The constant every point receives. |
| `value_vec2` | vec2 | `[0.0,0.0]` |  | shown only while `type` is `vec2`; The constant every point receives. |
| `value_vec3` | vec3 | `[0.0,0.0,0.0]` |  | shown only while `type` is `vec3`; The constant every point receives. |
| `value_vec4` | vec4 | `[1.0,1.0,1.0,1.0]` |  | shown only while `type` is `vec4`; The constant every point receives. As a `color` it is linear RGBA: opaque white by default. |

*Bypassed: passes `geometry` straight through.*

### Attribute Promote <a id="attribute_promote"></a>

`attribute_promote` · v1 · Attribute · placed inside a geo

Palette search also matches: promote, domain, primitive, point, demote.

Converts an attribute lane between the point and primitive domains: per-corner values combine into one value per triangle, segment, or point primitive (Average / Min / Max / First), and primitive values spread back onto their points, averaging where a point belongs to several primitives.

By default the lane MOVES to the destination domain; Keep Original leaves the source in place too. A point untouched by any primitive receives zeros on a primitive-to-point promote. The Attributes pane's Point and Primitive tabs show both domains, which is the quickest way to watch this node work.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `geometry` | in | Geometry | required; The geometry whose lane changes domain. |
| `geometry` | out | Geometry | The cooked geometry. Being the default output, a drag from the node's body wires from here, and a bypass passes the input through it. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `attr_name` | attributeName | `` |  | The lane to promote, resolved in the source domain. |
| `direction` | enum (pointToPrimitive / primitiveToPoint) | `pointToPrimitive` |  | Which way the lane moves: corner-point values combining into one value per primitive, or primitive values spreading onto their points. |
| `method` | enum (average / min / max / first) | `average` |  | How the several source values landing on one destination element combine, component-wise for vector lanes. |
| `keep_original` | bool | `false` |  | Keep the source-domain lane beside the promoted one. Off (the default), the promotion MOVES the lane. The fixed normal and uv buffers are never deleted by a promote: promoting `N` or `uv` copies out of them. |

*Bypassed: passes `geometry` straight through.*

### Attribute Randomize <a id="attribute_randomize"></a>

`attribute_randomize` · v1 · Attribute · placed inside a geo

Palette search also matches: random, variation, jitter, noise, color, attribute.

Fills an attribute lane with seeded uniform random values, one draw per point, each component between its Min and Max. At the defaults it writes `color`, so wiring any geometry through it paints every point a different color and the result displays immediately: the quickest proof the attribute system is live.

On `scatter` output it is the variation workhorse: randomize `color` for per-point tinting, or a free-form lane that a later release's consumers read. The draw is per point, deterministic in the seed, and independent per component, so a fixed alpha is just Min equal to Max in that lane.

Writing a reserved name with the wrong type is legal but inert (the node warns): `color` wants vec4, `N` vec3, `uv` vec2, `pscale` float. It replaces any existing lane under the same name.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `geometry` | in | Geometry | required; The geometry whose points receive the randomized lane. |
| `geometry` | out | Geometry | The cooked geometry. Being the default output, a drag from the node's body wires from here, and a bypass passes the input through it. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `attr_name` | attributeName | `color` |  | The lane to fill. The default `color` drives vertex-color display immediately; `pscale` (float) and free-form names feed downstream consumers instead. |
| `type` | enum (float / vec3 / vec4) | `vec4` |  | The lane's component count; each component draws independently between its Min and Max. `color` consumers expect vec4. |
| `min_float` | float | `0.0` |  | shown only while `type` is `float`; The lower bound of the uniform draw. |
| `max_float` | float | `1.0` |  | shown only while `type` is `float`; The upper bound of the uniform draw. |
| `min_vec3` | vec3 | `[0.0,0.0,0.0]` |  | shown only while `type` is `vec3`; The per-component lower bounds of the uniform draw. |
| `max_vec3` | vec3 | `[1.0,1.0,1.0]` |  | shown only while `type` is `vec3`; The per-component upper bounds of the uniform draw. |
| `min_vec4` | vec4 | `[0.0,0.0,0.0,1.0]` |  | shown only while `type` is `vec4`; The per-component lower bounds of the uniform draw. The default pins alpha at 1 so randomized colors stay opaque. |
| `max_vec4` | vec4 | `[1.0,1.0,1.0,1.0]` |  | shown only while `type` is `vec4`; The per-component upper bounds of the uniform draw. |
| `seed` | int | `0` | 0 to 2147483647 | Selects which random values you get. Any change redraws every point; the same seed always cooks the same values, so a saved scene reproduces exactly. |

*Bypassed: passes `geometry` straight through.*

### Attribute Wrangle <a id="attribute_wrangle"></a>

`attribute_wrangle` · v1 · Attribute · placed inside a geo

Palette search also matches: wrangle, attribute, vex, expression, snippet, code, script.

Runs a small program once per point or per primitive, reading and writing attributes by name. This is the general-purpose attribute tool: where `attribute_create` writes a constant and `attribute_randomize` writes noise, a wrangle computes a lane from whatever else is on the geometry.

`@Cd = set(@P.x + 0.5, @P.y + 0.5, @P.z + 0.5);` colours the geometry by position and shows immediately, because `@Cd` is the reserved colour lane the viewport already displays. `@P = set(@P.x, @P.y + sin(@P.x * 4 + $T), @P.z);` ripples the surface, and animates once playback is running.

A parse error names the line and column and badges the node. An arithmetic edge such as division by zero is not an error: it yields the IEEE result, so one bad element cannot blank a scene.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `geometry` | in | Geometry | required; The geometry the program runs over. |
| `geometry` | out | Geometry | The cooked geometry. Being the default output, a drag from the node's body wires from here, and a bypass passes the input through it. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `domain` | enum (point / primitive) | `point` |  | Which elements the program runs once for. Points is the usual choice and the only domain that can move geometry, because `@P` is a point attribute. Primitives runs once per triangle, segment or point primitive and reaches the primitive lanes `attribute_promote` writes. |
| `program` | snippet | `@Cd = set(@P.x + 0.5, @P.y + 0.5, @P.z + 0.5);` |  | Statements separated by `;`, each assigning to an `@attribute` or a local. Reads the same maths the expression language offers: around thirty builtins, `$T` for scene time, `ch("box1/width")` to read another node's parameter, and `npoints()` or `bbox("size")` for the incoming geometry.

The element scope is `@P` (position), `@N` (normal), `@Cd` (colour), `@uv`, plus `@ptnum` / `@numpt` (or `@primnum` / `@numprim`) and any lane on the input. Declare locals with `float`, `vector2`, `vector` or `vector4`.

There is no `if` and no `for`; use the `? :` conditional for a branching value. A lane the input does not carry is created at the width of its first assignment. |

*Bypassed: passes `geometry` straight through.*

### Attribute from Image <a id="attribute_from_image"></a>

`attribute_from_image` · v1 · Attribute · placed inside a geo

Palette search also matches: sample, map, texture, image, bake, vertex color.

Samples the connected image through each point's UV and writes the result into an attribute lane: full RGBA into `color` for vertex-color display, or one channel into a float lane.

This is the image-to-geometry bridge: build a map in a texture network, `tex_ref` it into the geometry graph, sample it here, and drive `displace` (or anything else that reads lanes) with the result. Sampling matches the renderer's orientation exactly, so the written colors line up with the same image textured on the surface.

Meshes without the UV lane, or an unwired image, pass through with a warning.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `geometry` | in | Geometry | required; the default input; The geometry the sampled lane is written onto. |
| `image` | in | Image | The image to sample. Wire a `tex_ref` (pointing at a texture network) or an `import_image`. Unwired, the geometry passes through with a warning. |
| `geometry` | out | Geometry | The cooked geometry. Being the default output, a drag from the node's body wires from here, and a bypass passes the input through it. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `attr_name` | text | `color` |  | The lane the samples are written under. The default `color` displays as vertex color immediately; any other name is free-form data for downstream nodes. |
| `uv_attr` | attributeName | `uv` |  | The vec2 point lane supplying sample coordinates; `uv` resolves the mesh's texture coordinates. A mesh without it passes through with a warning (chain `uv_project` first). |
| `channels` | enum (rgba / rgb / luminance / r / g / b / a) | `rgba` |  | What lands in the lane: the full color (vec4, the `color` contract), RGB (vec3), or one scalar channel. Luminance uses the Rec. 709 weights. |
| `filter` | enum (bilinear / nearest) | `bilinear` |  | Blend the four surrounding texels, or read the nearest one. |
| `wrap` | enum (repeat / clamp) | `repeat` |  | How UVs outside 0..1 resolve: tile the image, or extend its edges. |
| `srgb` | bool | `true` |  | Convert the sampled RGB to linear before writing (alpha is untouched). Keep it on for color images: the reserved `color` lane is linear RGBA by contract. Turn it off for data maps (height, masks) whose bytes are already linear. |

*Bypassed: passes `geometry` straight through.*

### Compute Normals <a id="compute_normals"></a>

`compute_normals` · v2 · Attribute · placed inside a geo

Palette search also matches: normals, recompute, smooth.

Rebuilds every mesh's vertex normals from its triangles. Each triangle's geometric normal is accumulated onto the three points it touches and the sum is normalized, so a point shared by several triangles ends up carrying their area-weighted average.

Reach for it when an import arrives with no normals, or with normals that disagree with the surface, or after something upstream left them stale. The `validate` node's Normals check reports exactly what this clears, so the two pair naturally: validate to see the problem, compute_normals to fix it.

It can only smooth where points are actually shared. Primitives split their corners so that each face carries its own copy -- a box has 24 points for 8 corners -- and recomputing normals on one leaves it just as flat-shaded as before. Smooth shading out of split geometry needs the points welded first, which this node does not do. Winding and face normals are triangle concepts: point clouds and polylines pass through untouched with a warning.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `geometry` | in | Geometry | required; The geometry whose normals to recompute. |
| `geometry` | out | Geometry | The cooked geometry. Being the default output, a drag from the node's body wires from here, and a bypass passes the input through it. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `flip_orientation` | bool | `false` |  | Reverses triangle winding and negates normals (fixes inside-out meshes). |

*Bypassed: passes `geometry` straight through.*

### UV Project <a id="uv_project"></a>

`uv_project` · v1 · Attribute · placed inside a geo

Palette search also matches: uv, unwrap, project, texture, mapping.

Writes a fresh UV set onto every mesh in the input using one of four projections, normalized over the whole set's bounding box so that a scale of 1 lands the geometry inside 0..1.

Imports very often arrive with no UVs at all, and texel density, the checker pattern, and any textured material all need them. This is the node that gives them something to read: it goes after the import or the primitive and before the material. It is a projection rather than an unwrap, so treat it as the fast way to usable UVs, not as a substitute for a real layout.

Three things to expect. Existing UVs are replaced, not merged. The normalization is against the bounds of the whole input set, so several meshes share one consistent mapping and a `transform` upstream drags the UVs along with the bounds. And Box mode rebuilds each mesh non-indexed, three points per triangle, so the point count jumps and validate's topology counts will reflect it. Projection is a surface operation: point clouds and polylines pass through untouched with a warning.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `geometry` | in | Geometry | required; The geometry to unwrap. Every mesh in the set is projected, and any UVs a mesh already carried are overwritten rather than kept. |
| `geometry` | out | Geometry | The cooked geometry. Being the default output, a drag from the node's body wires from here, and a bypass passes the input through it. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `mode` | enum (planar / box / cylindrical / spherical) | `planar` |  | Which shape maps position onto UV. Planar flattens the geometry along one axis and is the honest choice for anything roughly flat. Box gives each triangle the planar mapping of whichever axis its face normal is closest to, so a hard-surface model gets a sensible mapping on all six sides at once. Cylindrical turns the angle around the axis into u and the height along it into v. Spherical uses longitude and latitude about the axis. Cylindrical and Spherical wrap, and a triangle that straddles the wrap seam smears the whole texture across itself. |
| `axis` | enum (x / y / z) | `y` |  | The axis the projection is built around: Planar projects along it, Cylindrical wraps about it, Spherical takes it as the pole. Box uses all three axes by construction, so this does nothing at all in that mode. |
| `scale` | vec2 | `[1.0,1.0]` |  | Multiplies the normalized UVs. 1 fits the geometry's bounds into 0..1 exactly; 2 tiles the texture twice across it; 0.5 uses half of it. Applied before Offset. |
| `offset` | vec2 | `[0.0,0.0]` |  | Slides the UVs after Scale, in UV units, so 1 shifts by a full tile. Use it to line a texture up on the surface, not to resize it. |

*Bypassed: passes `geometry` straight through.*

## Cameras

### Camera <a id="camera"></a>

`camera` · v3 · Cameras · placed scene · camera silhouette

Palette search also matches: camera, view, cam, lens.

A camera you can look through, pane by pane: an eye at Position aimed at Target, projecting as perspective, orthographic, or physical (lens).

Author a shot here instead of leaving it in a viewport you will orbit away from. Lock a pane to the camera, frame it, and screenshots and turntable exports through that pane use it. Like the light nodes it is portless and lives in the root graph beside your `geo` containers: the scene builder reads its params directly, so there is no wire to connect and nothing downstream of it.

Near Clip and Far Clip currently do nothing -- a pane takes this camera's position, aim, field of view and projection, but derives its own clip planes from the orbit distance. The lens controls are mutually exclusive by Projection (Field of View for perspective, Focal Length and Sensor Width for physical, Ortho Scale for orthographic), so a control you are looking for and cannot find is usually hidden behind a different Projection. Up is always world Y: there is no roll.

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `position` | vec3 | `[7.0,5.0,7.0]` |  | meters; The eye, in metres: where the camera sits. With Target it fully determines the view -- there is no roll control, because up is always world Y, so a camera authored here can never be dutched. |
| `target` | vec3 | `[0.0,0.0,0.0]` |  | meters; The point the camera aims at, in metres. Position and Target together give both the aim and the distance, so on a perspective camera moving either one reframes the shot; on an orthographic camera only the aim matters and Ortho Scale does the framing. |
| `kind` | enum (perspective / orthographic / physical) | `perspective` |  | Which projection the camera uses, and therefore which lens control applies. Perspective takes Field of View directly. Physical (lens) computes that same field of view from Focal Length and Sensor Width, for when you would rather think in millimetres. Orthographic drops perspective altogether and frames by Ortho Scale, keeping parallel lines parallel for an elevation or a technical view. Switching this only hides and shows the controls below; the values you set are kept, so you can flip back and forth without losing a lens. |
| `fov_y` | float | `45.0` | 1 to 179 | degrees; shown only while `kind` is `perspective`; The VERTICAL angle the camera sees, in degrees. Smaller is tighter and flatter, larger is wider and more distorted at the edges; past about 90 the stretch in the corners gets hard to miss. Perspective only -- a physical camera derives this same number from Focal Length and Sensor Width instead, and an orthographic camera has no field of view at all. |
| `focal_length` | float | `50.0` | 1 to 2000 | shown only while `kind` is `physical`; The lens focal length in millimetres, as a photographer means it: 50 normal, 24 wide, 200 a long telephoto. It only means something against a sensor size -- together with Sensor Width it sets the field of view, as fov = 2 * atan(sensor / (2 * focal)), so a LONGER focal length gives a NARROWER view. Physical projection only. See Sensor Width for how that formula differs from a real camera's. |
| `sensor_width` | float | `36.0` | 1 to 100 | shown only while `kind` is `physical`; The film-back width in millimetres: 36 is full-frame 35mm, 25 is Super 35, 23.5 is APS-C. A larger sensor at the same Focal Length sees more, which is why the same lens is wide on full-frame and long on a phone. One caveat worth knowing: this value drives the VERTICAL field of view, where a real camera would use its sensor HEIGHT. A 50mm at 36mm here frames about 40 degrees tall; a real full-frame camera, working from its 24mm sensor height, frames about 27. So a physical camera reads wider than the equivalent real lens -- compensate with a longer Focal Length or a smaller Sensor Width. |
| `ortho_scale` | float | `5.0` | 0.001 to 100000 | meters; shown only while `kind` is `orthographic`; Half the visible height, in metres: at the default 5 the camera frames 10 metres top to bottom, and the width follows from the pane's shape. This is the orthographic stand-in for zoom. With no perspective, moving the camera closer changes nothing about the framing, so this is the only way to fit more or less in frame. Orthographic only. |
| `f_stop` | float | `0.0` | 0 to 128 | shown only while `kind` is not `orthographic`; How wide the aperture opens, as a photographer's f-number: the focal length divided by the opening's diameter. Smaller is wider, so f/1.4 throws almost everything out of focus and f/16 holds nearly all of it sharp.

0, the default, is a pinhole: everything is sharp at every distance, which is what a computer-generated image does unless told otherwise and what every camera made before this control existed still does. Set anything above 0 and Focus Distance starts to matter.

On a physical camera the f-number works against Focal Length directly. On a perspective camera there is no focal length to work against, so one is derived back out of Field of View against the same 36mm the Sensor Width control describes, which is what makes the same f-number mean the same blur on both.

Read by rendered output only: the interactive viewport draws through a pinhole whatever this says, because a rasterizer has one sample per pixel and no aperture to integrate over. |
| `focus_distance` | float | `0.0` | 0 to 100000 | meters; shown only while `kind` is not `orthographic`; How far in front of the camera is sharp, in metres. Everything nearer or further blurs, and how fast it blurs is F-Stop's job.

0, the default, focuses on Target, so aiming the camera also focuses it and opening the aperture does something sensible immediately. Set a number to override that and focus on a fixed distance instead, which is what you want when the subject is not what the camera is pointed at.

It does nothing while F-Stop is 0, because a pinhole has nothing to focus. |
| `aperture_blades` | int | `0` | 0 to 16 | shown only while `kind` is not `orthographic`; How many blades the iris has, which is the shape an out-of-focus highlight takes: 6 blades give hexagonal bokeh, 8 octagonal. 0, the default, is a perfectly circular opening, which no real lens has and every one approaches wide open. Values of 1 and 2 are treated as circular, there being no polygon with fewer than three sides.

It does nothing while F-Stop is 0: a pinhole has no opening to shape. |
| `near` | float | `0.1` | 0.0001 to 100000 | meters; How close a surface may come before it is clipped away, in metres. This control does nothing today: a pane looking through this camera takes its position, aim, field of view and projection, but derives its own near and far planes from the orbit distance and never reads this value. It is resolved and saved with the document; it will not change the image. |
| `far` | float | `1000.0` | 0.001 to 1000000 | meters; How far a surface may be before it is clipped away, in metres. Like Near Clip, this control does nothing today -- the pane derives its own clip planes from the orbit distance and ignores this value. |
| `aspect` | float | `1.7777777777777777` | 0.1 to 10 | The framing aspect, width over height. It does not change what the camera sees -- the pane's own shape does that -- it draws the framing gate: a rectangle inset in any pane locked to this camera, marking what a render at this aspect would keep. It also sets the shape of the film back on the camera gizmo. 16/9 by default. |
| `show_gizmo` | bool | `true` |  | Draw this camera's wireframe frustum in the viewport: the film back at its Aspect, four edges converging on the eye, and a wedge marking which way is up. A pane looking through this camera never draws its own gizmo, the way Blender hides the camera you are inside, so this is about seeing the camera from other panes. |
| `gizmo_size` | float | `1.0` | 0.1 to 10 | How big the wireframe frustum is drawn, in world metres. Purely cosmetic: it has no effect on what the camera sees or renders. Raise it when the gizmo is lost in a large scene, lower it when it swamps a small one. |
| `exposure` | float | `1.0` | 0.01 to 64 | under "Tone"; Linear multiplier on the whole image before tone mapping, so 2 is one stop brighter and 0.5 one stop darker. This is the first control to reach for when a render is broadly too dark or too bright, ahead of touching light intensities, because it moves the exposure of the shot rather than the lighting of the scene. |
| `tone` | enum (inherit / none / linear / reinhard / aces) | `inherit` |  | under "Tone"; How high dynamic range is brought down to what a screen can show. Inherit leaves the pane's own choice alone, which is the default so that adding a camera never silently restyles a scene. Set it to None when the Pre-Tonemap LUT below carries a full tone transform such as ACES or AgX, because applying both would tone map the image twice. |
| `lift` | vec3 | `[0.0,0.0,0.0]` | -1 to 1 | under "Grade"; Raises or lowers the darkest part of the image, per channel, after tone mapping. Positive values lift the blacks towards grey for a faded or filmic base; negative values crush them. Because it is an addition rather than a multiplication it moves the shadows far more than the highlights, which is what separates it from Gain. |
| `gamma` | vec3 | `[1.0,1.0,1.0]` | 0.01 to 10 | under "Grade"; Bends the midtones per channel without moving black or white: above 1 brightens them, below 1 darkens them. This is the control for an image whose ends are right and whose middle is not, and the one to reach for when a colour cast sits in the midtones rather than across the whole frame. 1 is neutral. |
| `gain` | vec3 | `[1.0,1.0,1.0]` | 0 to 10 | under "Grade"; Multiplies each channel, which moves the highlights most and leaves black at black. Use it to set the white point or to warm and cool an image by pushing the red and blue channels apart. 1 is neutral on every channel. |
| `lut_a` | assetRef | `` |  | under "Lookup tables"; A `.cube` table applied BEFORE tone mapping, on log-encoded scene light. This is the slot for a full tone transform such as ACES or AgX, which replaces the tone mapper rather than decorating it, so set Tone Map to None when you load one here. An ordinary look LUT belongs in the slot below and will look wrong in this one. |
| `lut_a_strength` | float | `1.0` | 0 to 1 | under "Lookup tables"; How far to blend towards the pre-tonemap table, from 0 for none of it to 1 for all of it. Mostly useful for checking what the table is doing by sliding it off and on; a tone transform is usually wanted at full strength. |
| `lut_b` | assetRef | `` |  | under "Lookup tables"; A `.cube` table applied AFTER tone mapping, on the finished image. This is the slot for the look LUTs people already own from a grading suite, which are authored against a display-referred picture. A tone transform belongs in the slot above and will look wrong in this one. |
| `lut_b_strength` | float | `1.0` | 0 to 1 | under "Lookup tables"; How far to blend towards the look table, from 0 for none of it to 1 for all of it. Unlike the pre-tonemap slot this one is routinely dialled back: a look at half strength is a common way to keep its character without its full contrast. |

*Bypassed: emits nothing.*

## Composite

### Blur <a id="blur"></a>

`blur` · v1 · Composite · placed inside a texture network

Palette search also matches: blur, gaussian, soften.

Separable Gaussian blur over all four channels: two 1D passes, horizontal then vertical, with edge pixels clamping to the border rather than wrapping.

The softener. Reach for it to take the hard edges off a `noise` mask, to pre-soften a height field before `height_to_normal` so the normals are not all needle, or to build a glow by blurring a bright layer and adding it back through `mix`.

Alone among the image nodes it filters alpha as well as RGB, so blurring a partly transparent image bleeds its edges. Radius counts pixels of the WORKING resolution, not of the source, so an image that got clamped on the way in blurs relatively harder than its full-size original would. It is not free: at radius 64 each of the two passes takes 129 samples per pixel.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `image` | in | Image | required; The image to operate on. Being the default input, a drag from an upstream node's body wires here, and dropping this node on an existing wire splices it in. Whatever arrives is clamped to the working resolution (2048 px on the long edge) before the operator runs, so a large source cannot stall the cook. |
| `image` | out | Image | The resulting image: RGBA8, and never more than 2048 px on the long edge, because that is the resolution the texture context cooks at. Being the default output, a drag from the node's body wires from here. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `radius` | float | `4.0` | 0 to 64 | Gaussian radius in pixels of the working resolution; sigma is half of it. 0 is a pass-through. Cost climbs linearly, each of the two passes taking 2*radius+1 samples per pixel, which is why the slider stops at 16 even though you can type up to 64. |

*Bypassed: passes `image` straight through.*

### Height to Normal <a id="height_to_normal"></a>

`height_to_normal` · v1 · Composite · placed inside a texture network

Palette search also matches: normal, height, bump, sobel.

Reads the input's red channel as a height field, takes its slope with a 3x3 Sobel filter, and writes the resulting surface normal out as a tangent-space normal map. Flat encodes as (128, 128, 255), edge pixels clamp, and alpha is opaque.

The last step of a procedural bump chain: `noise` for the height, `blur` and `levels` to shape it, then this, then the network's display node so a material can reference it by path. Feeding it an imported grayscale height map does the same job for scanned detail.

The output is a normal map, not a color: do not run the adjust nodes on it afterwards, because they operate on encoded color and will denormalize the vectors. Bypassing this node is not a no-op either -- it passes the raw HEIGHT image downstream, which a material will happily read as a normal map and shade wrong.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `image` | in | Image | required; The height field. Only the RED channel is read, as a height in 0..1; a color image works but silently uses its red. Being the default input, a body drag wires here. Blur it first if the field is noisy: a Sobel over raw noise gives needle-sharp normals. |
| `image` | out | Image | The resulting image: RGBA8, and never more than 2048 px on the long edge, because that is the resolution the texture context cooks at. Being the default output, a drag from the node's body wires from here. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `strength` | float | `4.0` | 0 to 16 | Scales the height slope before it becomes a normal: 0 emits a flat map whatever the input says, and higher values tilt the normals further from straight up. It is a gain on the gradient, not a height in metres, so the same value over a smooth field and a noisy one gives very different results. Tune it against the shaded preview. |

*Bypassed: passes `image` straight through.*

### Mix <a id="mix"></a>

`mix` · v1 · Composite · placed inside a texture network · gather silhouette

Palette search also matches: mix, blend, composite, over, multiply, screen.

Composites the Blend image onto the Image input under one of six modes, with Factor fading the result in. The output takes the base input's dimensions, and the blend is nearest-sampled to fit when the two differ.

The composite node: layers in a texture network stack through it. A `constant` or `ramp` as the base, an `import_image` or `noise` as the blend, Multiply or Overlay to combine them. Chain several to build a surface up the way you would layers in a 2D editor.

The two inputs are not interchangeable. The output always takes the base's size, every mode but Over keeps the base's alpha, and Factor always fades back toward the base -- so swapping the wires is not a no-op even in the symmetric modes. The resample is nearest-neighbour with no filtering, so match sizes when it matters.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `image` | in | Image | required; the default input; The base layer. The output takes this image's dimensions, every mode but Over keeps its alpha, and Factor always fades back toward it. Being the default input, a body drag wires here, and a bypass passes it straight through. |
| `blend` | in | Image | The layer composited onto the base. Optional: leave it unconnected and the node passes the base through untouched, whatever Mode and Factor say. When its size differs from the base it is nearest-sampled to fit, so it need not match, but it will alias if it does not. |
| `image` | out | Image | The resulting image: RGBA8, and never more than 2048 px on the long edge, because that is the resolution the texture context cooks at. Being the default output, a drag from the node's body wires from here. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `mode` | enum (normal / over / multiply / add / screen / overlay) | `normal` |  | How the two images combine before Factor fades the result. Normal replaces the base outright. Over does source-over alpha compositing and is the only mode that reads the blend image's alpha or writes a new one. Multiply darkens; Add and Screen brighten, Add clipping at 1 where Screen only approaches it; Overlay multiplies where the base is dark and screens where it is bright, pivoting at 0.5. |
| `factor` | float | `1.0` | 0 to 1 | Fades the blend in. At 0 the base passes through untouched, at 1 the mode applies at full strength. In Over it scales the blend image's alpha rather than lerping the color, so a half-factor Over of an opaque image is a half-opaque composite, not a half-blended one. |

*Bypassed: passes `image` straight through.*

### Pack ORM <a id="pack_orm"></a>

`pack_orm` · v1 · Composite · placed inside a texture network · gather silhouette

Palette search also matches: orm, pack, occlusion, roughness, metallic, gltf.

Packs three grayscale maps into the one image the renderer consumes for PBR, the glTF way: red carries occlusion, green carries roughness, blue carries metallic. Each input contributes its RED channel only, an unconnected input is filled with its constant instead, and alpha is always opaque.

The last node before a texture network feeds a material's ORM slot. Wire `noise`, `ramp` or `import_image` maps into the three inputs, or leave one out and dial its constant -- Metallic at 0 with a roughness map in green is the everyday dielectric case. Point the network's display flag at this node, then reference the network from a material by path with a `tex_ref`.

Only the red channel of each input is read, so a color image silently contributes its red and its other channels are dropped. The output takes the dimensions of the FIRST connected input in occlusion, roughness, metallic order, and the other two are nearest-sampled to fit; with nothing connected at all you get a single 1x1 pixel of the three constants.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `occlusion` | in | Image | the default input; An ambient-occlusion map; its RED channel becomes the output's red. Unconnected, the Occlusion constant fills that channel flat. Being both the default input and first in order, a body drop wires here, and when it is connected it sets the output's dimensions. |
| `roughness` | in | Image | A roughness map; its RED channel becomes the output's green. Unconnected, the Roughness constant fills that channel flat. Nearest-sampled if its size differs from whichever input set the output dimensions. |
| `metallic` | in | Image | A metallic map; its RED channel becomes the output's blue. Unconnected, the Metallic constant fills that channel flat, which is the common case: most surfaces are uniformly metal or uniformly not. |
| `image` | out | Image | The resulting image: RGBA8, and never more than 2048 px on the long edge, because that is the resolution the texture context cooks at. Being the default output, a drag from the node's body wires from here. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `occlusion` | float | `1.0` | 0 to 1 | overridden when `occlusion` is connected; The flat value packed into red when the Occlusion input is unconnected. 1 means no occlusion, which is why it is the default: an ORM map with no AO in it should not darken anything. Connecting the input neutralizes this. |
| `roughness` | float | `0.7` | 0 to 1 | overridden when `roughness` is connected; The flat value packed into green when the Roughness input is unconnected, where 0 is a mirror and 1 is fully diffuse. The 0.7 default is a plausibly matte surface. Connecting the input neutralizes this. |
| `metallic` | float | `0.0` | 0 to 1 | overridden when `metallic` is connected; The flat value packed into blue when the Metallic input is unconnected. It is effectively a binary choice: 0 for a dielectric (the default) and 1 for a metal, the values between only meaning anything where a map blends across the boundary. Connecting the input neutralizes this. |

*Bypassed: emits nothing.*

### Sharpen <a id="sharpen"></a>

`sharpen` · v1 · Composite · placed inside a texture network

Palette search also matches: sharpen, unsharp, detail.

Unsharp mask: blurs a copy of the image at a fixed 1.5 px radius, then adds the difference between the original and that blur back onto the original, scaled by Amount. RGB only; alpha is untouched.

The detail-recovery step, usually last in a chain. It earns its place after a `blur`, after an image has been clamped down to the working resolution, or on an imported image that reads soft. It is the natural opposite of `blur`, though it does not undo one.

The blur radius is not exposed: 1.5 px is baked in, so this sharpens fine detail only and cannot do the wide, halo-style sharpen a variable-radius unsharp mask would. Amount runs to 4, but the result clips at black and white, so anything already near an end of the range flattens into a hard edge instead of getting crisper. 0 is a pass-through.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `image` | in | Image | required; The image to operate on. Being the default input, a drag from an upstream node's body wires here, and dropping this node on an existing wire splices it in. Whatever arrives is clamped to the working resolution (2048 px on the long edge) before the operator runs, so a large source cannot stall the cook. |
| `image` | out | Image | The resulting image: RGBA8, and never more than 2048 px on the long edge, because that is the resolution the texture context cooks at. Being the default output, a drag from the node's body wires from here. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `amount` | float | `1.0` | 0 to 4 | Scales the high-pass detail added back, where 0 is a pass-through and 1 adds it at full strength. The radius that detail is extracted at is fixed, so this only controls how hard the fine detail is pushed. High values ring at high-contrast edges, the usual unsharp halo, and clip once the ring reaches black or white. |

*Bypassed: passes `image` straight through.*

## Container

### Geo <a id="geo"></a>

`geo` · v3 · Container · placed scene

*A container: diving in opens its geo network.*

Palette search also matches: object, container, group, subflow.

A container: one object in the scene, holding a whole geometry network inside it. It has no ports and produces no wire value. What it renders is whichever node inside carries the display flag, placed in the world by this node's transform.

Containers are how a scene stays a scene instead of one enormous graph. The object level holds geos, cameras, and lights -- the things a scene is made of -- and each geo's network holds the modelling that builds that one object. Double-click a geo to dive into its network; the breadcrumb walks you back out. Bypassing a geo takes its entire subflow out of the scene in one click.

The rendering flags live here and only here. Visible and Cast Shadow are per-object properties, so they belong to the object, not to the box or the merge inside it -- which is why a plain geometry node has no such params, and why hunting for a Visible checkbox on your `box` will not find one. The transform is the same story: it is applied to the object at draw time rather than baked into the points, so dragging a geo around never recooks the network inside it, however heavy that network is.

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `translate` | vec3 | `[0.0,0.0,0.0]` |  | meters; Where the object sits in the world, in metres from the origin. Applied after rotation and scale, so it moves the object as a finished whole. The move gizmo writes here. |
| `rotate` | vec3 | `[0.0,0.0,0.0]` |  | degrees; Euler angles in degrees, one per axis, applied in Rotate Order. With two or more axes nonzero the order changes the result, so the two params are read together. |
| `rotate_order` | enum (xyz / xzy / yxz / yzx / zxy / zyx) | `xyz` |  | The order the three Euler angles compose in. It only matters once two or more axes are nonzero -- a single-axis rotation is identical under all six orders. Match the order your DCC used if you are transcribing angles from one, or the object arrives pointing somewhere else. |
| `scale` | vec3 | `[1.0,1.0,1.0]` | 0.001 to 10000 | Per-axis scale multipliers. 1 on every axis leaves the object alone; unequal values stretch it. Multiplied by Uniform Scale, so the effective scale on each axis is this times that. |
| `uniform_scale` | float | `1.0` | 0.001 to 10000 | One multiplier over all three axes, on top of Scale. Reach for it to resize the whole object without disturbing a per-axis ratio you have already dialled in. |
| `visible` | bool | `true` |  | Whether this object is displayed. Hidden objects stay cooked, so re-show is instant. |
| `cast_shadow` | bool | `true` |  | Whether this object is drawn into the shadow map. |

*Bypassed: emits nothing.*

### Mat <a id="matnet"></a>

`matnet` · v1 · Container · placed scene

*A container: diving in opens its material network.*

Palette search also matches: matnet, material, shop, shader network.

A container for a material network. Surface nodes cook inside it, and whichever node you designate as the display node publishes its material as the network's one result.

Add one per material you want to reuse. Dive in, build a surface with `principled`, `matcap`, `toon` or `unlit`, and combine surfaces with `mix_material`. Nothing leaves on a wire: materials cross contexts by path only, so a geo-side `material` node in Reference mode is what pulls the result out, and any number of them can point at the same network.

It cooks nothing itself and cannot be bypassed. A network with no display node designated publishes nothing at all, and every `material` node referring to it fails its cook rather than falling back to a default surface.

*Bypassed: cannot be bypassed.*

### Tex <a id="texnet"></a>

`texnet` · v1 · Container · placed scene

*A container: diving in opens its texture network.*

Palette search also matches: texnet, texture, cop, image network.

A container you dive into to build an image procedurally: `constant`, `ramp`, `noise` and `import_image` as sources, then the adjust, filter and composite nodes. Whichever node inside carries the display flag publishes the network's image.

Drop one at the root next to your `geo` and `matnet` nodes, build the image inside, then consume it from a material network with a `tex_ref`, whose Texture Network param points at this container. The texture viewer pane previews the published image live while you work, and editing anything inside recooks every referrer.

The reference is a path, not a wire. This node has no ports at all, so nothing connects to it on the canvas, and it is not a scene object either -- no transform, nothing lowered into the scene delta. A texnet nothing refers to still cooks and still shows nothing.

*Bypassed: cannot be bypassed.*

## Copy & Instance

### Array <a id="array"></a>

`array` · v2 · Copy & Instance · placed inside a geo

Palette search also matches: duplicate, repeat, clone, radial, grid, instance.

Duplicates the input Count times, counting the original, either stepping each copy linearly along an offset or revolving it about an axis.

Copy Mode decides what a copy is. Instance, the default, keeps the input once and carries a placement matrix per copy, so a long fence costs one post. Bake makes every copy a real baked transform of the input, concatenated as though you had merged them yourself, with identical materials collapsing to one table entry rather than one per copy.

It replaces the branch you would otherwise wire by hand: a `transform` and a `merge` for every copy. Put it after whatever makes the single unit, a primitive or a small assembly you have already merged, and then change one number instead of rewiring.

Count includes the original, so 1 is a no-op rather than one extra copy. The radial step is Sweep divided by Count rather than by Count minus 1, which is what lets a full 360 tile evenly instead of stacking a copy on the original at the seam. And Radius defaults to 0, which leaves every radial copy sitting on the axis spinning in place: give it a radius to get a ring.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `geometry` | in | Geometry | required; The geometry to duplicate. |
| `geometry` | out | Geometry | The cooked geometry. Being the default output, a drag from the node's body wires from here, and a bypass passes the input through it. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `mode` | enum (linear / radial) | `linear` |  | How the copies are placed. Linear steps each one along a fixed offset, for fence posts and stair treads. Radial revolves them about an axis, for spokes and bolt circles, and turns each copy to follow the revolution. The mode decides which of the placement parameters below apply; the rest hide. |
| `count` | int | `3` | 1 to 512 | How many copies in total, counting the original. 1 is a no-op. |
| `offset` | vec3 | `[1.0,0.0,0.0]` |  | meters; shown only while `mode` is `linear`; The step between copies: copy i is offset by i times this. |
| `axis` | enum (x / y / z) | `y` |  | shown only while `mode` is `radial`; The axis the copies revolve about. |
| `radius` | float | `0.0` | 0 to 10000 | meters; shown only while `mode` is `radial`; How far each copy sits from the axis before it revolves. |
| `sweep` | float | `360.0` | -360 to 360 | degrees; shown only while `mode` is `radial`; The total angle the copies span. Each copy steps by sweep/count, so a full 360 tiles evenly without doubling up at the seam. |
| `copy_mode` | enum (instance / bake) | `instance` |  | Whether the copies are real geometry or placements of one prototype.

Instance keeps the input once and carries a transform per copy. Ten thousand copies of a five-thousand-triangle rock cost five thousand triangles rather than fifty million, so the copy count stops being the number you budget against.

What it costs is what the rest of the graph can see. Downstream nodes are handed the prototype and the placements, never the individual copies, so there is no per-copy attribute edit, no boolean against one copy, and no deleting the third one from the left: those copies do not exist as geometry. This is the difference between copying and instancing rather than a fast path and a slow one.

Bake makes every copy real, which is what you choose when the copies have to be edited afterwards. It is no harder to author, and it answers a different question rather than an outdated one. |

*Bypassed: passes `geometry` straight through.*

### Copy to Points <a id="copy_to_points"></a>

`copy_to_points` · v2 · Copy & Instance · placed inside a geo

Palette search also matches: instance, stamp, duplicate, clone, template, forest.

Stamps the Template input onto every point of the Points input: scatter a surface, wire the cloud in here, and a forest, a crowd, or a debris field is one node instead of a hand-wired branch per copy. Every vertex of the points input is a target whatever its topology, so mesh vertices work as well as scattered clouds.

Orient turns each copy's up axis onto its point's normal (what scatter writes), so copies stand on the surface rather than all facing the same way; Scale sizes every copy and Scale Variance adds seeded per-copy jitter for a natural, unrepeated look.

Copy Mode decides what a copy is. Instance, the default, keeps the template once and carries a transform per point, so ten thousand cones cost one cone. Bake makes every copy real geometry you can edit downstream, flattening the copies of each template mesh into one concatenated mesh so even thousands of them stay a handful of draw objects. Either way the template's materials ride along shared rather than duplicated, and a copy count whose output would exceed the eight-million primitive ceiling fails the cook before anything is allocated, with a message naming the running mode and the way out.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `template` | in | Geometry | required; The geometry stamped at every point: a primitive or any small assembly you have already merged. |
| `points` | in | Geometry | required; the default input; Where the copies land: every vertex of every input mesh is a target, whatever its topology. Scatter output is the canonical source. |
| `geometry` | out | Geometry | The cooked geometry. Being the default output, a drag from the node's body wires from here, and a bypass passes the input through it. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `orient` | enum (none / normal) | `normal` |  | How each copy turns at its point. Normal rotates the template's up axis onto the point's normal, so copies stand on the surface they were scattered over; points without a normal keep the template orientation. None keeps every copy axis-aligned. |
| `scale` | float | `1.0` | 0.001 to 1000 | A uniform size factor applied to every copy before it lands.

If the points carry a `pscale` float attribute, each copy multiplies this by its own point's value, so this stays the global dial while the attribute varies around it. Author one with `attribute_wrangle`: `@pscale = fit(rand(@ptnum), 0, 1, 0.4, 1.6);` gives a scatter of mixed sizes. Points without the lane copy at this size exactly. |
| `scale_variance` | float | `0.0` | 0 to 0.95 | Per-copy size jitter as a fraction of Scale: 0.2 lets each copy vary twenty percent bigger or smaller, seeded so the variation reproduces exactly. 0 keeps every copy the same size. |
| `seed` | int | `0` | 0 to 2147483647 | Selects which per-copy size jitter you get when Scale Variance is above zero. The same seed always cooks the same sizes. |
| `copy_mode` | enum (instance / bake) | `instance` |  | Whether the copies are real geometry or placements of one prototype.

Instance keeps the input once and carries a transform per copy. Ten thousand copies of a five-thousand-triangle rock cost five thousand triangles rather than fifty million, so the copy count stops being the number you budget against.

What it costs is what the rest of the graph can see. Downstream nodes are handed the prototype and the placements, never the individual copies, so there is no per-copy attribute edit, no boolean against one copy, and no deleting the third one from the left: those copies do not exist as geometry. This is the difference between copying and instancing rather than a fast path and a slow one.

Bake makes every copy real, which is what you choose when the copies have to be edited afterwards. It is no harder to author, and it answers a different question rather than an outdated one. |

*Bypassed: passes `points` straight through.*

### Mirror <a id="mirror"></a>

`mirror` · v1 · Copy & Instance · placed inside a geo

Palette search also matches: reflect, symmetry, flip.

Reflects the input across the axis-aligned plane sitting at Offset along the chosen axis. The reflection carries the normals to their mirrored directions on its own, but it also reverses which way round each triangle reads, so the node swaps the winding back: the reflected half comes out facing outward with nothing left to repair. Keep Original merges both halves into one set, original first.

This is the symmetry workflow. Model one half, mirror the other, and the two stay in sync as you keep editing upstream. It sits at the end of the half you built, before the `merge` or `validate` that sees the whole model.

Offset is where the mirror is, not where the copy lands: a box spanning -0.5 to 0.5 mirrored across x = 3 comes out at 5.5 to 6.5, because a reflection maps x to twice the offset minus x. Nothing is welded either, so mirroring a model that already crosses the plane leaves you two overlapping surfaces down the middle.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `geometry` | in | Geometry | required; The geometry to reflect. |
| `geometry` | out | Geometry | The cooked geometry. Being the default output, a drag from the node's body wires from here, and a bypass passes the input through it. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `axis` | enum (x / y / z) | `x` |  | The axis the mirror plane is perpendicular to. |
| `offset` | float | `0.0` | -10000 to 10000 | meters; Where the mirror plane sits along the axis. |
| `keep_original` | bool | `true` |  | Merge the reflection with the original instead of replacing it. |

*Bypassed: passes `geometry` straight through.*

### Scatter <a id="scatter"></a>

`scatter` · v2 · Copy & Instance · placed inside a geo

Palette search also matches: points, distribute, sprinkle, sample, random, spray.

Sprinkles Count random points over the input's triangle surfaces and outputs them as a point cloud. Placement is area-weighted: a big face receives proportionally more points than a small one, so density stays even no matter how the surface happens to be triangulated, and the same Seed always reproduces the same arrangement.

Each point inherits the surface under it: the interpolated normal (so copies can orient to the surface downstream), the UV, and the vertex color when the source carries one. Feed the cloud to `copy_to_points` to stamp a template onto every point, or use it directly as a visible dressing of the surface.

Only triangles have area, so line and point inputs scatter nothing and the node warns instead of guessing. Points draw at a uniform screen-space size and are unpickable in the viewport; select them on the node canvas.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `geometry` | in | Geometry | required; The surface to scatter points over. Only triangle meshes have area; line and point inputs contribute nothing. |
| `geometry` | out | Geometry | The cooked geometry. Being the default output, a drag from the node's body wires from here, and a bypass passes the input through it. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `count` | int | `100` | 1 to 1000000 | How many points to place. The hard ceiling of one million keeps a mistyped count from stalling the cook; past about ten thousand, expect the point display itself to become the cost. |
| `seed` | int | `0` | 0 to 2147483647 | Selects which random placement you get. Any change gives a completely different arrangement rather than a shifted one, so scrub it to hunt for one you like. The same seed always cooks the same points, which is what lets a saved scene reproduce exactly. |
| `density` | attributeName | `` |  | A float point attribute that biases WHERE the points land. Empty (the default) scatters by area alone. Named, each triangle is weighted by its area times the mean of the attribute at its three corners, so twice the value gathers roughly twice the points.

Author it with `attribute_wrangle`: `@density = fit(@P.y, 0, 1, 0, 1);` gathers points toward the top of a surface, and `@density = @Cd.r;` follows a texture already on the geometry. Zero means never; negative clamps to zero rather than flipping the weight. |

*Bypassed: passes `geometry` straight through.*

## Export

### Export Geometry <a id="geo_export"></a>

`geo_export` · v2 · Export · placed inside a geo · terminal silhouette

Palette search also matches: export, save, file, obj, stl, ply, gltf, glb, rop.

Writes the geometry reaching it out to a file -- OBJ, STL, PLY, or GLB -- and passes that same geometry on unchanged. The output is the input by refcount, not a copy.

Because it passes through, it taps a chain rather than terminating one: drop it partway down and the nodes after it never notice. That makes it cheap to leave several in a network at the points worth exporting, each with its own format and name, and press whichever you need. Saving is a button, not a side effect of cooking -- the node never writes a file on its own.

Materials and vertex colors travel with the geometry: GLB embeds the full material table with its textures and carries colors as COLOR_0, OBJ with materials arrives as an OBJ + MTL + textures archive, and PLY writes color properties. Point clouds and polylines export as true point and line primitives in GLB and OBJ and as face-less vertices in PLY; STL is triangle facets only and skips them. Include Materials turns the material side off for a bare-geometry file. The button exports what the node last cooked, so it reports having nothing to export rather than writing an empty file.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `geometry` | in | Geometry | required; The geometry to write out. It also leaves by the output port untouched, so this node taps a chain rather than ending it. Unconnected, there is nothing to export. |
| `geometry` | out | Geometry | The cooked geometry. Being the default output, a drag from the node's body wires from here, and a bypass passes the input through it. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `format` | enum (obj / stl / ply / glb) | `glb` |  | Which writer encodes the file. glTF Binary is the default and keeps the most: one file with positions, normals, UVs, vertex colors, materials with embedded textures, and true point/line primitives. OBJ keeps geometry as text with `p`/`l`/`f` records; with materials it becomes an OBJ + MTL + textures archive. PLY merges every mesh into one vertex/face list, writes normals, UVs, and colors when every mesh has them, and a pure point cloud exports face-less. STL is the lossiest: triangle facets and nothing else, so point and line geometry is skipped. |
| `include_materials` | bool | `true` |  | Whether materials leave with the geometry. On, GLB embeds the material table with its textures and OBJ delivers the MTL sidecar archive. Off exports bare geometry in every format: smaller files, single-file OBJ, and no texture re-encoding. |
| `filename` | text | `export` |  | The base name for the saved file, without an extension -- the chosen format supplies that. Left empty it falls back to 'export'. |
| `save` | action | `false` |  | Encodes what this node last cooked and hands it to the browser's save dialog. It is a button, not a setting: nothing is stored, and nothing is written until you press it and pick a destination. It exports the current cooked result, so a node that has not cooked yet reports that there is nothing to export rather than writing an empty file. |

*Bypassed: passes `geometry` straight through.*

### Export Image <a id="image_export"></a>

`image_export` · v1 · Export · placed inside a texture network · terminal silhouette

Palette search also matches: export, save, file, png, jpeg, image.

Writes the image reaching it out to a PNG or JPEG file, and passes that same image on unchanged. The counterpart to `geo_export`, for texture networks.

It taps a chain rather than terminating one, so it drops in partway down a texture network without disturbing what follows: leave one at each stage worth baking out. Saving is a button, never a side effect of cooking, so a texture network does not litter your disk while you tweak it.

It exports at whatever resolution the chain cooked at, which is the working resolution and not necessarily the one you want on disk -- if you need a specific size, set it upstream. Choosing JPEG discards the alpha channel outright, flattening transparency, which matters when the map you are baking uses it.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `image` | in | Image | required; The image to write out. It also leaves by the output port untouched, so this node taps a texture chain rather than ending it. Unconnected, there is nothing to export. |
| `image` | out | Image | The input image, passed straight through. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `format` | enum (png / jpg) | `png` |  | PNG is lossless and keeps the alpha channel: the right default for a map that will drive a material. JPEG is lossy and has no alpha -- transparency is flattened away on write -- so keep it for reference images and previews rather than working textures. |
| `quality` | int | `90` | 1 to 100 | shown only while `format` is `jpg`; The JPEG quality factor, 1 to 100: higher is a bigger file with fewer compression artifacts. Only read when Format is JPEG, and hidden otherwise, since PNG has no equivalent knob. |
| `filename` | text | `texture` |  | The base name for the saved file, without an extension -- the chosen format supplies that. Left empty it falls back to 'export'. |
| `save` | action | `false` |  | Encodes what this node last cooked and hands it to the browser's save dialog. It is a button, not a setting: nothing is stored, and nothing is written until you press it and pick a destination. It exports the current cooked result, so a node that has not cooked yet reports that there is nothing to export rather than writing an empty file. |

*Bypassed: passes `image` straight through.*

### Render <a id="render"></a>

`render` · v3 · Export · placed scene · terminal silhouette

Palette search also matches: render, rop, output, capture, screenshot.

Holds a render setup: which camera to shoot through, at what resolution, with which renderer and how much patience. It carries settings and nothing else -- no ports, no cook, no output. Pressing Render Still renders it and opens a dialog showing it arrive, where you review the frame and choose whether to save it.

It lives at the object level beside the cameras and lights it refers to, and it is how a shot stops being something you re-find by orbiting. Point it at a `camera` node and the same framing comes back every session; keep several around, one per shot, each named for what it captures.

Output size is either a named delivery size or your own two numbers, and an orientation turns a preset without retyping it. Up to 8192 pixels an edge. Anything larger than a browser draws in one pass is rendered in tiles and assembled, so the size you ask for is the size you get -- what changes with resolution is how long it takes, not whether it works.

The tabs are the decisions rather than the fields: Render is the shot, Quality is how long you are willing to wait and what the tracer is allowed to do while it waits, Denoise is what happens to the grain that is left, and Output is what leaves besides the picture. Everything under Quality and Denoise is path traced only; a rasterized still draws each pixel once.

Depth of field belongs to the camera, not here: aperture, focus distance and blade count live on the `camera` node this points at, so the same lens applies wherever that camera is used.

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `camera_path` | nodePath | `null` |  | Which camera to render through, picked by path from the cameras in the scene -- a reference, not a wire. Left unset the render comes from the current viewport view, wherever you last orbited it to, which is convenient but not repeatable. Point it at a camera to pin the shot down. |
| `resolution_preset` | enum (custom / hd / uhd_4k / uhd_8k / dci_2k / dci_4k / square / social_5x4 / a4_300 / a3_300 / letter_300) | `custom` |  | The output size, chosen by name. Custom is the default and reveals Width and Height for a size that is not on the list.

Choosing a preset sets the size rather than describing it. Width and Height together fix the aspect the camera frames at, so a preset changes the composition, not just the file size.

Every entry states its pixel size, and the print entries state the density that size assumes, so nothing here depends on a dots-per-inch setting the renderer would ignore. Sizes are listed wide edge first; Orientation turns them. |
| `orientation` | enum (landscape / portrait) | `landscape` |  | shown only while `resolution_preset` is not `custom`; Which way round the chosen size is: Landscape puts the wide edge across, Portrait puts it up.

It turns the size, and the camera frames to whatever aspect that gives it, so switching orientation reframes what is in view rather than only rotating the file.

The vertical delivery sizes come from here rather than from entries of their own, which is what keeps the list short: HD in Portrait is 1080 x 1920, the story and reel size, and A4 in Portrait is 2480 x 3508. |
| `width` | int | `1920` | 16 to 8192 | shown only while `resolution_preset` is `custom`; Output width in pixels. Together with Height it also fixes the aspect ratio the camera frames at, so changing it changes the composition, not just the file size.

Read only when Output Size is Custom, and hidden otherwise, since a preset states its own size in its name. |
| `height` | int | `1080` | 16 to 8192 | shown only while `resolution_preset` is `custom`; Output height in pixels. Large renders are drawn in tiles, each inside the four-megapixel budget a browser reliably survives, and assembled afterwards -- so the size you ask for is the size you get, however long it takes.

Read only when Output Size is Custom, and hidden otherwise. |
| `engine` | enum (raster / traced) | `raster` |  | Which renderer draws the still. Rasterized is the viewport's own renderer: fast, and its shadows, ambient occlusion and reflections are approximations. Path traced follows light through the scene instead, so shadows, bounced colour and soft reflections come out of the same calculation rather than being added on top -- and it takes as long as it takes.

A traced still shows what the tracer integrates: the environment where a ray leaves the scene, and no grid, gizmo or overlay. It is a photograph of the scene rather than a screenshot of the viewport. |
| `render` | action | `false` |  | Renders the still and opens a dialog showing it arrive, tile by tile, with a running count and a cancel. Nothing is written until you save from there.

The viewport is left where it is. The shot comes from the camera above, not from what you happen to be looking at, so pressing this never moves your view. |
| `quality` | enum (draft / good / high / reference / custom) | `good` |  | How many samples each pixel averages, and so how much grain is left. Four times the samples is half the noise, not a quarter, which is why the steps are wide: Draft to Good is a visible improvement and Draft to Reference is sixty-four times the wait.

Exact count reveals a Samples field for a scene that is not where the presets are. The wide steps are worth keeping for everything else: most shots want one of four answers, not a number to pick.

Path traced only. A rasterized still draws each pixel once. |
| `samples` | int | `64` | 1 to 8192 | shown only while `quality` is `custom`; The exact number of samples each pixel averages. Read only when Quality is Exact count, and hidden otherwise, since the named presets carry their own counts.

Reach for it when a preset is not where your scene is: a shot that is clean at ninety would otherwise mean either shipping the grain at sixty-four or waiting four times as long for two hundred and fifty-six.

Path traced only. |
| `bounces` | int | `6` | 1 to 32 | How many times light may scatter before a path is given up on. Higher opens up interiors and deep folds, where most of the light arrives after several bounces; an exterior on a bright day is usually finished by four.

Path traced only. |
| `transmissive_bounces` | int | `4` | 0 to 32 | How many of the bounces above may additionally pass through transmissive surfaces. Counted separately so a pane of glass does not spend a whole path's budget getting through it: a window is two surfaces, a tumbler is four, and running out ends the path rather than turning the glass opaque.

Path traced only. |
| `firefly_clamp` | float | `16.0` | 0 to 1000 | A ceiling on how much light one sample may contribute after it has bounced. A rare path that finds a bright source through a mirror or a tight caustic comes back hundreds of times the average, and a single one of those leaves a lone bright pixel that thousands of ordinary samples cannot average away.

What it costs is energy. Clamping discards the part above the ceiling rather than redistributing it, so the image gets darker exactly where the clamp acts, and a scene lit mostly through those rare paths gets darker overall. Lower it to suppress more of them, raise it to let brighter contributions through, and set it to zero to turn the clamp off and keep every last one.

Path traced only. |
| `seed` | int | `2654435769` | 0 to 4294967295 | What the sampling sequence is drawn from. The same seed gives the same image for the same scene, size and sample count on the same device, which is what makes a comparison between two settings a comparison rather than two different grain patterns.

The promise stops at the surface that rendered it. The browser and the command line accumulate in different chunk sizes, each for a reason sound on its own surface, and floating-point addition is not invariant to the grouping, so the same seed does not give the same bytes across them.

Changing it changes the grain and not the answer: two seeds at a high sample count converge to the same image.

Path traced only. |
| `denoise` | bool | `false` |  | Smooths the remaining grain, steered by what each pixel's surface looks like so material boundaries survive.

Off by default, and that is the right default for a finished still: at a high sample count there is little grain left to remove and a filter can only take detail away. Turn it on for a Draft, where the grain is the thing standing between you and seeing the shot.

This is the still's own setting. The viewport's traced preview keeps a separate one, in preferences, because a preview and a delivered frame want different answers. |
| `denoise_strength` | float | `1.0` | 0 to 4 | shown only while `denoise` is on; How hard the filter works, as a multiple of the colour tolerance it was measured at. One is that measured setting.

Below one the image keeps more grain and more detail, and material boundaries stay crisp. Above one it is smoother and softer, and fine texture starts to go with the noise. It steers the value that most changes the outcome rather than being a fifth independent number, so Colour Tolerance under Advanced remains the thing this multiplies. |
| `denoise_until_samples` | int | `0` | 0 to 8192 | shown only while `denoise` is on; The sample count past which the filter stops. Zero means it never stops.

A still that starts noisy and converges does not need at the end the filtering it needed at the start, and a converged image still being smoothed is losing detail for grain that is no longer there. Set this where your scene stops looking noisy and the filter steps out of the way after it.

The filter already relaxes on its own as a render converges, because its colour tolerance is divided by the square root of the sample count. This sharpens a behaviour that exists rather than introducing one. |
| `denoise_sigma_color` | float | `1.2` | 0.01 to 10 | under "Advanced"; shown only while `denoise` is on; How different in brightness two neighbouring pixels may be and still be averaged together. Larger reaches across those differences and removes more noise; smaller keeps detail and leaves more grain.

The default is measured rather than chosen: it came from an error sweep against a reference, scored both for error and for how much of a material step survived. Strength above multiplies this value.

Divided by the square root of the sample count inside the filter, so it tightens on its own as a render converges. |
| `denoise_normal_power` | float | `128.0` | 1 to 1024 | under "Advanced"; shown only while `denoise` is on; How closely two pixels' surface directions must agree before they are averaged together. The default corresponds to about ten degrees.

Higher keeps creases and curvature crisp and filters less across them; lower lets the filter reach around a curve, which is smoother and takes the edge off a bevel. This is the value that stops geometry melting. |
| `denoise_sigma_albedo` | float | `0.08` | 0.001 to 1 | under "Advanced"; shown only while `denoise` is on; How different two pixels' base colours may be before the filter treats them as different materials. Smaller keeps the boundary between two materials sharp; larger lets one bleed into the next.

Much tighter than the colour tolerance on purpose: base colour is noise-free where brightness is not, so it is the most reliable thing the filter has to steer by. |
| `denoise_level_falloff` | float | `2.0` | 1 to 8 | under "Advanced"; shown only while `denoise` is on; How much the tolerances tighten at each coarser pass. The filter runs at five scales and each one divides its tolerances by this number.

Higher makes the coarse passes conservative, keeping large-scale detail and removing less of the broad blotching; lower lets them reach further and flattens wide areas. |
| `transparent_background` | bool | `false` |  | Renders with nothing behind the subject. The environment still lights the scene exactly as it did, but it is not photographed into the frame, and what comes out carries a matte: opaque where the camera found a surface, clear where it found sky, and fractional along every silhouette.

That is what makes a render an element rather than a picture. The alternative is rendering against a colour and keying it by hand, which fails the moment the subject is glossy, because the background is in its reflections. |
| `aov_albedo` | bool | `false` |  | Writes the base colour each pixel saw as a file beside the image: surface colour before any lighting, which is what a compositor re-grades or relights against.

Producing a pass and displaying one are separate choices. This asks for the file; which pass the render window shows is chosen there, while it converges.

Path traced only. A rasterized still writes no auxiliary passes. |
| `aov_normal` | bool | `false` |  | Writes the surface direction each pixel saw as a file beside the image, encoded as colour. It is what a compositor relights with, and it is also what the denoiser steers by, so it is the pass to look at when a denoised result has lost an edge it should have kept.

Path traced only. |
| `aov_depth` | bool | `false` |  | Writes how far away each pixel is as a file beside the image. It is what depth of field, fog and atmospheric grading are built from in a compositor, and what tells you whether the shot has the depth separation you thought it had.

Path traced only. |

*Bypassed: cannot be bypassed.*

## Generate

### Brick <a id="brick"></a>

`brick` · v1 · Generate · placed inside a texture network · image source silhouette

Palette search also matches: brick, wall, masonry, bond.

A running-bond brick wall: Columns by Rows bricks separated by mortar, with alternate courses shifted by Row Offset.

A ready architectural pattern and a compact test of a texture chain. Feed it into `height_to_normal` for a raised-brick surface, into `levels` for a wear mask, or `mix` it with `noise` to break up the flat colors.

Colors are written without conversion, alpha included. The layout is in normalized coordinates, so a non-square image stretches the bricks; Mortar Width is a fraction of a cell, applied on every side, and Row Offset drives the bond from stacked to running.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `image` | out | Image | The resulting image: RGBA8, and never more than 2048 px on the long edge, because that is the resolution the texture context cooks at. Being the default output, a drag from the node's body wires from here. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `width` | int | `512` | 16 to 2048 | Pixels across the generated image's width. Independent of the other axis, so a non-square image is fine, but the pattern is laid out in normalized coordinates and stretches to fit rather than staying square. The ceiling is 2048, the working resolution the texture context cooks at; these are single-threaded CPU loops, so doubling both dimensions quadruples the pixels a cook touches. |
| `height` | int | `512` | 16 to 2048 | Pixels across the generated image's height. Independent of the other axis, so a non-square image is fine, but the pattern is laid out in normalized coordinates and stretches to fit rather than staying square. The ceiling is 2048, the working resolution the texture context cooks at; these are single-threaded CPU loops, so doubling both dimensions quadruples the pixels a cook touches. |
| `brick_color` | color | `[0.550000011920929,0.25,0.18000000715255737,1.0]` |  | The color of the bricks themselves. Written straight into the texels with no conversion, alpha included, so it lands exactly as the picker shows it. |
| `mortar_color` | color | `[0.8500000238418579,0.8500000238418579,0.8199999928474426,1.0]` |  | The color of the mortar lines between the bricks. Written straight into the texels with no conversion, alpha included. |
| `columns` | int | `6` | 1 to 64 | How many bricks span the image horizontally. Independent of Rows, so tall or squat bricks are a matter of the two counts. |
| `rows` | int | `12` | 1 to 64 | How many brick courses span the image vertically. Independent of Columns; alternate courses shift by Row Offset for the running bond. |
| `mortar` | float | `0.06` | 0 to 0.5 | The mortar thickness as a fraction of a cell (0 is no mortar, 0.5 leaves no brick). Applied on all four sides of each brick, so the visible brick shrinks as this grows. |
| `row_offset` | float | `0.5` | 0 to 1 | How far alternate courses shift sideways, as a fraction of a brick: 0.5 is the classic running bond, 0 stacks the bricks in straight columns. |

*Bypassed: emits nothing.*

### Checker <a id="checker"></a>

`checker` · v1 · Generate · placed inside a texture network · image source silhouette

Palette search also matches: checker, checkerboard, grid, uv.

A two-color checkerboard of Tiles X by Tiles Y alternating cells.

The classic UV and scale reference: drop it on a model to read texel density and seams at a glance, or use it as a hard-edged mask and tint source. Two checkers at different tile counts through a `mix` gives a quick plaid.

Colors are written without conversion, alpha included, so what the picker shows is what the texels hold. Tiles map to a normalized grid, so a non-square image or an uneven tile count yields rectangular cells rather than square ones.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `image` | out | Image | The resulting image: RGBA8, and never more than 2048 px on the long edge, because that is the resolution the texture context cooks at. Being the default output, a drag from the node's body wires from here. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `width` | int | `512` | 16 to 2048 | Pixels across the generated image's width. Independent of the other axis, so a non-square image is fine, but the pattern is laid out in normalized coordinates and stretches to fit rather than staying square. The ceiling is 2048, the working resolution the texture context cooks at; these are single-threaded CPU loops, so doubling both dimensions quadruples the pixels a cook touches. |
| `height` | int | `512` | 16 to 2048 | Pixels across the generated image's height. Independent of the other axis, so a non-square image is fine, but the pattern is laid out in normalized coordinates and stretches to fit rather than staying square. The ceiling is 2048, the working resolution the texture context cooks at; these are single-threaded CPU loops, so doubling both dimensions quadruples the pixels a cook touches. |
| `color_a` | color | `[0.10000000149011612,0.10000000149011612,0.10000000149011612,1.0]` |  | The color of the even tiles (the top-left one). Written straight into the texels with no conversion, alpha included, so it lands exactly as the picker shows it. |
| `color_b` | color | `[0.8999999761581421,0.8999999761581421,0.8999999761581421,1.0]` |  | The color of the odd tiles, alternating with Color A across the board. Written straight into the texels with no conversion, alpha included. |
| `tiles_x` | int | `8` | 1 to 256 | How many tiles span the image horizontally. Independent of Tiles Y, so an uneven count gives rectangular tiles, and the grid maps to normalized coordinates rather than staying square. |
| `tiles_y` | int | `8` | 1 to 256 | How many tiles span the image vertically. Independent of Tiles X, so an uneven count gives rectangular tiles. |

*Bypassed: emits nothing.*

### Constant <a id="constant"></a>

`constant` · v1 · Generate · placed inside a texture network · image source silhouette

Palette search also matches: constant, solid, fill, color.

A solid image: every texel is the Color param, at the given size.

The workhorse fill of a texture network. Reach for it as the base layer under a `mix`, as a flat map into `pack_orm`, or as a solid tint to multiply an imported image against. Two constants and a `mix` is the shortest path to a mask.

The color is not converted on the way in, so what the picker shows is what the texels hold. Alpha survives every adjust node untouched (`blur` is the one exception, it filters alpha too), but only `mix` in Over mode ever reads it.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `image` | out | Image | The resulting image: RGBA8, and never more than 2048 px on the long edge, because that is the resolution the texture context cooks at. Being the default output, a drag from the node's body wires from here. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `width` | int | `512` | 16 to 2048 | Pixels across the generated image's width. Independent of the other axis, so a non-square image is fine, but the pattern is laid out in normalized coordinates and stretches to fit rather than staying square. The ceiling is 2048, the working resolution the texture context cooks at; these are single-threaded CPU loops, so doubling both dimensions quadruples the pixels a cook touches. |
| `height` | int | `512` | 16 to 2048 | Pixels across the generated image's height. Independent of the other axis, so a non-square image is fine, but the pattern is laid out in normalized coordinates and stretches to fit rather than staying square. The ceiling is 2048, the working resolution the texture context cooks at; these are single-threaded CPU loops, so doubling both dimensions quadruples the pixels a cook touches. |
| `color` | color | `[0.5,0.5,0.5,1.0]` |  | The fill, RGBA. It is written straight into the stored 8-bit texels with no color conversion, so it lands exactly as the picker shows it. The alpha lane is real: 1 is opaque, and lowering it only changes what `mix` in Over mode does with the image. |

*Bypassed: emits nothing.*

### Gradient <a id="gradient"></a>

`gradient` · v1 · Generate · placed inside a texture network · image source silhouette

Palette search also matches: gradient, conic, radial, linear.

A two-color gradient with a movable centre and four falloff shapes: linear, radial, angular (conic), or diamond.

The richer sibling of `ramp`: reach for it when you need a conic or diamond falloff or an off-centre origin that `ramp`'s fixed horizontal / vertical / radial cannot give. Feed it into `mix` as a base or blend layer, or into `levels` to shape the falloff.

All four channels interpolate, alpha included. Distances are measured in normalized coordinates, so the rings and diamonds become ellipses and rhombi on a non-square image, and the factor is clamped so the corners past the far color sit flat.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `image` | out | Image | The resulting image: RGBA8, and never more than 2048 px on the long edge, because that is the resolution the texture context cooks at. Being the default output, a drag from the node's body wires from here. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `width` | int | `512` | 16 to 2048 | Pixels across the generated image's width. Independent of the other axis, so a non-square image is fine, but the pattern is laid out in normalized coordinates and stretches to fit rather than staying square. The ceiling is 2048, the working resolution the texture context cooks at; these are single-threaded CPU loops, so doubling both dimensions quadruples the pixels a cook touches. |
| `height` | int | `512` | 16 to 2048 | Pixels across the generated image's height. Independent of the other axis, so a non-square image is fine, but the pattern is laid out in normalized coordinates and stretches to fit rather than staying square. The ceiling is 2048, the working resolution the texture context cooks at; these are single-threaded CPU loops, so doubling both dimensions quadruples the pixels a cook touches. |
| `mode` | enum (linear / radial / angular / diamond) | `linear` |  | How the blend factor is measured around the Centre: Linear runs left to right through it, Radial spreads outward from it, Angular sweeps the angle around it (a conic gradient), and Diamond is a rotated square. All four honour Centre. |
| `color_a` | color | `[0.0,0.0,0.0,1.0]` |  | The color at the start of the gradient: the left edge, the centre, or the start of the angular sweep depending on Mode. All four channels interpolate, so an alpha set here ramps as well. |
| `color_b` | color | `[1.0,1.0,1.0,1.0]` |  | The color at the end of the gradient: the right edge, the outer limit of the falloff, or the end of the angular sweep. Swap it with From to reverse the gradient. |
| `center` | vec2 | `[0.5,0.5]` | 0 to 1 | Where the gradient originates, in normalized 0..1 coordinates (0.5, 0.5 is the image centre). Radial, Angular, and Diamond spread from here; Linear uses only its x as the midpoint. |

*Bypassed: emits nothing.*

### Noise <a id="noise"></a>

`noise` · v1 · Generate · placed inside a texture network · image source silhouette

Palette search also matches: noise, random, value noise.

Value noise: a lattice of hashed values, smoothstep-interpolated into a grayscale image. Deterministic, so the same seed and size cook the same pixels in every session and on every machine.

The base of most procedural texture work. Run it through `levels` or `brightness_contrast` to shape its range, `blur` to soften it, or feed it straight to `height_to_normal` for a bumpy surface. Two noise nodes at different scales through a `mix` in Multiply is the cheap way to get detail at two frequencies.

The output is opaque gray: R, G and B carry the same value and alpha is always 1, so this is a scalar field rather than a color. There is no octave or fractal control, just one lattice at one frequency, and nothing wraps that lattice at the image edge, so the result does not tile.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `image` | out | Image | The resulting image: RGBA8, and never more than 2048 px on the long edge, because that is the resolution the texture context cooks at. Being the default output, a drag from the node's body wires from here. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `width` | int | `512` | 16 to 2048 | Pixels across the generated image's width. Independent of the other axis, so a non-square image is fine, but the pattern is laid out in normalized coordinates and stretches to fit rather than staying square. The ceiling is 2048, the working resolution the texture context cooks at; these are single-threaded CPU loops, so doubling both dimensions quadruples the pixels a cook touches. |
| `height` | int | `512` | 16 to 2048 | Pixels across the generated image's height. Independent of the other axis, so a non-square image is fine, but the pattern is laid out in normalized coordinates and stretches to fit rather than staying square. The ceiling is 2048, the working resolution the texture context cooks at; these are single-threaded CPU loops, so doubling both dimensions quadruples the pixels a cook touches. |
| `scale` | float | `8.0` | 1 to 64 | How many noise cells span the image: 8 lays an 8x8 lattice whatever the pixel size. Raise it for finer grain, lower it for broad blobs. The count is the same on both axes, so a non-square image gets stretched cells. |
| `seed` | int | `0` | 0 to 9999 | Selects the hash lattice. Any change gives a completely different image rather than a shifted one, so scrub it to hunt for a pattern you like. The same seed always cooks the same pixels, which is what lets a saved scene reproduce exactly. |

*Bypassed: emits nothing.*

### Ramp <a id="ramp"></a>

`ramp` · v1 · Generate · placed inside a texture network · image source silhouette

Palette search also matches: ramp, gradient.

A two-color gradient across the image: `From` at one end, `To` at the other, linearly interpolated, horizontal, vertical, or radial.

The standard mask and gradient source. Feed it into `mix` as the base or the blend layer, or into `levels` to shape the falloff, which is the usual way to give a linear ramp a knee.

All four channels interpolate, alpha included. Radial measures distance from the image centre in normalized coordinates and doubles it, so it hits `To` at the four edge midpoints and the corners are clamped flat; that same normalization makes the rings ellipses rather than circles on a non-square image.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `image` | out | Image | The resulting image: RGBA8, and never more than 2048 px on the long edge, because that is the resolution the texture context cooks at. Being the default output, a drag from the node's body wires from here. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `width` | int | `512` | 16 to 2048 | Pixels across the generated image's width. Independent of the other axis, so a non-square image is fine, but the pattern is laid out in normalized coordinates and stretches to fit rather than staying square. The ceiling is 2048, the working resolution the texture context cooks at; these are single-threaded CPU loops, so doubling both dimensions quadruples the pixels a cook touches. |
| `height` | int | `512` | 16 to 2048 | Pixels across the generated image's height. Independent of the other axis, so a non-square image is fine, but the pattern is laid out in normalized coordinates and stretches to fit rather than staying square. The ceiling is 2048, the working resolution the texture context cooks at; these are single-threaded CPU loops, so doubling both dimensions quadruples the pixels a cook touches. |
| `direction` | enum (horizontal / vertical / radial) | `horizontal` |  | How the blend factor is measured across the image: Horizontal runs left to right, Vertical top to bottom, Radial outward from the centre. Radial reaches `To` at the four edge midpoints rather than at the corners, so the corners sit clamped at flat `To`. |
| `color_a` | color | `[0.0,0.0,0.0,1.0]` |  | The color at the start of the gradient: the left edge, the top edge, or the image centre, depending on Direction. All four channels interpolate, so an alpha set here ramps as well. |
| `color_b` | color | `[1.0,1.0,1.0,1.0]` |  | The color at the end of the gradient: the right edge, the bottom edge, or the outer limit of the radial falloff. Swap it with `From` to reverse the gradient. |

*Bypassed: emits nothing.*

### Voronoi <a id="voronoi"></a>

`voronoi` · v1 · Generate · placed inside a texture network · image source silhouette

Palette search also matches: voronoi, worley, cellular, cells.

Worley / Voronoi cellular noise: one jittered feature point per lattice cell, read back per texel as a distance falloff, a per-cell value, or the cell edges.

A staple of stone, scale, crackle, and organic-cell texturing. Feed the Distance pattern into `levels` for cracked-earth or reptile masks, the Cell ID pattern into `mix` for a random per-cell tint, or the Edges pattern as a wear or grout mask.

The output is opaque grey: R, G and B carry the same value and alpha is 1, so this is a scalar field rather than a color. It is deterministic in the seed and, like `noise`, does not tile at the image edge.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `image` | out | Image | The resulting image: RGBA8, and never more than 2048 px on the long edge, because that is the resolution the texture context cooks at. Being the default output, a drag from the node's body wires from here. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `width` | int | `512` | 16 to 2048 | Pixels across the generated image's width. Independent of the other axis, so a non-square image is fine, but the pattern is laid out in normalized coordinates and stretches to fit rather than staying square. The ceiling is 2048, the working resolution the texture context cooks at; these are single-threaded CPU loops, so doubling both dimensions quadruples the pixels a cook touches. |
| `height` | int | `512` | 16 to 2048 | Pixels across the generated image's height. Independent of the other axis, so a non-square image is fine, but the pattern is laid out in normalized coordinates and stretches to fit rather than staying square. The ceiling is 2048, the working resolution the texture context cooks at; these are single-threaded CPU loops, so doubling both dimensions quadruples the pixels a cook touches. |
| `scale` | float | `8.0` | 0.5 to 64 | How many cells span the image: 8 scatters an 8x8 lattice of feature points whatever the pixel size. Raise it for smaller, denser cells, lower it for a few big ones. The count is the same on both axes, so a non-square image gets stretched cells. |
| `seed` | int | `0` | 0 to 9999 | Selects where the feature points land inside their cells. Any change rescatters them into a new pattern rather than shifting the old one, and the same seed always cooks the same pixels, so a saved scene reproduces exactly. |
| `jitter` | float | `1.0` | 0 to 1 | How far each feature point strays from its cell centre: 0 pins every point to the centre for a regular grid, 1 lets it fall anywhere in the cell for a fully irregular pattern. |
| `metric` | enum (euclidean / manhattan / chebyshev) | `euclidean` |  | The distance measure that decides which feature owns a texel: Euclidean gives round cells, Manhattan diamond-shaped ones, and Chebyshev square ones. It reshapes both the cells and the Distance falloff. |
| `pattern` | enum (distance / cell_id / edges) | `distance` |  | What each texel stores: Distance is the nearest-feature distance (a cellular falloff, dark at the centres), Cell ID is a flat hashed grey per cell (a random mask), and Edges draws bright lines along the cell boundaries. |

*Bypassed: emits nothing.*

## Generators

### Box <a id="box"></a>

`box` · v2 · Generators · placed inside a geo

Palette search also matches: cube, rectangle, cuboid.

A rectangular box centred on the origin, sized per axis and optionally divided into a grid of quads on each face.

This is the usual starting point for a blockout: box out the massing first, then reach for `transform` to place it and `merge` to combine it with others. It generates flat-shaded geometry with hard edges, because each face carries its own corner points -- 24 points for the default 12 triangles, not 8 shared ones.

The segment counts only matter to something downstream that needs the extra points. A plain box needs none, so they default to 1.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `geometry` | out | Geometry | The cooked geometry. Being the default output, a drag from the node's body wires from here, and a bypass passes the input through it. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `width` | float | `1.0` | 0.001 to 10000 | meters; Size along X, in metres. The box is centred on the origin, so this extends ±0.5x either side rather than growing in one direction. |
| `height` | float | `1.0` | 0.001 to 10000 | meters; Size along Y, in metres. The box is centred on the origin, so this extends ±0.5x either side rather than growing in one direction. |
| `depth` | float | `1.0` | 0.001 to 10000 | meters; Size along Z, in metres. The box is centred on the origin, so this extends ±0.5x either side rather than growing in one direction. |
| `width_segments` | int | `1` | 1 to 512 | How many divisions the X faces are cut into. 1 leaves a flat quad. Raise it only when something downstream needs the extra points to work with -- a deform, a noise displacement, a subdivide -- because every segment multiplies the point count. |
| `height_segments` | int | `1` | 1 to 512 | How many divisions the Y faces are cut into. 1 leaves a flat quad. Raise it only when something downstream needs the extra points to work with -- a deform, a noise displacement, a subdivide -- because every segment multiplies the point count. |
| `depth_segments` | int | `1` | 1 to 512 | How many divisions the Z faces are cut into. 1 leaves a flat quad. Raise it only when something downstream needs the extra points to work with -- a deform, a noise displacement, a subdivide -- because every segment multiplies the point count. |

*Bypassed: emits nothing.*

### Circle <a id="circle"></a>

`circle` · v1 · Generators · placed inside a geo

Palette search also matches: ring, loop, profile, disc, curve.

A closed loop of straight segments at Radius around the chosen axis, centred on the origin. Like `line` it is a curve: no surface, no normals, drawn as an unlit one-pixel wire.

It is the standard profile shape: the upcoming extrude family consumes closed loops like this one, and until then it serves as a guide, a path for copies, or a scatter-free ring of points via `points_from_geo`.

The default Y axis lays the loop flat in the ground plane, winding counter-clockwise seen from above. Segments trades smoothness against point count: each segment is one straight piece and one carrier point.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `geometry` | out | Geometry | The cooked geometry. Being the default output, a drag from the node's body wires from here, and a bypass passes the input through it. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `radius` | float | `0.5` | 0.001 to 10000 | meters; The loop's radius, in metres, centred on the origin. |
| `segments` | int | `32` | 3 to 512 | How many straight segments approximate the circle. 3 is a triangle, 32 reads as smooth at typical sizes; raise it only when the circle is large on screen or feeds a downstream operation that needs the density. |
| `axis` | enum (x / y / z) | `y` |  | The axis the circle rings around: the loop lies in the plane perpendicular to it. The default Y lays it flat in the ground plane. |

*Bypassed: emits nothing.*

### Cone <a id="cone"></a>

`cone` · v2 · Generators · placed inside a geo

Palette search also matches: pyramid, spike.

A cone centred on the origin, with its apex at +height/2 and a flat base cap at -height/2. The sloping side is smooth-shaded, and there is no top cap.

Reach for it for spikes, roofs, and trees in a blockout, then `transform` to place it and `merge` to combine it. At low `radial_segments` it is also the pyramid primitive: 4 gives a square pyramid, 3 a tetrahedron.

This is `cylinder` with the top radius pinned to 0, sharing one generator so the tip is handled in exactly one place. That means the apex is not a single welded point: the tip row keeps one coincident vertex per column so each column holds its own UV, and the degenerate half of each tip quad is skipped. The default is 100 points for 64 triangles, against the cylinder's 134 for 128.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `geometry` | out | Geometry | The cooked geometry. Being the default output, a drag from the node's body wires from here, and a bypass passes the input through it. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `radius` | float | `0.5` | 0.001 to 10000 | meters; Radius of the base ring, at -height/2, in metres. The apex sits directly above the centre of that ring on the Y axis, so this sets how wide the cone flares rather than where it points. |
| `height` | float | `1.0` | 0.001 to 10000 | meters; Distance from base to apex along Y, in metres. The cone is centred on the origin, so raising it moves both ends apart: the apex to +height/2 and the base to -height/2, not the apex alone. |
| `radial_segments` | int | `32` | 3 to 512 | Facets around the base circumference. At 32 it reads as a smooth cone; drop it to 4 for a square pyramid or to the minimum of 3 for a tetrahedron, which is what the `pyramid` alias is about. |
| `height_segments` | int | `1` | 1 to 512 | How many rows the sloping side is cut into between apex and base. The silhouette is unchanged, because the side is straight either way, so raise it only for a downstream deform that needs the extra points to work with. |

*Bypassed: emits nothing.*

### Cylinder <a id="cylinder"></a>

`cylinder` · v2 · Generators · placed inside a geo

Palette search also matches: tube, pipe.

A cylinder running along Y and centred on the origin, with a flat cap at each end and a smooth-shaded torso. The two radii are independent, so the same node covers tapered tubes and truncated cones.

Reach for it for pipes, pillars, and pegs in a blockout, then `transform` to place it and `merge` to combine it with others. For a plain cone prefer `cone`, which is this same generator with the top radius pinned to 0 and one less param to set.

Either radius may be 0, which collapses that ring to a point and omits its cap. Torso normals lean by the slope between the two radii, so a collapsed tip still shades correctly with no special case. The caps never share vertices with the torso, because their normals differ, so the rim is a hard edge and the default comes to 134 points for 128 triangles.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `geometry` | out | Geometry | The cooked geometry. Being the default output, a drag from the node's body wires from here, and a bypass passes the input through it. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `radius_top` | float | `0.5` | 0 to 10000 | meters; Radius of the top ring, at +height/2, in metres. Set it to 0 and the ring collapses to a point and its cap disappears, turning the cylinder into a cone; set it anywhere between 0 and `radius_bottom` for a truncated cone. |
| `radius_bottom` | float | `0.5` | 0 to 10000 | meters; Radius of the bottom ring, at -height/2, in metres. 0 collapses it to a point and drops the bottom cap, the same as `radius_top` does at the other end. Both radii at 0 collapses the whole surface onto the Y axis and leaves nothing to see. |
| `height` | float | `1.0` | 0.001 to 10000 | meters; Length along Y, in metres. The cylinder is centred on the origin, so this extends half either side and the caps sit at +/-height/2, rather than growing up from the base. |
| `radial_segments` | int | `32` | 3 to 512 | Facets around the circumference. This is what makes the tube read as round: 32 is smooth at ordinary sizes, and the minimum of 3 gives a triangular tube. It prices both caps as well as the torso. |
| `height_segments` | int | `1` | 1 to 512 | How many rows the torso is cut into between the caps. It never changes the silhouette, because the torso is straight-sided either way. Raise it only when something downstream needs the extra points -- a bend, a noise displacement -- which is why it defaults to 1. |

*Bypassed: emits nothing.*

### Line <a id="line"></a>

`line` · v1 · Generators · placed inside a geo

Palette search also matches: curve, segment, polyline, wire, path.

A straight polyline from Start to End, subdivided into evenly spaced points. It is the first curve primitive: line topology has no surface, so it draws as an unlit wire at a constant one-pixel width, unaffected by lights and materials.

At the default 2 points it is a single segment. More points give downstream nodes something to grab: a deform has interior points to move, and `copy_to_points` can stamp a template along the line's vertices.

The default runs from the origin one metre up the Y axis. Wires and edges are unpickable in the viewport; select the node on the canvas.

The two optional inputs anchor the ends to existing geometry: a connected input overrides its parameter with point 0 of the geometry's first non-empty mesh, so a single-point `scatter` or `points_from_geo` upstream pins that end and the line follows it on every recook.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `start_point` | in | Geometry | Optional. When connected, the starting endpoint snaps to point 0 of this geometry's first non-empty mesh, overriding the parameter. |
| `end_point` | in | Geometry | Optional. When connected, the finishing endpoint snaps to point 0 of this geometry's first non-empty mesh, overriding the parameter. |
| `geometry` | out | Geometry | The cooked geometry. Being the default output, a drag from the node's body wires from here, and a bypass passes the input through it. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `start` | vec3 | `[0.0,0.0,0.0]` |  | meters; overridden when `start_point` is connected; The starting end of the line, in metres. Both ends are free points; nothing pins the line to the origin. A geometry wired into the matching input overrides this with its first point. |
| `end` | vec3 | `[0.0,1.0,0.0]` |  | meters; overridden when `end_point` is connected; The finishing end of the line, in metres. Both ends are free points; nothing pins the line to the origin. A geometry wired into the matching input overrides this with its first point. |
| `points` | int | `2` | 2 to 1025 | How many evenly spaced points the line carries, endpoints included. 2 is a single segment; raise it when a deform or scatter downstream needs interior points to work with. |

*Bypassed: emits nothing.*

### Plane <a id="plane"></a>

`plane` · v2 · Generators · placed inside a geo

Palette search also matches: quad, grid, ground.

A flat rectangle in the XY plane facing +Z, centred on the origin and optionally cut into a grid of quads. It is a single sheet: every normal is +Z, and there is no thickness and no back face.

It is the usual base for anything displaced -- raise the segment counts and feed it to a deform -- and it doubles as a ground plane or a backdrop once `transform` has placed it.

It stands upright, it does not lie flat. The XY/+Z orientation is the spec the other primitives share, so using it as ground means rotating it -90 degrees about X first, to face +Y. It is the cheapest primitive here: 4 points and 2 triangles at the default 1 x 1 segments.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `geometry` | out | Geometry | The cooked geometry. Being the default output, a drag from the node's body wires from here, and a bypass passes the input through it. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `width` | float | `1.0` | 0.001 to 10000 | meters; Size along X, in metres. The plane is centred on the origin, so this extends ±0.5x either side rather than growing in one direction. |
| `height` | float | `1.0` | 0.001 to 10000 | meters; Size along Y, in metres. The plane is centred on the origin, so this extends ±0.5x either side rather than growing in one direction. |
| `width_segments` | int | `1` | 1 to 1024 | How many divisions the plane is cut into along X. 1 leaves a single flat quad. Unlike the segment counts on `box`, raising this is routine: a plane is usually the input to a displacement or a deform, and those have nothing to move without points. |
| `height_segments` | int | `1` | 1 to 1024 | How many divisions the plane is cut into along Y. 1 leaves a single flat quad. Unlike the segment counts on `box`, raising this is routine: a plane is usually the input to a displacement or a deform, and those have nothing to move without points. |

*Bypassed: emits nothing.*

### Sphere <a id="sphere"></a>

`sphere` · v2 · Generators · placed inside a geo

Palette search also matches: ball, globe.

A UV sphere centred on the origin, built as a latitude/longitude grid with its poles on the Y axis. Its normals are exact -- each one is just the normalized position -- so it shades smooth rather than faceted.

Reach for it for blockout massing and as a general test object: `transform` places it, `merge` combines it with others. Raise the segment counts for a rounder silhouette, drop them for a low-poly look; both change the shape, unlike the segment counts on `box`.

Point count is (width + 1) x (height + 1), which at the default 32 x 16 is 561 points and 960 triangles. The extra column is the UV seam, repeating the first column's positions at u = 1 instead of u = 0, and each pole row holds one coincident vertex per column so every column keeps its own UV. At each pole the degenerate half of every quad is skipped, leaving a triangle fan.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `geometry` | out | Geometry | The cooked geometry. Being the default output, a drag from the node's body wires from here, and a bypass passes the input through it. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `radius` | float | `0.5` | 0.001 to 10000 | meters; Distance from the centre to the surface, in metres. The sphere is centred on the origin, so the default 0.5 is a 1 m ball and the poles land at (0, +/-radius, 0). |
| `width_segments` | int | `32` | 3 to 512 | Columns of longitude around the Y axis, so this is how round the sphere reads when you look down at it. The minimum is 3, which leaves a three-sided husk. Each column adds a point to every latitude row, so this is the more expensive of the two counts. |
| `height_segments` | int | `16` | 2 to 512 | Rows of latitude from pole to pole, so this is how round the profile reads from the side. The minimum is 2, which gives a bipyramid: two cones meeting at the equator. The default 16 against 32 columns keeps the quads roughly square, this axis spanning half a turn against the other's full one. |

*Bypassed: emits nothing.*

### Torus <a id="torus"></a>

`torus` · v2 · Generators · placed inside a geo

Palette search also matches: donut, ring.

A torus, a donut, centred on the origin and swept around the Z axis so it lies in the XY plane. Normals point radially out of the tube and are exact, so it shades smooth.

Reach for it for rings, tyres, and handles, and as a test object: it curves in two directions and carries a clean UV grid, which makes it an honest check of normals, texture mapping, or a displacement before you commit to real geometry.

The two segment counts are named the opposite way round from most people's first guess. `radial_segments` subdivides the tube's cross-section, how round the tube is; `tubular_segments` subdivides the sweep, how round the ring is. The defaults, 16 and 32, give 561 points and 1024 triangles. Like `plane` it stands upright: the hole faces along Z, not Y.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `geometry` | out | Geometry | The cooked geometry. Being the default output, a drag from the node's body wires from here, and a bypass passes the input through it. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `radius` | float | `0.5` | 0.001 to 10000 | meters; Distance from the centre of the torus out to the centre of the tube, in metres. It is the ring's radius, not the outer edge: the silhouette reaches radius + tube, so the default 0.5 against a 0.2 tube measures 0.7 from the origin. |
| `tube` | float | `0.2` | 0.001 to 10000 | meters; Radius of the tube's cross-section, in metres, measured out from the ring. It eats the hole from both sides: the hole's radius is radius - tube, so at tube = radius the hole shuts completely and above that the surface passes through itself. |
| `radial_segments` | int | `16` | 3 to 1024 | Facets around the tube's cross-section, so this is how round the tube itself is. The minimum of 3 gives a tube of triangular section. This is the cross-section, not the sweep: the sweep is `tubular_segments`. |
| `tubular_segments` | int | `32` | 3 to 1024 | Facets around the sweep, so this is how round the ring is. The minimum of 3 bends the tube into a triangle. It usually wants to be the higher of the two counts -- the defaults are 32 against 16 -- because the sweep covers the longer distance. |

*Bypassed: emits nothing.*

### Torus Knot <a id="torus_knot"></a>

`torus_knot` · v2 · Generators · placed inside a geo

Palette search also matches: knot, pretzel.

A tube swept along a (p, q) torus knot: a closed curve that winds p times around the Z axis while winding q times through the ring's hole. The default (2, 3) is the trefoil.

It is mostly a showpiece and a test object. Its curve turns through every orientation, which makes it the honest check for a shader, a normal map, or a deform -- the kind of thing a box or a sphere would let pass unnoticed.

It is far and away the heaviest primitive here. The defaults, 128 tubular by 32 radial segments, come to 4257 points and 8192 triangles, about eight times the sphere, and both counts reach 2048. They multiply, so trim `radial_segments` before `tubular_segments`: the sweep needs its samples to keep the curve smooth, the cross-section rarely needs all 32.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `geometry` | out | Geometry | The cooked geometry. Being the default output, a drag from the node's body wires from here, and a bypass passes the input through it. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `radius` | float | `0.5` | 0.001 to 10000 | meters; Scales the whole knot curve, in metres. It is not an outer radius: the curve's distance from the Z axis rides between 0.5x and 1.5x this value as it winds, so the silhouette reaches about 1.5 x radius, plus `tube`. |
| `tube` | float | `0.2` | 0.001 to 10000 | meters; Radius of the swept tube's cross-section, in metres. Nothing checks the knot for self-clearance, so once this grows large against `radius` neighbouring passes of the curve simply intersect each other. |
| `p` | int | `2` | 1 to 10 | How many times the curve winds around the Z axis before it closes. With `q` it picks the knot: (2, 3) is the trefoil, (2, 5) the cinquefoil. Keep it coprime with `q` -- a shared factor makes the curve retrace its own path instead of tying a new knot. |
| `q` | int | `3` | 1 to 10 | How many times the curve winds through the ring's hole before it closes. Raising it against a fixed `p` adds lobes. As with `p`, a factor shared between the two retraces the curve: it is covered gcd(p, q) times, laying that many coincident copies of the tube on the same path. |
| `tubular_segments` | int | `128` | 3 to 2048 | Samples taken along the knot curve. This is what keeps the curve itself smooth, and it needs to be generous because the curve is long and twists constantly: hence 128 by default, against 32 for the cross-section. |
| `radial_segments` | int | `32` | 3 to 2048 | Facets around the tube's cross-section, the same meaning it has on `torus`: it makes the tube round, not the curve. When the mesh is too heavy this is usually the cheaper of the two to cut. |

*Bypassed: emits nothing.*

## Import

### Import Image <a id="import_image"></a>

`import_image` · v1 · Import · placed inside a geo or inside a texture network · image source silhouette

Palette search also matches: image, texture, png, jpeg, webp, import.

Decodes a PNG, JPEG, or WebP file into an Image value that can drive a material's map ports or feed a texture network.

It is the source end of both image workflows: wire it into a `material` node's base colour, roughness, or normal port to texture a surface, or drop it in a texture network as the input an image operator chain works over. It is one of the few nodes placeable in both geometry and texture networks, for exactly that reason.

With no file staged the node emits no value at all, not a placeholder pixel: a map port wired to it reads as unconnected, so an empty import never silently drives a channel to black. On the web the decode happens off the main thread in the import worker via `createImageBitmap`; a failed decode badges the node and the previous image stays live.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `image` | out | Image | The decoded RGBA image. Empty until a file is staged, which a material map port reads as 'no map' rather than as a blank texture. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `file` | assetRef | `` |  | The staged image file: PNG, JPEG, or WebP. Identity is the bytes' SHA-256, not the path, so staging the same file twice costs nothing and a saved `.slxy` embeds a copy -- the scene still opens once the original is gone. Only this reference is stored in the document; the decoded pixels are a cook artifact, rebuilt on load. |

*Bypassed: emits nothing.*

### Import OBJ <a id="import_obj"></a>

`import_obj` · v4 · Import · placed inside a geo

Palette search also matches: obj, wavefront, import, colors.

Loads a Wavefront OBJ, triangulating as it parses. Materials come from the companion MTL and its textures, resolved by file name against the staged assets rather than the file system.

This heads a chain: import, then `transform` to place the model, `merge` to combine it with others, `bounds` to check where it actually landed. It is also the validator's entry point -- an import validates the raw file as it parses, the same check the desktop viewer runs on load, so both products report the same issues on the same file.

OBJ is a multi-file format and the MTL is not optional-by-accident: stage the `.mtl` and its textures together with the `.obj` (the picker is multi-select, and dropping the containing folder traverses it) or the model arrives with geometry and no materials, no error raised. On the web the parse runs in an import worker off the main thread, so a heavy file does not freeze the canvas.

Per-vertex colours ride the unofficial extended-position form (`v x y z r g b`), which scanners and MeshLab both write. They survive as the per-point colour attribute and display directly; the Vertex Colors toggle drops them at the door. Colours are read as sRGB, matching PLY, so the same scan exported to either format imports the same.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `geometry` | out | Geometry | The cooked geometry. Being the default output, a drag from the node's body wires from here, and a bypass passes the input through it. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `file` | assetRef | `` |  | The staged model file. Identity is the bytes' SHA-256, not the path, so re-staging the same file costs nothing and a saved `.slxy` carries a copy of the bytes -- the scene keeps loading after the original moves or is deleted. The picker is multi-select: stage companion files (an MTL, a `.bin`, textures) in the same go and the parser resolves them by name. Left empty the node cooks to nothing, without an error. |
| `scale` | float | `1.0` | 0 to 10000 | A uniform multiplier baked into the points at import, about the origin. Use it to reconcile units at the source -- a millimetre CAD export needs 0.001 to land in metres. Unlike a downstream `transform`, this is baked, so everything after it measures the scaled model. |
| `center_to_origin` | bool | `false` |  | Moves the model so its bounding-box centre sits at the origin. Applied after Scale. Worth turning on for a file authored far from the origin, which otherwise imports off-screen and orbits around nothing. |
| `preserve_materials` | bool | `true` |  | On keeps the materials the file defines. Off drops every material binding and the material table with them, so the whole model draws in the renderer's neutral default -- the clay look you want when judging form, or when a file's own materials are fighting you. |
| `vertex_colors` | bool | `true` |  | Keep the file's per-vertex colours (red/green/blue, optional alpha). On, colours import as the per-point colour attribute and display in the viewport; off, they are dropped at import for when a scan's colours are noise rather than signal. |

*Bypassed: emits nothing.*

### Import PLY <a id="import_ply"></a>

`import_ply` · v3 · Import · placed inside a geo

Palette search also matches: ply, import, scan, points, cloud.

Loads a PLY mesh or point cloud, binary or ASCII. Self-contained like STL: one file, no companions, no materials.

PLY is the scanning and photogrammetry format, so this usually heads a cleanup chain and pairs with the validator -- like every import it validates the raw file as it parses, which on a multi-million-point scan is where the issue counts actually matter. On the web the parse runs in an import worker off the main thread.

A file with no face element loads as a true point cloud and draws as camera-facing points. Vertex colours (red/green/blue, optional alpha, uchar or float) survive the import as the per-point colour attribute and display directly; the Vertex Colors toggle drops them at the door when a scan's colours are noise rather than signal. Points and point clouds are not click-selectable in the viewport; select their node on the canvas instead.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `geometry` | out | Geometry | The cooked geometry. Being the default output, a drag from the node's body wires from here, and a bypass passes the input through it. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `file` | assetRef | `` |  | The staged model file. Identity is the bytes' SHA-256, not the path, so re-staging the same file costs nothing and a saved `.slxy` carries a copy of the bytes -- the scene keeps loading after the original moves or is deleted. The picker is multi-select: stage companion files (an MTL, a `.bin`, textures) in the same go and the parser resolves them by name. Left empty the node cooks to nothing, without an error. |
| `scale` | float | `1.0` | 0 to 10000 | A uniform multiplier baked into the points at import, about the origin. Use it to reconcile units at the source -- a millimetre CAD export needs 0.001 to land in metres. Unlike a downstream `transform`, this is baked, so everything after it measures the scaled model. |
| `center_to_origin` | bool | `false` |  | Moves the model so its bounding-box centre sits at the origin. Applied after Scale. Worth turning on for a file authored far from the origin, which otherwise imports off-screen and orbits around nothing. |
| `vertex_colors` | bool | `true` |  | Keep the file's per-vertex colours (red/green/blue, optional alpha). On, colours import as the per-point colour attribute and display in the viewport; off, they are dropped at import for when a scan's colours are noise rather than signal. |

*Bypassed: emits nothing.*

### Import STL <a id="import_stl"></a>

`import_stl` · v2 · Import · placed inside a geo

Palette search also matches: stl, import, print.

Loads an STL mesh, binary or ASCII (the loader sniffs which). One file, no companions, no materials -- STL carries triangles and nothing else.

This is the 3D-printing and CAD-handoff path. It pairs with the validator more than most: STL is where degenerate triangles, non-manifold edges, and flipped windings actually show up, and the import validates the raw file as it parses, so the badge is populated before you wire anything downstream.

STL stores a normal per facet rather than per vertex, and the loader keeps none of them: a parsed STL arrives with positions and triangles and no normals at all. That is why Recompute Normals defaults to on -- turn it off and the mesh has no normals for anything downstream to shade with.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `geometry` | out | Geometry | The cooked geometry. Being the default output, a drag from the node's body wires from here, and a bypass passes the input through it. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `file` | assetRef | `` |  | The staged model file. Identity is the bytes' SHA-256, not the path, so re-staging the same file costs nothing and a saved `.slxy` carries a copy of the bytes -- the scene keeps loading after the original moves or is deleted. The picker is multi-select: stage companion files (an MTL, a `.bin`, textures) in the same go and the parser resolves them by name. Left empty the node cooks to nothing, without an error. |
| `scale` | float | `1.0` | 0 to 10000 | A uniform multiplier baked into the points at import, about the origin. Use it to reconcile units at the source -- a millimetre CAD export needs 0.001 to land in metres. Unlike a downstream `transform`, this is baked, so everything after it measures the scaled model. |
| `center_to_origin` | bool | `false` |  | Moves the model so its bounding-box centre sits at the origin. Applied after Scale. Worth turning on for a file authored far from the origin, which otherwise imports off-screen and orbits around nothing. |
| `recompute_normals` | bool | `true` |  | Computes vertex normals from the triangles. The STL loader keeps none of the file's facet normals, so off leaves the mesh with no normals at all -- leave this on unless something downstream is about to supply its own. |

*Bypassed: emits nothing.*

### Import glTF <a id="import_gltf"></a>

`import_gltf` · v2 · Import · placed inside a geo

Palette search also matches: gltf, glb, import.

Loads a glTF 2.0 model, either the self-contained binary `.glb` or the `.gltf` JSON with its companion `.bin` and textures resolved by file name against the staged assets. Materials come across natively -- glTF is the format that survives the round trip best.

Reach for it when you have a choice of export from the DCC: it heads the same chain as any import (`transform`, `merge`, `bounds` downstream) and, like the others, validates the raw file as it parses, so the Validation tab reports on it exactly as the desktop viewer does.

Draco-compressed glTF is rejected outright, with a message asking you to re-export without Draco -- there is no decoder in the app yet, and the check runs in the import worker before the parse so the previous geometry stays on screen. Prefer `.glb` when you can: `.gltf` splits into files that all have to be staged together.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `geometry` | out | Geometry | The cooked geometry. Being the default output, a drag from the node's body wires from here, and a bypass passes the input through it. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `file` | assetRef | `` |  | The staged model file. Identity is the bytes' SHA-256, not the path, so re-staging the same file costs nothing and a saved `.slxy` carries a copy of the bytes -- the scene keeps loading after the original moves or is deleted. The picker is multi-select: stage companion files (an MTL, a `.bin`, textures) in the same go and the parser resolves them by name. Left empty the node cooks to nothing, without an error. |
| `scale` | float | `1.0` | 0 to 10000 | A uniform multiplier baked into the points at import, about the origin. Use it to reconcile units at the source -- a millimetre CAD export needs 0.001 to land in metres. Unlike a downstream `transform`, this is baked, so everything after it measures the scaled model. |
| `center_to_origin` | bool | `false` |  | Moves the model so its bounding-box centre sits at the origin. Applied after Scale. Worth turning on for a file authored far from the origin, which otherwise imports off-screen and orbits around nothing. |
| `preserve_materials` | bool | `true` |  | On keeps the materials the file defines. Off drops every material binding and the material table with them, so the whole model draws in the renderer's neutral default -- the clay look you want when judging form, or when a file's own materials are fighting you. |

*Bypassed: emits nothing.*

### Texture Reference <a id="tex_ref"></a>

`tex_ref` · v1 · Import · placed inside a geo or inside a material network · image source silhouette

Palette search also matches: tex_ref, fetch, object merge, texture, reference.

Pulls the image a texture network publishes into this network as an Image wire. It reads across contexts by path, so no wire ever crosses a network boundary.

Point it at a `texnet` and feed the result into a map port on `principled`, or into the geo-side `material` node -- it is placeable in both Mat and Geo networks for exactly that reason. One texture network can back any number of these, which is how a texture gets authored once and used everywhere; editing the network recooks every referrer.

This is the fetch pattern rather than a wire, so the dependency is invisible on the canvas: nothing draws a line from the `texnet` to here, and the only record of the link is this node's Texture Network param. An unset path is harmless (no output, read downstream as no map), but a path pointing at a network with no display node is a cook error.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `image` | out | Image | The fetched image. It emits nothing at all while Texture Network is unset, which a downstream map port reads as `no map connected` rather than as an error. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `texture_path` | nodePath | `null` |  | The `texnet` to fetch from; only containers that open a texture context can be picked. What arrives is that network's display node output, so re-designating the display node inside it changes every referrer at once. Left unset this node simply emits nothing, but a path aimed at a deleted node or at a network that publishes nothing fails the cook rather than yielding a blank image. |

*Bypassed: emits nothing.*

## Lights

### Ambient Light <a id="ambient_light"></a>

`ambient_light` · v1 · Lights · placed scene · light silhouette

Palette search also matches: light, fill, environment.

A uniform fill: it adds the same light to every surface, whatever its position and whichever way it faces. No position, no direction, no shadow, no falloff.

The blunt instrument for lifting shadows that a key light left too dark. `hemisphere_light` is usually the better answer -- it costs the same and at least varies from sky to ground -- so reach for ambient when you specifically want flatness, or want a quick global lift while blocking out a scene.

It costs no light slot: ambient and hemisphere lights fold into the ambient term instead of competing for the interactive viewport's 8 direct-light slots, so stack as many as you like. Two honest limits: it ADDS to the IBL environment rather than scaling it, so it cannot dim an HDRI, and ambient occlusion still darkens it, so it will not fully flatten creases. Show Helper and Helper Size do nothing here -- with no position and no direction there is no honest shape to draw.

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `color` | color | `[1.0,1.0,1.0,1.0]` |  | The fill color, linear RGB, multiplied by Intensity and added to every surface equally. It multiplies the surface color like any other light. Keep it dim and slightly tinted -- a bright neutral ambient is what makes a render look washed out and unlit. Alpha is ignored. |
| `intensity` | float | `0.5` | 0 to 1000 | Linear multiplier on this light's contribution, and linear means what it says: doubling this doubles the light, and two lights an octave apart in this number are an octave apart on screen. 0 turns the light off without removing it from the scene, which is the quick way to A/B one you want to keep. There are still no lumens or watts behind the number, so it is not calibrated against the physical world, but it is consistent within a scene and against a value authored anywhere else: nothing is scaled behind your back on the way to the shader. |
| `visible` | bool | `true` |  | Whether this light is in the scene at all. Off removes its contribution and hides its helper, and releases its slot in the interactive viewport's 8-light budget for the next light that wants one. |
| `show_helper` | bool | `false` |  | This control does nothing on an ambient light. An ambient light has no position and no direction, so there is no shape to draw and no place to draw it; the viewport shows nothing however this is set. |
| `helper_size` | float | `1.0` | 0.1 to 10 | This control does nothing on an ambient light: there is no helper to size. |

*Bypassed: emits nothing.*

### Directional Light <a id="directional_light"></a>

`directional_light` · v2 · Lights · placed scene · light silhouette

Palette search also matches: light, sun, sky.

A parallel light, like the sun: every ray travels the same direction, so nothing is nearer to it and nothing falls off with distance. Only the direction from Position to Target matters.

The key light for most scenes -- sun, moon, a large window. Aim it by moving Target rather than Position, and pair it with a `hemisphere_light` or `ambient_light` to lift the shadow side, because a directional light on its own leaves every face turned away from it black.

Its Position lights nothing. The shading uses only the Position-to-Target direction, and the shadow frustum auto-fits the scene bounds instead of sitting at Position, so moving the node in space moves nothing but its helper arrow. It spends one of the 8 direct-light slots the interactive viewport binds -- that ceiling is the viewport's, not the scene's -- and Cast Shadow is exclusive: granting it here revokes it from every other light in a single undo step.

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `position` | vec3 | `[10.0,10.0,5.0]` |  | meters; Where the helper arrow is drawn, in metres, and the tail of the aiming vector. The shading ignores it: rays are parallel, so only the direction toward Target counts, and the shadow frustum fits itself to the scene rather than to this point. Move it to park the helper somewhere readable; the lighting will not change. |
| `target` | vec3 | `[0.0,0.0,0.0]` |  | meters; The point the light aims at, in metres. Only the direction from Position to Target is used, so the distance between them is irrelevant -- this is a rotation control wearing XYZ clothes. If Target and Position coincide the light falls back to pointing straight down. |
| `color` | color | `[1.0,1.0,1.0,1.0]` |  | The light's color, linear RGB. It multiplies the surface color, so a saturated light cannot put back a hue the surface does not reflect. Alpha is ignored. A slightly warm sun against a cool `hemisphere_light` fill is the cheapest believable daylight there is. |
| `intensity` | float | `4.5` | 0 to 1000 | Linear multiplier on this light's contribution, and linear means what it says: doubling this doubles the light, and two lights an octave apart in this number are an octave apart on screen. 0 turns the light off without removing it from the scene, which is the quick way to A/B one you want to keep. There are still no lumens or watts behind the number, so it is not calibrated against the physical world, but it is consistent within a scene and against a value authored anywhere else: nothing is scaled behind your back on the way to the shader. |
| `cast_shadow` | bool | `true` |  | Whether this light renders the shadow map. Exactly one light in the scene may cast at a time: switching this on here switches it off on every other light, as a single undo step. Switching it off here leaves the scene with no shadows until you grant it to another light. |
| `map_size` | enum (512 / 1024 / 2048) | `2048` |  | shown only while `cast_shadow` is on; The resolution the shadow map would render at, trading crisper shadow edges against memory and fill cost. This control does nothing today: the shadow map size is fixed by the host (2048 in the web app) and nothing reads this value. It is resolved and saved with the document, waiting on the per-light shadow work, so setting it now changes neither the image nor performance. |
| `bias` | float | `0.0001` | -0.01 to 0.01 | shown only while `cast_shadow` is on; Depth offset applied when testing against the shadow map, the usual dial for trading shadow acne against peter-panning. This control does nothing today: the shader hardcodes one bias for every caster and nothing reads this value. Dragging it will not change the image; if you are fighting acne, the bias is not currently yours to tune. |
| `visible` | bool | `true` |  | Whether this light is in the scene at all. Off removes its contribution and hides its helper, and releases its slot in the interactive viewport's 8-light budget for the next light that wants one. |
| `show_helper` | bool | `false` |  | Draw a wireframe arrow at Position pointing toward Target. Worth turning on while aiming: the direction is the only thing this light actually uses, and the arrow is the only way to see it. Hidden whenever Visible is off. |
| `helper_size` | float | `1.0` | 0.1 to 10 | How long the helper arrow is drawn, in world metres. Purely cosmetic -- it has no effect on the light. |

*Bypassed: emits nothing.*

### Environment <a id="environment"></a>

`environment` · v1 · Lights · placed scene · light silhouette

Palette search also matches: hdri, ibl, sky, lighting, environment.

Makes the lighting environment part of the scene rather than part of the application, so the HDRI you light with is saved in the file and comes back when you reopen it.

Drop it in the root graph beside your `geo` containers. It takes no wires: the scene builder reads it straight off the node, the same way it reads the light and camera nodes. Point it at a `.hdr` or `.exr`, then use Rotation to place the highlights and Intensity to balance the environment against your lights.

There is exactly one environment, so if a graph holds more than one of these the first in document order wins and the rest are ignored. An environment set here also takes precedence over one loaded through the viewport's own HDRI control, which stays available for scenes that have no environment node.

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `hdri` | assetRef | `` |  | The high-dynamic-range image that lights the scene: a Radiance `.hdr` or an `OpenEXR` `.exr`. It supplies both the ambient light and the reflections, so loading one changes the look of every material at once. Identity is the bytes' SHA-256 rather than the path, so a saved scene embeds a copy and still opens once the original file is gone. With no file staged the node asserts no environment at all, which leaves whatever background and procedural sky the viewport already had rather than going black. |
| `rotation` | float | `0.0` |  | Spins the environment around the vertical axis, in degrees. Moves the visible sky and the lighting it casts together, so it is how you place a highlight without moving a light. |
| `intensity` | float | `1.0` |  | Scales how much light the environment casts, leaving the visible sky alone. Use it to keep a backdrop readable while dialling the key it throws up or down. 1 is the image as it was authored. |
| `background` | enum (keep / hdri_sky) | `keep` |  | Whether the environment also claims the backdrop. **Keep** lights from the HDRI but leaves each viewport's own background alone, which is what you want for a product shot on white. **HDRI Sky** draws the image itself behind the scene. Solid and gradient backdrops stay per-viewport, so this never fights the background control on the pane toolbar. |

*Bypassed: emits nothing.*

### Hemisphere Light <a id="hemisphere_light"></a>

`hemisphere_light` · v1 · Lights · placed scene · light silhouette

Palette search also matches: light, sky, gradient.

A two-tone ambient: Sky Color from above, Ground Color from below, blended across each surface by how far its normal tilts up or down. No position, no direction, no shadow.

The default choice for fill, and a cheap stand-in for a real environment: a cool sky over a warm ground reads as outdoors without loading an HDRI. It sits under a `directional_light` key in most rigs. Prefer it to `ambient_light`, which costs exactly the same and gives none of the variation.

It costs no light slot -- ambient and hemisphere lights fold into the ambient term rather than taking one of the 8 direct-light slots. The blend is decided purely by the surface normal, so it is a gradient in ORIENTATION, not in space: a floor at the top of your scene still gets Sky Color, and nothing occludes the light except ambient occlusion. Having no position, its helper dome always draws at the world origin no matter where the scene sits.

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `sky_color` | color | `[1.0,1.0,1.0,1.0]` |  | Linear RGB reaching surfaces that face up; multiplied by Intensity. This is the dominant half in practice, because most of what you light -- floors, shoulders, the tops of things -- faces up. Alpha is ignored. |
| `ground_color` | color | `[0.2669999897480011,0.2669999897480011,0.2669999897480011,1.0]` |  | Linear RGB reaching surfaces that face down; multiplied by Intensity. Read it as bounce off the floor, and tint it toward whatever the floor is made of. Setting it equal to Sky Color makes this light exactly an `ambient_light`. Alpha is ignored. |
| `intensity` | float | `1.0` | 0 to 1000 | Linear multiplier on this light's contribution, and linear means what it says: doubling this doubles the light, and two lights an octave apart in this number are an octave apart on screen. 0 turns the light off without removing it from the scene, which is the quick way to A/B one you want to keep. There are still no lumens or watts behind the number, so it is not calibrated against the physical world, but it is consistent within a scene and against a value authored anywhere else: nothing is scaled behind your back on the way to the shader. |
| `visible` | bool | `true` |  | Whether this light is in the scene at all. Off removes its contribution and hides its helper, and releases its slot in the interactive viewport's 8-light budget for the next light that wants one. |
| `show_helper` | bool | `false` |  | Draw a wireframe dome, in Sky Color, at the WORLD ORIGIN -- a hemisphere light has no position, so the dome cannot follow your scene. It is an indicator that the light exists, not a picture of where it is. |
| `helper_size` | float | `1.0` | 0.1 to 10 | How big the helper dome is drawn, in world metres. Purely cosmetic -- it has no effect on the light. |

*Bypassed: emits nothing.*

### Point Light <a id="point_light"></a>

`point_light` · v3 · Lights · placed scene · light silhouette

Palette search also matches: light, omni, bulb.

An omnidirectional light: it emits from Position equally in every direction, dimming with distance according to Range and Decay.

The workhorse for a local source -- a bulb, a candle, a muzzle flash. Drop it in the root graph beside your `geo` containers; it takes no wires, because the scene builder reads its params straight off the node rather than passing a light down a chain. Reach for `directional_light` when you want a sun instead, or `spot_light` when you want the same falloff inside a cone.

Two limits bite. It spends one of the 8 direct-light slots the interactive viewport binds, and past 8 the first 8 in document order win with the rest dropped silently, so a scene that quietly stops responding to new lights is probably at that cap (ambient and hemisphere lights are free, and an invisible light gives its slot back). That ceiling is the viewport's, not the scene's -- rendered output reads every light. And shadow casting is exclusive: switching Cast Shadow on here switches it off on every other light, in one undo step.

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `position` | vec3 | `[10.0,10.0,5.0]` |  | meters; Where the light sits, in metres. Everything radiates from here: Range and Decay measure their distance from this point, and the helper sphere is drawn around it. |
| `color` | color | `[1.0,1.0,1.0,1.0]` |  | The light's color, linear RGB. It multiplies the surface color, so a saturated light cannot put back a hue the surface does not reflect. Alpha is ignored. Tint this rather than Intensity when you want warmth, not brightness. |
| `intensity` | float | `4.5` | 0 to 1000 | Linear multiplier on this light's contribution, and linear means what it says: doubling this doubles the light, and two lights an octave apart in this number are an octave apart on screen. 0 turns the light off without removing it from the scene, which is the quick way to A/B one you want to keep. There are still no lumens or watts behind the number, so it is not calibrated against the physical world, but it is consistent within a scene and against a value authored anywhere else: nothing is scaled behind your back on the way to the shader. |
| `range` | float | `0.0` | 0 to 100000 | meters; Distance at which the light's contribution reaches zero, in metres. 0, the default, means no cutoff at all: the light carries infinitely far and only Decay dims it. Above 0 the falloff is windowed so brightness arrives at exactly zero on the Range sphere, which is how you stop a lamp from lighting the far side of a set. Range and Decay are independent; both apply when both are set. |
| `decay` | float | `2.0` | 0 to 10 | Falloff exponent: brightness is divided by the distance raised to this power. The default 2 is physical inverse-square. 0 disables decay outright, so the light is equally bright at any distance -- handy for a flat fill, wrong for anything meant to read as a real source. Values between 0 and 2 give the gentler falloff that is often easier to light with than the physical answer. |
| `radius` | float | `0.0` | 0 to 1000 | meters; How big the emitter is, in metres. 0, the default, is a mathematical point, which casts a shadow with a perfectly hard edge -- the giveaway that a light is not a real object. Give it a size and the shadow gains a penumbra that widens with distance from the surface, the way a real lamp's does, because part of the emitter is visible where the rest is hidden.

This is read by rendered output only; the interactive viewport draws hard-edged shadows whatever you set here. That is not an oversight to be fixed later: a shadow map answers one visibility question from one place, and blurring it would soften a contact shadow as much as a distant one. |
| `cast_shadow` | bool | `true` |  | Whether this light renders the shadow map. Exactly one light in the scene may cast at a time: switching this on here switches it off on every other light, as a single undo step. Switching it off here leaves the scene with no shadows until you grant it to another light. |
| `map_size` | enum (512 / 1024 / 2048) | `1024` |  | shown only while `cast_shadow` is on; The resolution the shadow map would render at, trading crisper shadow edges against memory and fill cost. This control does nothing today: the shadow map size is fixed by the host (2048 in the web app) and nothing reads this value. It is resolved and saved with the document, waiting on the per-light shadow work, so setting it now changes neither the image nor performance. |
| `bias` | float | `-0.0001` | -0.01 to 0.01 | shown only while `cast_shadow` is on; Depth offset applied when testing against the shadow map, the usual dial for trading shadow acne against peter-panning. This control does nothing today: the shader hardcodes one bias for every caster and nothing reads this value. Dragging it will not change the image; if you are fighting acne, the bias is not currently yours to tune. |
| `visible` | bool | `true` |  | Whether this light is in the scene at all. Off removes its contribution and hides its helper, and releases its slot in the interactive viewport's 8-light budget for the next light that wants one. |
| `show_helper` | bool | `false` |  | Draw a wireframe sphere at Position, so you can see where the light is without hunting for it. The helper is drawn in the light's own color, and is hidden whenever Visible is off. |
| `helper_size` | float | `1.0` | 0.1 to 10 | How big the helper sphere is drawn, in world metres. Purely cosmetic -- it has no effect on the light itself. Raise it when the helper is lost in a large scene. |

*Bypassed: emits nothing.*

### Rect Area Light <a id="rect_area_light"></a>

`rect_area_light` · v4 · Lights · placed scene · light silhouette

Palette search also matches: light, area, softbox, panel.

A rectangular emitter -- the softbox of the light kit -- with a Width and Height, sitting at Translate and facing straight down until you rotate it.

What you reach for it for: soft key and fill on a product or character shot, and the broad specular roll-off a panel gives that a pinpoint source cannot. Widen it and the highlight spreads and the terminator softens; turn it edge-on and it dims, because you are looking at less of it.

The shading integrates over the whole rectangle (linearly transformed cosines), so Width, Height and Rotate all reach the image rather than only the helper. Two caveats remain. It cannot cast shadows -- it has no Cast Shadow param, and the exclusive shadow caster stays with the punctual lights -- so it lights through geometry. And Helper Size is ignored, because the helper rectangle takes its size from Width and Height. It spends one of the 8 direct-light slots the interactive viewport binds, like any other.

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `translate` | vec3 | `[0.0,0.0,0.0]` |  | meters; The centre of the rectangle, in metres. Unrotated, the panel lies flat with its Width along X, its Height along Z, and its emitting face pointing straight down. |
| `color` | color | `[1.0,1.0,1.0,1.0]` |  | The panel's color, linear RGB, multiplied by Intensity. It multiplies the surface color like any other light. Alpha is ignored. |
| `intensity` | float | `4.5` | 0 to 1000 | Linear multiplier on this light's contribution, and linear means what it says: doubling this doubles the light, and two lights an octave apart in this number are an octave apart on screen. 0 turns the light off without removing it from the scene, which is the quick way to A/B one you want to keep. There are still no lumens or watts behind the number, so it is not calibrated against the physical world, but it is consistent within a scene and against a value authored anywhere else: nothing is scaled behind your back on the way to the shader. |
| `width` | float | `10.0` | 0.1 to 1000 | meters; One edge length of the emitting rectangle, in metres, along the panel's local X. It reaches the shading: a wider panel spreads the specular highlight and softens the terminator, and emits more total light, because a bigger emitter is a brighter one at the same intensity. |
| `height` | float | `10.0` | 0.1 to 1000 | meters; The other edge length of the emitting rectangle, in metres, along the panel's local Z. Setting it far from Width gives the long thin source a strip light makes, which stretches a highlight along one axis only. |
| `rotate` | vec3 | `[0.0,0.0,0.0]` |  | degrees; Euler angles in degrees, composed in XYZ order, turning the panel away from face-down. Rotating about Y is the one that matters for a square panel; for a rectangular one it also decides which way the long edge runs, which is the difference between a strip light lying down and standing up. |
| `two_sided` | bool | `false` |  | Emit from the back face as well as the front. Off, the panel lights only what it faces and anything behind it is untouched, which is what a real softbox does. On, it behaves like a floating pane of light, which is useful for filling a room from a plane in its middle without placing two lights. |
| `visible` | bool | `true` |  | Whether this light is in the scene at all. Off removes its contribution and hides its helper, and releases its slot in the interactive viewport's 8-light budget for the next light that wants one. |
| `show_helper` | bool | `false` |  | Draw the emitting rectangle at Translate, sized by Width and Height, with a short stub along its normal so the emitting side is unambiguous. Worth turning on: it is the only place Width and Height have any visible effect at all. |
| `helper_size` | float | `1.0` | 0.1 to 10 | This control does nothing on a rect-area light. The helper rectangle takes its size from Width and Height instead. |

*Bypassed: emits nothing.*

### Spot Light <a id="spot_light"></a>

`spot_light` · v3 · Lights · placed scene · light silhouette

Palette search also matches: light, cone, flashlight.

A cone of light from Position toward Target: full intensity in the middle, nothing past the outer Angle, and a Penumbra that controls how abruptly it gets there.

The pick for a deliberate pool of light -- a lamp, a torch, a stage special. It falls off with distance exactly like a `point_light` and shares its Range and Decay, so think of it as a point light wearing a cone; use the point light when you do not need the cone.

Penumbra defaults to 0, which gives a razor-hard cone edge -- the classic tell of a CG spotlight, and rarely what you want; a little goes a long way. Angle is the HALF-angle, so the default 45 spreads 90 degrees in total. The light spends one of the 8 direct-light slots the interactive viewport binds -- that ceiling is the viewport's, not the scene's -- and Cast Shadow is exclusive: granting it here revokes it from every other light in a single undo step.

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `position` | vec3 | `[10.0,10.0,5.0]` |  | meters; The apex of the cone, in metres: where the light sits and emits from. Range and Decay measure distance from this point, and the helper cone is drawn from it toward Target. |
| `target` | vec3 | `[0.0,0.0,0.0]` |  | meters; The point the cone aims at, in metres. Only the direction from Position is used, so moving Target further away aims the light without lengthening its reach -- that is Range. If Target and Position coincide the light points straight down. |
| `color` | color | `[1.0,1.0,1.0,1.0]` |  | The light's color, linear RGB. It multiplies the surface color, so a saturated light cannot put back a hue the surface does not reflect. Alpha is ignored. |
| `intensity` | float | `4.5` | 0 to 1000 | Linear multiplier on this light's contribution, and linear means what it says: doubling this doubles the light, and two lights an octave apart in this number are an octave apart on screen. 0 turns the light off without removing it from the scene, which is the quick way to A/B one you want to keep. There are still no lumens or watts behind the number, so it is not calibrated against the physical world, but it is consistent within a scene and against a value authored anywhere else: nothing is scaled behind your back on the way to the shader. |
| `range` | float | `0.0` | 0 to 100000 | meters; Distance at which the light's contribution reaches zero, in metres. 0, the default, means no cutoff at all: the light carries infinitely far and only Decay dims it. Above 0 the falloff is windowed so brightness arrives at exactly zero on the Range sphere, which is how you stop a lamp from lighting the far side of a set. Range and Decay are independent; both apply when both are set. |
| `decay` | float | `2.0` | 0 to 10 | Falloff exponent: brightness is divided by the distance raised to this power. The default 2 is physical inverse-square. 0 disables decay outright, so the light is equally bright at any distance -- handy for a flat fill, wrong for anything meant to read as a real source. Values between 0 and 2 give the gentler falloff that is often easier to light with than the physical answer. |
| `angle` | float | `45.0` | 1 to 89 | degrees; Half-angle of the cone's outer edge, in degrees: the full spread is twice this, so the default 45 is a 90-degree cone. Nothing outside the angle receives any light. It also sets the width of the helper cone. |
| `penumbra` | float | `0.0` | 0 to 1 | normalized; How soft the cone edge is, 0 to 1. 0, the default, is a hard edge: full intensity right up to Angle, then nothing. Raising it shrinks the full-intensity inner cone toward the centre -- the inner half-angle is Angle * (1 - Penumbra) -- and fades across the gap, so 1 spreads the falloff over the whole cone and leaves no flat core at all. |
| `radius` | float | `0.0` | 0 to 1000 | meters; How big the emitter is, in metres. 0, the default, is a mathematical point, which casts a shadow with a perfectly hard edge -- the giveaway that a light is not a real object. Give it a size and the shadow gains a penumbra that widens with distance from the surface, the way a real lamp's does, because part of the emitter is visible where the rest is hidden.

This is read by rendered output only; the interactive viewport draws hard-edged shadows whatever you set here. That is not an oversight to be fixed later: a shadow map answers one visibility question from one place, and blurring it would soften a contact shadow as much as a distant one. |
| `cast_shadow` | bool | `true` |  | Whether this light renders the shadow map. Exactly one light in the scene may cast at a time: switching this on here switches it off on every other light, as a single undo step. Switching it off here leaves the scene with no shadows until you grant it to another light. |
| `map_size` | enum (512 / 1024 / 2048) | `1024` |  | shown only while `cast_shadow` is on; The resolution the shadow map would render at, trading crisper shadow edges against memory and fill cost. This control does nothing today: the shadow map size is fixed by the host (2048 in the web app) and nothing reads this value. It is resolved and saved with the document, waiting on the per-light shadow work, so setting it now changes neither the image nor performance. |
| `bias` | float | `-0.0001` | -0.01 to 0.01 | shown only while `cast_shadow` is on; Depth offset applied when testing against the shadow map, the usual dial for trading shadow acne against peter-panning. This control does nothing today: the shader hardcodes one bias for every caster and nothing reads this value. Dragging it will not change the image; if you are fighting acne, the bias is not currently yours to tune. |
| `visible` | bool | `true` |  | Whether this light is in the scene at all. Off removes its contribution and hides its helper, and releases its slot in the interactive viewport's 8-light budget for the next light that wants one. |
| `show_helper` | bool | `false` |  | Draw a wireframe cone from Position along the aim, at the outer Angle, plus a dimmer inner circle when Penumbra has opened a gap worth seeing. The cone is drawn out to Range when it has one, so the helper shows where the light actually stops. |
| `helper_size` | float | `1.0` | 0.1 to 10 | How long the helper cone is drawn, in world metres, when Range is 0 (an unbounded light has no natural length to draw). Once Range is set the cone is drawn out to Range instead and this does nothing. Cosmetic either way. |

*Bypassed: emits nothing.*

## Shaders

### MatCap <a id="matcap"></a>

`matcap` · v1 · Shaders · placed inside a material network

Palette search also matches: matcap, material capture, sculpt, zbrush.

A material-capture surface: the connected image is sampled by the view-space normal and returned as-is, with no lighting whatsoever.

Reach for it for a sculpt preview, or for a stylized look that must not depend on the scene's lights or HDRI -- the way ZBrush and Blender's solid mode shade. Feed the image from `tex_ref` or an `import_image` and designate this the network's display node.

The matcap image IS the base-color texture role; no separate matcap slot exists anywhere in the pipeline. The lighting is baked into the image, so shading follows the CAMERA: orbit around a fixed object and the highlights swing with you rather than staying put. Alpha is forced opaque, and a viewport material override (Clay, Chrome, Silhouette) wins over this model entirely.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `matcap` | in | Image | The matcap image: a sphere lit the way you want the surface to look. It is sampled at the view-space normal, not at the mesh's UVs, so the mesh needs no UVs at all. Left empty, the base-color slot falls back to white and the surface renders as the flat Tint. |
| `material` | out | Material | The material this node builds. Wire it into `mix_material`, or designate this node as the network's display node so the enclosing `matnet` publishes it to `material` nodes in Reference mode. Being the default output, a drag from the node's body wires from here. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `tint` | color | `[1.0,1.0,1.0,1.0]` |  | Multiplied into every matcap sample. White (the default) leaves the image exactly as authored; a colour recolours it without needing a second image. Its alpha is ignored -- matcap always renders opaque. |
| `material_name` | text | `` |  | What the material is called wherever it is listed. It has no effect on the shading. Empty falls back to the node type's own name. |

*Bypassed: emits nothing.*

### Material <a id="material"></a>

`material` · v3 · Shaders · placed inside a geo

Palette search also matches: material, pbr, texture, shader, color.

Assigns one material to the meshes of the input geometry. The material is either built INLINE from this node's own factors and map ports, or taken by REFERENCE from a `matnet` elsewhere in the scene.

Drop it at the tail of a geo network, after the modelling and the UV work: it only rewrites the material table and each mesh's material index, so points, normals and UVs pass through untouched. Reach for Inline for a one-off surface nothing else needs. Reach for Reference once a material is shared: point `material_path` at a `matnet` and one edit inside that network updates every object referring to it.

`target` decides how much this node claims. Empty, it overrides everything -- the material table collapses to this one material and every mesh points at it. Non-empty, it appends instead and re-points only the meshes whose name contains that substring. Note that Reference mode hides the factor params but NOT the five map ports: they stay on the node and are ignored, because the referenced network owns the whole surface.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `geometry` | in | Geometry | required; the default input; The geometry to dress. Required: this node only rewrites the material table and each mesh's material index, so it has nothing to assign to without an input. Points, normals and UVs pass through untouched. |
| `base_color_map` | in | Image | The albedo texture, read as sRGB and multiplied by the Base Color factor. Connecting it neutralizes that factor to white, so the map alone drives the colour. Left empty, the factor is the colour. |
| `normal_map` | in | Image | A tangent-space normal map, read as linear data. It has no factor to neutralize, so nothing dims when you connect it. Left empty, the surface samples a flat normal and shades from the mesh normals alone. |
| `metallic_roughness_map` | in | Image | glTF-packed: roughness in G, metallic in B. Connecting it neutralizes BOTH the Metallic and Roughness factors to 1.0, so one port takes over two channels at once -- there is no way to map one of them and keep the scalar on the other. |
| `occlusion_map` | in | Image | Baked ambient occlusion, read from R and composited into the packed ORM texture. It only reaches the renderer when a Metallic Roughness Map is connected too AND the two images have identical dimensions; connected alone, or at a mismatched size, it is silently dropped. |
| `emissive_map` | in | Image | Light the surface emits by itself, read as sRGB and multiplied by the Emissive factor. Connecting it neutralizes that factor to white. Left empty, the factor alone decides the emission. |
| `geometry` | out | Geometry | The cooked geometry. Being the default output, a drag from the node's body wires from here, and a bypass passes the input through it. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `mode` | enum (inline / reference) | `inline` |  | under "Assignment"; Inline builds the surface from this node's own factors and map ports. Reference ignores both and assigns the material a `matnet` publishes instead. Switching to Reference hides the factors but keeps their values, so switching back restores exactly what you had. |
| `material_path` | nodePath | `null` |  | under "Assignment"; shown only while `mode` is `reference`; The `matnet` to take the material from. What arrives is whatever that network's display node publishes, so re-designating the display node inside it re-points every referrer at once. In Reference mode this is required: unset, dangling, or aimed at a network that publishes nothing all fail the cook rather than quietly assigning a default surface. |
| `target` | text | `` |  | under "Assignment"; A case-sensitive substring matched against mesh names. Empty is the override-all default: the material table collapses to this one material and every mesh takes it. Non-empty appends the material and re-points only the matching meshes, leaving the rest on whatever they already had, so several `material` nodes in a row can dress different parts of one merged object. Primitives are named after their type (`box`, `sphere`); imported meshes keep the names from the file. |
| `base_color` | color | `[0.800000011920929,0.800000011920929,0.800000011920929,1.0]` |  | overridden when `base_color_map` is connected; shown only while `mode` is `inline`; The surface colour of a dielectric, or the reflectance tint of a metal, multiplied into the base-color sample. Connecting a Base Color Map neutralizes this to white so the map alone drives the channel; the value you set is kept for when the map comes off again. Alpha is carried, but these nodes only build Opaque materials today. |
| `metallic` | float | `0.0` | 0 to 1 | overridden when `metallic_roughness_map` is connected; shown only while `mode` is `inline`; How metallic the surface is. 0 is a dielectric: coloured diffuse plus an uncoloured specular highlight. 1 is bare metal: no diffuse at all, and the reflection takes the base colour. Values in between are not physical -- reach for them for a worn or corroded edge, not as a dial for shininess (that is Roughness). |
| `roughness` | float | `0.5` | 0 to 1 | overridden when `metallic_roughness_map` is connected; shown only while `mode` is `inline`; Microsurface scatter, which sets how wide the specular lobe is: 0 is a mirror, 1 is fully diffuse. The shader clamps the low end to 0.04, so a perfect mirror is not reachable and highlights never collapse into a single aliased pixel. |
| `emissive` | color | `[0.0,0.0,0.0,1.0]` |  | overridden when `emissive_map` is connected; shown only while `mode` is `inline`; Light the surface emits on its own, added on top of the lit result, so an emissive surface stays visible in shadow. Black (the default) is no emission. It lights nothing else: there is no emissive bounce, so a glowing panel does not brighten the wall behind it. |
| `emissive_strength` | float | `1.0` | 0 to 100 | shown only while `mode` is `inline`; Multiplies Emissive, so emission can exceed the unit range a colour can express. 1 is no change. Reach for it when a surface should read as a light source rather than as a bright material: past about 1 the tone mapper starts rolling it off, which is what makes it bloom rather than clip. |
| `material_name` | text | `` |  | What the material is called wherever it is listed. It has no effect on the shading. Empty falls back to `material`. The geo-side `material` node keeps this visible in Reference mode but ignores it there: a referenced network's material carries its own name. |
| `clearcoat` | float | `0.0` | 0 to 1 | under "Clearcoat"; shown only while `mode` is `inline`; A thin glossy layer over the surface, the lacquer on a car panel or the varnish on wood. 0 is no coat. The coat adds its own reflection and dims what is under it by whatever it reflects away, so a coated surface reads slightly darker as well as shinier. It is an analytic approximation in the interactive viewport rather than a simulated layer, and a traced render evaluates it by a different method, so the two will not match pixel for pixel. Judge the coat in the render. |
| `clearcoat_roughness` | float | `0.0` | 0 to 1 | under "Clearcoat"; shown only while `clearcoat` is on and `mode` is `inline`; How polished the coat is, independent of the surface beneath it. This is the point of a separate coat: a rough, worn base under a mirror-smooth lacquer is a combination one roughness value cannot express. |
| `sheen_color` | color | `[0.0,0.0,0.0,1.0]` |  | under "Sheen"; shown only while `mode` is `inline`; The colour of the soft retroreflective rim that fabric has, brightest where the surface turns away from you. Black, the default, is no sheen, which is why this one starts at black rather than white. Velvet, satin and brushed cloth are what it is for. It is an analytic approximation in the interactive viewport, and a traced render evaluates it by a different method, so the two will not match pixel for pixel. Judge the sheen in the render. |
| `sheen_roughness` | float | `0.0` | 0 to 1 | under "Sheen"; shown only while `mode` is `inline`; How wide the sheen band is. Low keeps it a tight rim at the silhouette; high spreads it across the whole surface for a dusty, powdery look. |
| `iridescence` | float | `0.0` | 0 to 1 | under "Iridescence"; shown only while `mode` is `inline`; Thin-film interference: the shifting colours of a soap bubble, an oil slick or anodized metal. 0 is none. The hue depends on viewing angle and on the film's thickness, so it moves as the camera moves, which is the whole effect. It is an analytic approximation in the interactive viewport, and a traced render evaluates it by a different method, so the two will not match pixel for pixel. Judge the film in the render. |
| `iridescence_ior` | float | `1.3` | 1 to 3 | under "Iridescence"; shown only while `iridescence` is on and `mode` is `inline`; Index of refraction of the film itself, not of the surface under it. It sets how strongly the film bends light and so how saturated the interference colours are. |
| `iridescence_thickness_min` | float | `100.0` | 0 to 2000 | under "Iridescence"; shown only while `iridescence` is on and `mode` is `inline`; The low end of the film thickness range, in nanometres. It only matters once a thickness map varies the film across the surface; with no map the maximum is used everywhere. |
| `iridescence_thickness_max` | float | `400.0` | 0 to 2000 | under "Iridescence"; shown only while `iridescence` is on and `mode` is `inline`; The high end of the film thickness range, in nanometres, and the thickness used everywhere when no thickness map is present. This is the dial that chooses which colours appear: a few hundred nanometres is where visible light interferes, and sweeping it walks the whole rainbow. |
| `specular_intensity` | float | `1.0` | 0 to 1 | under "Specular and anisotropy"; shown only while `mode` is `inline`; Scales the reflectance a dielectric has when you look straight at it, the value IOR derives. 1 leaves it alone. Lower it to dull a surface's reflections without making it rougher, which roughness alone cannot do. It has no effect on metals, whose reflectance is the base colour. |
| `specular_color` | color | `[1.0,1.0,1.0,1.0]` |  | under "Specular and anisotropy"; shown only while `mode` is `inline`; Tints that same head-on reflectance. White is untinted and is what almost every real dielectric wants; this exists for the ones that do not, and for matching a reference that was authored with it. |
| `anisotropy` | float | `0.0` | 0 to 1 | under "Specular and anisotropy"; shown only while `mode` is `inline`; Stretches the specular highlight along one direction instead of leaving it round: brushed metal, hair, the bottom of a saucepan. 0 is isotropic. It follows the surface's tangents, so it needs UVs to point anywhere meaningful. The interactive viewport and a traced render evaluate the stretched highlight by different methods, so the two will not match pixel for pixel. Judge the highlight in the render. |
| `anisotropy_rotation` | float | `0.0` | -360 to 360 | under "Specular and anisotropy"; degrees; shown only while `anisotropy` is on and `mode` is `inline`; Turns the stretch direction within the surface, for when the brushing runs across the UVs rather than along them. |
| `transmission` | float | `0.0` | 0 to 1 | shown only while `mode` is `inline`; How much light passes through the surface instead of scattering back off it: glass, water, a thin plastic. 0 is opaque. This is not the same as alpha, which makes a surface partly absent; transmission keeps the surface and its reflections and lets light through it.

In the interactive viewport it refracts the environment, not the objects behind the surface. Glass reads correctly against an environment and shows nothing of what is behind it. A traced render does carry light through to what is behind, so the two differ by more than pixels here: judge glass in the render. The traced result is itself an approximation, close enough to light a shot by and not a physical prediction. |
| `ior` | float | `1.5` | 1 to 3 | shown only while `mode` is `inline`; Index of refraction: how strongly the material bends light, and with it how much reflects at a glancing angle. 1.5 is window glass and the default every surface used before this was exposed. Water is about 1.33, diamond about 2.42. It drives reflectance whether or not the surface transmits. |
| `thickness` | float | `0.0` | 0 to 1000 | shown only while `transmission` is on and `mode` is `inline`; How far light travels through the interior, in world units. 0 means the surface is thin-walled, a bubble or a pane with no volume behind it. It only matters alongside Attenuation Distance, which is what turns distance travelled into colour. |
| `attenuation_color` | color | `[1.0,1.0,1.0,1.0]` |  | under "Absorption"; shown only while `transmission` is on and `mode` is `inline`; The colour light becomes after travelling Attenuation Distance through the interior. White is no tint. This is why thick green glass is green at its edge and clear at its face: the colour is a property of the distance travelled, not of the surface. |
| `attenuation_distance` | float | `0.0` | 0 to 1000 | under "Absorption"; shown only while `transmission` is on and `mode` is `inline`; The distance over which light reaches Attenuation Color. Shorter absorbs faster, so a small value tints even thin glass strongly. 0 disables absorption entirely and is the default, which is why a transmissive surface starts out water-clear. |

*Bypassed: passes `geometry` straight through.*

### Mix Material <a id="mix_material"></a>

`mix_material` · v1 · Shaders · placed inside a material network · gather silhouette

Palette search also matches: mix, blend, layer, material.

Blends two materials, but only partially. The scalar factors -- base colour, metallic, roughness, emissive, toon bands -- interpolate between A and B by Factor. Everything else does NOT blend: the five texture maps, the shading model and the alpha settings are taken wholesale from whichever side is dominant, which is B at Factor 0.5 and above, A below it.

So it does what you expect for two untextured surfaces of the same shading model: dialing between a rough dielectric and a polished metal, or animating a preset-to-preset roughness change. It misleads as soon as either side carries maps or a different model, because those pop over at the midpoint instead of crossfading. True map and shading-model blending needs shader work and is on the milestone backlog; this node is a documented approximation until then.

Put plainly: a sweep from 0 to 1 is smooth in the factors and discontinuous at 0.5 in everything else. If what you actually want is two materials on one object, assign them to different meshes with two `material` nodes and their `target` filters instead of mixing here. Both inputs are required: this node fails its cook until each side has a material.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `a` | in | Material | required; the default input; The first material, and what Factor 0 resolves to. It is also the dominant side BELOW Factor 0.5, so its maps and shading model are the ones used there. Bypassing the node passes this input straight through. |
| `b` | in | Material | required; The second material, and what Factor 1 resolves to. It is the dominant side at Factor 0.5 AND ABOVE, so its maps and shading model take over from the midpoint on -- exactly 0.5 already counts as B, not as a tie. |
| `material` | out | Material | The material this node builds. Wire it into `mix_material`, or designate this node as the network's display node so the enclosing `matnet` publishes it to `material` nodes in Reference mode. Being the default output, a drag from the node's body wires from here. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `factor` | float | `0.5` | 0 to 1 | Where between A (0) and B (1) the scalar factors land. It also picks the dominant side for the half of the material that does not blend: below 0.5 A's maps and shading model are used, at 0.5 and above B's. That switch is a hard cut, so sweeping this param pops at the midpoint whenever the two sides differ in their maps or their model. |
| `material_name` | text | `` |  | What the material is called wherever it is listed. It has no effect on the shading. Empty falls back to the node type's own name. |

*Bypassed: passes `a` straight through.*

### Principled <a id="principled"></a>

`principled` · v2 · Shaders · placed inside a material network

Palette search also matches: principled, pbr, surface, standard.

The physically-based metallic-roughness surface: base colour, metallic, roughness and emissive as factors, each with an optional texture map port that takes its channel over.

This is the surface to reach for first inside a `matnet`, and the one the renderer's Cook-Torrance path with image-based lighting exists for. Feed its map ports from `tex_ref` or an `import_image`, then either designate it the network's display node or run it into `mix_material` first.

The factor-and-map pairing is a hand-off, not a blend: connecting a map sets its factor to the multiplicative identity (white, or 1.0) so the map alone drives the channel, and the parameter panel dims the factor to say so. Metallic Roughness is one port for two channels and neutralizes both at once. It is the same surface builder the geo-side `material` node uses inline; the difference is only that this one outputs a Material wire instead of assigning to geometry.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `base_color_map` | in | Image | The albedo texture, read as sRGB and multiplied by the Base Color factor. Connecting it neutralizes that factor to white, so the map alone drives the colour. Left empty, the factor is the colour. |
| `normal_map` | in | Image | A tangent-space normal map, read as linear data. It has no factor to neutralize, so nothing dims when you connect it. Left empty, the surface samples a flat normal and shades from the mesh normals alone. |
| `metallic_roughness_map` | in | Image | glTF-packed: roughness in G, metallic in B. Connecting it neutralizes BOTH the Metallic and Roughness factors to 1.0, so one port takes over two channels at once -- there is no way to map one of them and keep the scalar on the other. |
| `occlusion_map` | in | Image | Baked ambient occlusion, read from R and composited into the packed ORM texture. It only reaches the renderer when a Metallic Roughness Map is connected too AND the two images have identical dimensions; connected alone, or at a mismatched size, it is silently dropped. |
| `emissive_map` | in | Image | Light the surface emits by itself, read as sRGB and multiplied by the Emissive factor. Connecting it neutralizes that factor to white. Left empty, the factor alone decides the emission. |
| `material` | out | Material | The material this node builds. Wire it into `mix_material`, or designate this node as the network's display node so the enclosing `matnet` publishes it to `material` nodes in Reference mode. Being the default output, a drag from the node's body wires from here. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `base_color` | color | `[0.800000011920929,0.800000011920929,0.800000011920929,1.0]` |  | overridden when `base_color_map` is connected; The surface colour of a dielectric, or the reflectance tint of a metal, multiplied into the base-color sample. Connecting a Base Color Map neutralizes this to white so the map alone drives the channel; the value you set is kept for when the map comes off again. Alpha is carried, but these nodes only build Opaque materials today. |
| `metallic` | float | `0.0` | 0 to 1 | overridden when `metallic_roughness_map` is connected; How metallic the surface is. 0 is a dielectric: coloured diffuse plus an uncoloured specular highlight. 1 is bare metal: no diffuse at all, and the reflection takes the base colour. Values in between are not physical -- reach for them for a worn or corroded edge, not as a dial for shininess (that is Roughness). |
| `roughness` | float | `0.5` | 0 to 1 | overridden when `metallic_roughness_map` is connected; Microsurface scatter, which sets how wide the specular lobe is: 0 is a mirror, 1 is fully diffuse. The shader clamps the low end to 0.04, so a perfect mirror is not reachable and highlights never collapse into a single aliased pixel. |
| `emissive` | color | `[0.0,0.0,0.0,1.0]` |  | overridden when `emissive_map` is connected; Light the surface emits on its own, added on top of the lit result, so an emissive surface stays visible in shadow. Black (the default) is no emission. It lights nothing else: there is no emissive bounce, so a glowing panel does not brighten the wall behind it. |
| `emissive_strength` | float | `1.0` | 0 to 100 | Multiplies Emissive, so emission can exceed the unit range a colour can express. 1 is no change. Reach for it when a surface should read as a light source rather than as a bright material: past about 1 the tone mapper starts rolling it off, which is what makes it bloom rather than clip. |
| `material_name` | text | `` |  | What the material is called wherever it is listed. It has no effect on the shading. Empty falls back to `material`. The geo-side `material` node keeps this visible in Reference mode but ignores it there: a referenced network's material carries its own name. |
| `clearcoat` | float | `0.0` | 0 to 1 | under "Clearcoat"; A thin glossy layer over the surface, the lacquer on a car panel or the varnish on wood. 0 is no coat. The coat adds its own reflection and dims what is under it by whatever it reflects away, so a coated surface reads slightly darker as well as shinier. It is an analytic approximation in the interactive viewport rather than a simulated layer, and a traced render evaluates it by a different method, so the two will not match pixel for pixel. Judge the coat in the render. |
| `clearcoat_roughness` | float | `0.0` | 0 to 1 | under "Clearcoat"; shown only while `clearcoat` is on; How polished the coat is, independent of the surface beneath it. This is the point of a separate coat: a rough, worn base under a mirror-smooth lacquer is a combination one roughness value cannot express. |
| `sheen_color` | color | `[0.0,0.0,0.0,1.0]` |  | under "Sheen"; The colour of the soft retroreflective rim that fabric has, brightest where the surface turns away from you. Black, the default, is no sheen, which is why this one starts at black rather than white. Velvet, satin and brushed cloth are what it is for. It is an analytic approximation in the interactive viewport, and a traced render evaluates it by a different method, so the two will not match pixel for pixel. Judge the sheen in the render. |
| `sheen_roughness` | float | `0.0` | 0 to 1 | under "Sheen"; How wide the sheen band is. Low keeps it a tight rim at the silhouette; high spreads it across the whole surface for a dusty, powdery look. |
| `iridescence` | float | `0.0` | 0 to 1 | under "Iridescence"; Thin-film interference: the shifting colours of a soap bubble, an oil slick or anodized metal. 0 is none. The hue depends on viewing angle and on the film's thickness, so it moves as the camera moves, which is the whole effect. It is an analytic approximation in the interactive viewport, and a traced render evaluates it by a different method, so the two will not match pixel for pixel. Judge the film in the render. |
| `iridescence_ior` | float | `1.3` | 1 to 3 | under "Iridescence"; shown only while `iridescence` is on; Index of refraction of the film itself, not of the surface under it. It sets how strongly the film bends light and so how saturated the interference colours are. |
| `iridescence_thickness_min` | float | `100.0` | 0 to 2000 | under "Iridescence"; shown only while `iridescence` is on; The low end of the film thickness range, in nanometres. It only matters once a thickness map varies the film across the surface; with no map the maximum is used everywhere. |
| `iridescence_thickness_max` | float | `400.0` | 0 to 2000 | under "Iridescence"; shown only while `iridescence` is on; The high end of the film thickness range, in nanometres, and the thickness used everywhere when no thickness map is present. This is the dial that chooses which colours appear: a few hundred nanometres is where visible light interferes, and sweeping it walks the whole rainbow. |
| `specular_intensity` | float | `1.0` | 0 to 1 | under "Specular and anisotropy"; Scales the reflectance a dielectric has when you look straight at it, the value IOR derives. 1 leaves it alone. Lower it to dull a surface's reflections without making it rougher, which roughness alone cannot do. It has no effect on metals, whose reflectance is the base colour. |
| `specular_color` | color | `[1.0,1.0,1.0,1.0]` |  | under "Specular and anisotropy"; Tints that same head-on reflectance. White is untinted and is what almost every real dielectric wants; this exists for the ones that do not, and for matching a reference that was authored with it. |
| `anisotropy` | float | `0.0` | 0 to 1 | under "Specular and anisotropy"; Stretches the specular highlight along one direction instead of leaving it round: brushed metal, hair, the bottom of a saucepan. 0 is isotropic. It follows the surface's tangents, so it needs UVs to point anywhere meaningful. The interactive viewport and a traced render evaluate the stretched highlight by different methods, so the two will not match pixel for pixel. Judge the highlight in the render. |
| `anisotropy_rotation` | float | `0.0` | -360 to 360 | under "Specular and anisotropy"; degrees; shown only while `anisotropy` is on; Turns the stretch direction within the surface, for when the brushing runs across the UVs rather than along them. |
| `transmission` | float | `0.0` | 0 to 1 | How much light passes through the surface instead of scattering back off it: glass, water, a thin plastic. 0 is opaque. This is not the same as alpha, which makes a surface partly absent; transmission keeps the surface and its reflections and lets light through it.

In the interactive viewport it refracts the environment, not the objects behind the surface. Glass reads correctly against an environment and shows nothing of what is behind it. A traced render does carry light through to what is behind, so the two differ by more than pixels here: judge glass in the render. The traced result is itself an approximation, close enough to light a shot by and not a physical prediction. |
| `ior` | float | `1.5` | 1 to 3 | Index of refraction: how strongly the material bends light, and with it how much reflects at a glancing angle. 1.5 is window glass and the default every surface used before this was exposed. Water is about 1.33, diamond about 2.42. It drives reflectance whether or not the surface transmits. |
| `thickness` | float | `0.0` | 0 to 1000 | shown only while `transmission` is on; How far light travels through the interior, in world units. 0 means the surface is thin-walled, a bubble or a pane with no volume behind it. It only matters alongside Attenuation Distance, which is what turns distance travelled into colour. |
| `attenuation_color` | color | `[1.0,1.0,1.0,1.0]` |  | under "Absorption"; shown only while `transmission` is on; The colour light becomes after travelling Attenuation Distance through the interior. White is no tint. This is why thick green glass is green at its edge and clear at its face: the colour is a property of the distance travelled, not of the surface. |
| `attenuation_distance` | float | `0.0` | 0 to 1000 | under "Absorption"; shown only while `transmission` is on; The distance over which light reaches Attenuation Color. Shorter absorbs faster, so a small value tints even thin glass strongly. 0 disables absorption entirely and is the default, which is why a transmissive surface starts out water-clear. |

*Bypassed: emits nothing.*

### Toon <a id="toon"></a>

`toon` · v1 · Shaders · placed inside a material network

Palette search also matches: toon, cel, cartoon, banded.

Cel shading: the surface takes an ordinary base colour, but each light's diffuse contribution is quantized into a fixed number of flat bands instead of falling off smoothly.

Reach for it inside a `matnet` for a cartoon or graphic-novel look. Give it a colour or a map, set the band count, and designate it the network's display node. Fewer bands read as more graphic; the hard edge between them is the whole point.

Only the DIRECT lights are banded. Ambient image-based lighting is still added smoothly on top, and because this node exposes no roughness the ambient term sits at the shader's 0.04 roughness floor, which reads as a sharp environment reflection. A bright HDRI can therefore wash the bands out; turn the environment down if the banding must read. There is no outline either: this node shades, it does not draw contours.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `base_color_map` | in | Image | Optional albedo texture. Connecting it drives the colour entirely and neutralizes Base Color to white; the banding then applies to the sampled colour. Left empty, Base Color is the surface colour. |
| `material` | out | Material | The material this node builds. Wire it into `mix_material`, or designate this node as the network's display node so the enclosing `matnet` publishes it to `material` nodes in Reference mode. Being the default output, a drag from the node's body wires from here. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `base_color` | color | `[0.800000011920929,0.800000011920929,0.800000011920929,1.0]` |  | overridden when `base_color_map` is connected; The colour the bands are cut from: each band is this colour scaled by its step, so a saturated colour gives saturated bands. A connected Base Color Map neutralizes this to white and bands the map instead. |
| `steps` | float | `3.0` | 2 to 8 | How many flat bands each light's diffuse term is quantized into, 2 to 8. 2 is the hardest, most graphic split into lit and unlit; by 8 the steps are close enough together that the cel look mostly disappears. Only the direct lights read it, never the ambient term. |
| `material_name` | text | `` |  | What the material is called wherever it is listed. It has no effect on the shading. Empty falls back to the node type's own name. |

*Bypassed: emits nothing.*

### Unlit <a id="unlit"></a>

`unlit` · v1 · Shaders · placed inside a material network

Palette search also matches: unlit, flat, constant, emission.

Flat colour with no lighting at all: the base colour times the base color map, straight to the screen.

Reach for it inside a `matnet` for surfaces that must read at exactly the colour you typed: reference planes, backdrop cards, UI-like panels, or a texture whose lighting is already baked in. Wire the colour or a map in and designate it the network's display node.

Its semantics come from glTF's `KHR_materials_unlit`. Nothing else in the shading pipeline reaches it: no normal map, no ambient occlusion, no image-based lighting, and it is not darkened by shadows falling on it. It does still CAST shadows, though -- the shadow pass never reads the shading model, so an unlit object occludes light like any other.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `base_color_map` | in | Image | Optional texture, multiplied by Base Color and sent straight to the screen. Connecting it neutralizes Base Color to white. Left empty, Base Color alone is what you see. |
| `material` | out | Material | The material this node builds. Wire it into `mix_material`, or designate this node as the network's display node so the enclosing `matnet` publishes it to `material` nodes in Reference mode. Being the default output, a drag from the node's body wires from here. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `base_color` | color | `[0.800000011920929,0.800000011920929,0.800000011920929,1.0]` |  | overridden when `base_color_map` is connected; The colour the surface renders at, with no lighting applied to it, so it lands on screen as close to what you typed as the tone mapping allows. A connected Base Color Map neutralizes this to white and is shown instead. |
| `material_name` | text | `` |  | What the material is called wherever it is listed. It has no effect on the shading. Empty falls back to the node type's own name. |

*Bypassed: emits nothing.*

## Topology

### Delete <a id="delete"></a>

`delete` · v1 · Topology · placed inside a geo

Palette search also matches: remove, cull, erase, filter.

Removes the triangles the selection picks: in Bounding Box mode the ones whose centroid falls inside the region box, in Normal Direction mode the ones whose face normal points within Angle of Direction. Invert deletes everything the selection did not pick instead.

The debugging knife. Cut a wall away to see inside a model, strip the ground plane off a scan, drop the half of an import you do not need before it reaches a `merge` or an output. It goes anywhere in a modifier chain and it is usually quicker than going back to fix the source file.

Those two predicates are the whole selection model. There are no groups, no primitive ids, and no per-face attributes to select on, so whatever you want gone has to be describable as a region or as a facing. Points left orphaned by the removal are compacted away, a mesh that loses every triangle drops out of the set, and deleting everything is legal: you get empty geometry and a warning, not an error. The predicate sees triangles only: point clouds and polylines pass through untouched with a warning rather than being silently emptied.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `geometry` | in | Geometry | required; The geometry to cull triangles from. |
| `geometry` | out | Geometry | The cooked geometry. Being the default output, a drag from the node's body wires from here, and a bypass passes the input through it. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `mode` | enum (bbox / normal) | `bbox` |  | Select triangles by where they are, or by which way they face. |
| `invert` | bool | `false` |  | Delete everything the selection does NOT cover. |
| `center` | vec3 | `[0.0,0.0,0.0]` |  | meters; shown only while `mode` is `bbox`; The center of the region box. |
| `size` | vec3 | `[1.0,1.0,1.0]` |  | meters; shown only while `mode` is `bbox`; The size of the region box. A triangle goes when its centroid is inside. |
| `direction` | vec3 | `[0.0,1.0,0.0]` |  | shown only while `mode` is `normal`; The facing to match against. |
| `angle` | float | `45.0` | 0 to 180 | degrees; shown only while `mode` is `normal`; How far off the direction a face can point and still be selected. |

*Bypassed: passes `geometry` straight through.*

### Edges to Geo <a id="edges_to_geo"></a>

`edges_to_geo` · v1 · Topology · placed inside a geo

Palette search also matches: wireframe, wire, outline, skeleton, convert.

Extracts every unique edge of the input as a real line segment: a triangle mesh becomes its wireframe, drawn unlit at one pixel, and shared edges appear once rather than once per neighboring triangle.

Unlike the wireframe display overlay, this output is geometry: it survives export, feeds downstream modifiers, and its points carry the source's attributes, so a colored scan's wireframe stays colored. Line inputs pass through with duplicate segments folded; point clouds have no edges and contribute nothing.

A typical inspection chain pairs it with `points_from_geo`: edges show the connectivity, points show the sampling. Materials are dropped in the conversion, and wires are unpickable in the viewport.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `geometry` | in | Geometry | required; The geometry whose edges become line segments. |
| `geometry` | out | Geometry | The cooked geometry. Being the default output, a drag from the node's body wires from here, and a bypass passes the input through it. |

*Bypassed: passes `geometry` straight through.*

### Merge <a id="merge"></a>

`merge` · v2 · Topology · placed inside a geo · gather silhouette

Palette search also matches: combine, join, union, append.

Concatenates every connected geometry input into a single set, in port order. Materials are deduplicated by content as it goes, so four copies of the same red plastic arrive as one table entry rather than four identical ones.

This is the recombine end of a fan-out: a `box` down one branch and a `sphere` down another, each with its own `transform`, merged back into the one set that a `validate` or an output node sees. It takes as many inputs as you wire into it.

Nothing is welded, intersected, or fused geometrically. Two meshes that overlap stay two overlapping meshes and the point count is exactly the sum of the inputs. What order buys you is the mesh order downstream and which input a bypass passes through (the first). Merging nothing is legal: empty geometry and a warning.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `inputs` | in | Geometry | accepts many; The geometry sets to concatenate. Wire as many as you like. Order matters in three ways: it fixes the mesh order of the result, it fixes the order of the deduplicated material table, and a bypassed merge passes through the FIRST connected input. A wire whose upstream has no geometry yet is skipped, and it contributes exactly nothing -- the result is the same as if the wire were not there. Connecting nothing at all is a warning, not an error. |
| `geometry` | out | Geometry | The cooked geometry. Being the default output, a drag from the node's body wires from here, and a bypass passes the input through it. |

*Bypassed: passes `inputs` straight through.*

### Points from Geo <a id="points_from_geo"></a>

`points_from_geo` · v1 · Topology · placed inside a geo

Palette search also matches: vertices, cloud, convert, centers, centroid.

Collapses the input to a Points-topology cloud. Vertices mode keeps every vertex where it is, with normals and UVs lifted into the point attributes and every attribute riding along untouched; Primitive Centers places one point per triangle or segment at its center, averaging the corner attributes into it.

Two jobs, one node: as an inspection lens it strips a surface down to its point structure, showing vertex distribution and density at a glance. As a modeling source it turns any mesh into targets for `copy_to_points` without scattering, so copies land exactly on vertices or face centers rather than randomly.

Points draw unlit at a uniform screen-space size, colored by their `color` attribute when one exists, and materials are dropped in the conversion.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `geometry` | in | Geometry | required; The geometry to collapse into a point cloud. |
| `geometry` | out | Geometry | The cooked geometry. Being the default output, a drag from the node's body wires from here, and a bypass passes the input through it. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `mode` | enum (vertices / primitive_centers) | `vertices` |  | What each output point corresponds to. Vertices keeps every input vertex with its attributes carried verbatim. Primitive Centers places one point at each triangle centroid or segment midpoint, averaging the corner attributes into it. |

*Bypassed: passes `geometry` straight through.*

### Subdivide <a id="subdivide"></a>

`subdivide` · v2 · Topology · placed inside a geo

Palette search also matches: subdivide, smooth, tessellate, refine.

Splits every triangle into four at its edge midpoints, one pass per iteration, interpolating normals, UVs, and any named attributes onto the new points. Neighbouring triangles look their shared edge's midpoint up in an edge map instead of each making its own, so both sides land on the same point and the surface stays crack-free.

Reach for it to buy resolution for something downstream that needs points to work with: a displacement, a noise, a deform. It sits between the source geometry and that node, and it is the answer when the source is an import rather than a primitive whose segment counts you could have raised instead.

The subdivision is linear: it adds points onto the existing surface without moving any of them, so a subdivided box is still a box with the same silhouette and four times the triangles. Nothing here smooths. And the growth compounds hard -- at 5 iterations a 10,000-triangle mesh projects to over 10 million, past the kernel's 8 million ceiling, which is a cook error rather than a stall. Point clouds and polylines have no triangles to split and pass through untouched with a warning.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `geometry` | in | Geometry | required; The geometry to refine. Every mesh in the set is subdivided, and it is the triangle count of the whole set that the output ceiling is measured against. |
| `geometry` | out | Geometry | The cooked geometry. Being the default output, a drag from the node's body wires from here, and a bypass passes the input through it. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `iterations` | int | `1` | 1 to 5 | How many subdivision passes to run. Each pass splits every triangle into four, so the multiplier is 4 to this power: 2 is 16 times the input, 3 is 64, 5 is over a thousand. The cook fails rather than stalls once the result would pass 8 million triangles, which is what the ceiling of 5 exists to keep you away from. |

*Bypassed: passes `geometry` straight through.*

## Transform

### Displace <a id="displace"></a>

`displace` · v1 · Transform · placed inside a geo

Palette search also matches: displacement, deform, push, noise, height, relief.

Moves every point along a direction scaled by an amplitude: the point normal by default, a constant vector, or a vec3 attribute lane, with an optional float lane multiplying the amplitude per point.

This is where attributes start driving shape: `attribute_randomize` a float lane and feed it to Amplitude Attribute for surface noise, or sample an image into a lane with `attribute_from_image` for map-driven relief.

Normals are left as they were (deliberately, so chained displaces compound predictably); chain `compute_normals` after the last displace to relight the result. A mesh without a usable direction source passes through with a warning.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `geometry` | in | Geometry | required; The geometry whose points move. |
| `geometry` | out | Geometry | The cooked geometry. Being the default output, a drag from the node's body wires from here, and a bypass passes the input through it. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `direction` | enum (normal / vector / attribute) | `normal` |  | Where each point's movement direction comes from: the point normal (`N`), one constant vector for every point, or a vec3/vec4 attribute lane by name. |
| `vector` | vec3 | `[0.0,1.0,0.0]` |  | shown only while `direction` is `vector`; The constant direction every point moves along. |
| `direction_attr` | attributeName | `N` |  | shown only while `direction` is `attribute`; The vec3 (or vec4, xyz) point lane supplying each point's direction. |
| `amplitude` | float | `0.1` | -10000 to 10000 | How far each point moves, in metres along its (unit) direction. Negative pushes inward. With an amplitude attribute set, this multiplies the lane's value. |
| `amp_attr` | attributeName | `` |  | Optional float point lane multiplying the amplitude per point. Empty means the constant amplitude alone. This is the driving seat for attribute workflows: randomize a lane, or sample one from an image, and feed it here. |
| `normalize` | bool | `true` |  | Unit-length each direction before scaling, so the amplitude is an honest distance. Off, a longer direction vector moves its point proportionally further. |

*Bypassed: passes `geometry` straight through.*

### Transform <a id="transform"></a>

`transform` · v2 · Transform · placed inside a geo

Palette search also matches: move, rotate, scale, xform.

Moves, rotates, and scales the input, baking the result straight into the point positions. The composition is fixed: scale, then rotation about the pivot, then translation, with normals carried through the inverse transpose so they survive a non-uniform scale.

This is the placement node. It sits between a primitive or an import and a `merge`, and it is what turns three boxes into a blockout instead of a pile at the origin. Nothing is stored as a separate object transform, so chaining two transforms simply composes them and every downstream node reads geometry that has already moved.

Rotate Order and Pivot are the two that catch people out. The angles compose in the order named -- XYZ means Rx * Ry * Rz, so the Z angle turns the geometry first -- and everything except Translate happens about the pivot, which defaults to the world origin rather than to the object's centre.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `geometry` | in | Geometry | required; The geometry to transform. Required: leaving it unwired is a cook error rather than an empty result, because disconnecting a wire is something you meant to do. |
| `geometry` | out | Geometry | The cooked geometry. Being the default output, a drag from the node's body wires from here, and a bypass passes the input through it. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `translate` | vec3 | `[0.0,0.0,0.0]` |  | meters; How far to move the geometry along each axis, in metres. It is applied after the rotation and the scale, and it is the one part of the transform the pivot has no say in. |
| `rotate` | vec3 | `[0.0,0.0,0.0]` |  | degrees; Euler angles in degrees about each axis. The rotation happens about the pivot, and Rotate Order decides how the three angles combine. |
| `rotate_order` | enum (xyz / xzy / yxz / yzx / zxy / zyx) | `xyz` |  | Which order the three Euler angles compose in. The name reads left to right as the matrix product, so XYZ is Rx * Ry * Rz and the Z angle is the one that turns the geometry first. It only changes anything when two or more of the angles are nonzero. |
| `scale` | vec3 | `[1.0,1.0,1.0]` | 0.0001 to 10000 | Per-axis scale factor, applied about the pivot. 1 leaves an axis alone, below 1 shrinks it. Squashing one axis is safe: normals go through the inverse transpose, so they stay perpendicular to the surface instead of shearing off it. |
| `uniform_scale` | float | `1.0` | 0.0001 to 10000 | A single factor multiplied into all three Scale lanes, so the two compound rather than override: Scale (2, 1, 1) at Uniform Scale 3 gives (6, 3, 3). Resize with this and keep Scale for proportions. |
| `pivot` | vec3 | `[0.0,0.0,0.0]` |  | meters; The point the rotation and the scale act about, in the input's own space. The pivot itself does not move under them, so scaling about a corner pins that corner and grows everything away from it. The default (0, 0, 0) is the world origin, which is only the object's centre if the object happens to be sitting there. |

*Bypassed: passes `geometry` straight through.*

## Utility

### Bounds <a id="bounds"></a>

`bounds` · v2 · Utility · placed inside a geo

Palette search also matches: bbox, aabb, extents, measure, center.

Emits the input's axis-aligned bounding box as geometry: a solid box spanning the input's extents, or a single point primitive sitting at its centre. It measures the input and replaces it; the box is the output, not an overlay on the original.

This is the measuring tape. Tap it off a chain to see how big something actually is, where its centre really sits, or whether two parts occupy the space you think they do -- `merge` the bounds with the model it measured and you can eyeball the fit directly. Bypassing it passes the input through, which makes it cheap to leave wired in as an inspection tap.

The box is axis-aligned in object space, so a diagonally oriented model gets a box much larger than the model itself; that is the AABB being honest, not a bug. Center mode's point draws at the renderer's uniform on-screen point size and is not pickable in the viewport (select it on the node canvas). An empty input warns and emits nothing rather than boxing the fallback bounds into a confident unit cube around nothing.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `geometry` | in | Geometry | required; The geometry to measure. Its extents drive the output; the geometry itself does not appear downstream. An empty or unconnected input yields empty geometry, with a warning in the empty case. |
| `geometry` | out | Geometry | The cooked geometry. Being the default output, a drag from the node's body wires from here, and a bypass passes the input through it. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `mode` | enum (box / center) | `box` |  | Box emits the bounding box itself, matching the input's extents on every axis. Center discards the volume and emits a single point primitive at the box's centre, for when the pivot is what you are chasing rather than the extents. |

*Bypassed: passes `geometry` straight through.*

### Note <a id="note"></a>

`note` · v2 · Utility · placed scene or inside a geo or inside a material network or inside a texture network · note silhouette

Palette search also matches: comment, annotation, sticky, label.

A sticky note on the canvas. It has no ports, does not cook, and produces nothing -- it exists to be read by whoever opens the file next, including you in six months.

Use it to leave the reasoning that the graph itself cannot carry: why this branch is bypassed, what the magic number in that param came from, which half of the network is still a work in progress. It is the one node allowed in every network kind, object through texture, because every network eventually needs a comment. Double-click to edit (Esc reverts, Ctrl+Enter or clicking away commits) and drag its corner to resize.

It cannot be bypassed, since there is nothing to switch off, and it is the single node the frontend draws with a bespoke component instead of the standard registry-driven one -- a note is a sticky, not a box with ports. Its edits are ordinary param commands underneath, so notes undo, redo, and save into a `.slxy` like any other node.

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `text` | text | `` |  | The note's body text, the whole point of the node. Edit it in place by double-clicking the note on the canvas rather than through this field. Empty by default: a new note is a blank sticky waiting to be typed into. |
| `color` | color | `[0.9919999837875366,0.9020000100135803,0.5410000085830688,1.0]` |  | The sticky's fill colour, amber by default. The note's corner swatch cycles a set of pastels; this field takes any colour. Nothing but the canvas reads it, so it is free to carry whatever convention you like -- one colour for TODOs, another for warnings to the next person in the file. |
| `width` | float | `160.0` | 120 to 800 | The sticky's width on the canvas, in canvas units, which are screen pixels at 100% zoom. Usually set by dragging the note's corner rather than typed here. Text wraps to it. |
| `height` | float | `80.0` | 60 to 600 | The sticky's height on the canvas, in the same units as Width, and likewise usually dragged rather than typed. It does not grow to fit: text longer than the box is clipped, so size the note to its contents. |
| `text_size` | enum (small / medium / large) | `small` |  | The note text's size on the canvas. Small keeps annotations quieter than the node labels around them and is the default; Medium matches the pre-0.8.0 look; Large is for the one heading a network deserves. Notes saved before this option exists open as Small, the new default. |

*Bypassed: cannot be bypassed.*

### Null <a id="null"></a>

`null` · v1 · Utility · placed inside a geo · terminal silhouette

Palette search also matches: out, output, anchor, passthrough.

Passes its input geometry through untouched. The cook is a refcount bump on the incoming geometry, not a copy, so a null costs effectively nothing however large the model.

A graph wants a no-op for two reasons. The first is a stable reference point: put a null at the end of a subflow, name it OUT, and point the display flag at it. You can then rewire, insert, and delete everything upstream and the flag never moves -- whereas a flag pointed at whichever node happened to be last has to be re-set every time you extend the chain. The second is routing: a null is a reroute, a place to give a long wire a corner and a name.

Nothing about it is inert to the graph, only to the geometry. It is a real node that cooks, appears in the dependency chain, and can be bypassed (bypassing passes the input straight through, which for a null is what it already did).

| Port | Direction | Type | Notes |
|---|---|---|---|
| `geometry` | in | Geometry | required; The geometry to pass through unchanged. Left unconnected the null cooks to empty geometry rather than failing, so an anchor placed before its upstream exists is not an error. |
| `geometry` | out | Geometry | The cooked geometry. Being the default output, a drag from the node's body wires from here, and a bypass passes the input through it. |

*Bypassed: passes `geometry` straight through.*

### Switch <a id="switch"></a>

`switch` · v1 · Utility · placed inside a geo · branch silhouette

Palette search also matches: select, choose, multiplex, if.

Passes exactly one of its variadic inputs through, chosen by Index. Everything else connected to it is ignored, though still cooked -- a switch selects an output, it does not prune the graph behind the branches it did not pick.

It is how a graph carries variants: wire three treatments of a model into one switch and flip between them from a single parameter, or drive it from an expression to make the choice follow something else in the scene. It sits anywhere a `null` would, and reads as one from downstream.

Selection is positional, and this is the part that surprises people. The index addresses the nth connected wire, counting from 0 in wire order -- not the nth wire that produced geometry. A branch that errored, was bypassed to empty, or has not cooked yet still occupies its slot, so it cannot shift the others down and silently change what you selected. Land on such a wire and the switch emits empty geometry and warns; it will not quietly substitute a neighbour. An index past the last wire clamps to it, also with a warning.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `inputs` | in | Geometry | accepts many; The candidate geometries. Order is load-bearing: Index addresses these by position, so reordering the wires changes what a given index selects. Every connected branch cooks whether or not it is the one selected. With nothing connected the switch emits empty geometry and warns. |
| `geometry` | out | Geometry | The cooked geometry. Being the default output, a drag from the node's body wires from here, and a bypass passes the input through it. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `index` | int | `0` | 0 to 255 | Which input wire to pass through, counting from 0 in wire order. Past the last connected wire it clamps to that wire and warns rather than emitting nothing. Landing on a wire whose upstream produced no geometry yields empty, never the neighbouring branch. |

*Bypassed: passes `inputs` straight through.*

### Text <a id="text"></a>

`text` · v1 · Utility · placed scene or inside a geo or inside a material network or inside a texture network · text silhouette

Palette search also matches: text, script, snippet, code, notepad, scratch, datablock.

A named snippet of text or code, stored in the scene.

Somewhere to keep a wrangle program you are reusing, a note to your future self, or a fragment you are still working out. The Text panel lists every snippet in the document and gives them a full editor; this node is where one actually lives.

It computes nothing and has no ports. Solarxy does not run snippets: to use one, paste it into an `attribute_wrangle` or an expression field.

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `body` | snippet | `` |  | The snippet itself. Edited in the Text panel or here; either way it is one parameter on one node, so it saves with the scene and undoes in a single step. |
| `language` | enum (plain / wrangle) | `plain` |  | How the editor presents the snippet. **Wrangle** turns on syntax highlighting, completions and bracket handling; **Plain** leaves the text alone, which is what notes and to-do lists want.

Presentation only. Solarxy does not run a snippet from here: a wrangle snippet is something you paste into an `attribute_wrangle`, and this node stores it rather than executing it. |

*Bypassed: cannot be bypassed.*

### Validate <a id="validate"></a>

`validate` · v2 · Utility · placed inside a geo · analyzer silhouette

Palette search also matches: validate, check, lint, inspect.

Runs the Solarxy validation checks over the input and emits what it finds on the `report` output, while the geometry itself passes through on `geometry` completely unchanged. Each toggle turns on a group of related checks rather than a single one.

It is the only node with two outputs, and that is what makes it droppable into the middle of a chain you have already built: wire it between a modeling branch and an output and the result is identical, you just gain the report. Put one after an import to see what the file arrived with; the fixes usually live in `compute_normals` and `uv_project`.

Nothing here repairs anything, so a reported problem stays reported until you add the node that fixes it. Above 250,000 input triangles the checks move off the cook thread onto a background worker, so on heavy geometry the report lands a moment after the geometry does.

| Port | Direction | Type | Notes |
|---|---|---|---|
| `geometry` | in | Geometry | required; The geometry to validate (passed through unchanged). |
| `geometry` | out | Geometry | the default output; The cooked geometry. Being the default output, a drag from the node's body wires from here, and a bypass passes the input through it. |
| `report` | out | Report | The issues found: what went wrong, how bad it is, and which mesh it belongs to, one row each. Empty when every enabled check passed. |

| Parameter | Type | Default | Range | Notes |
|---|---|---|---|---|
| `normals` | bool | `true` |  | Flags a mesh whose normal count disagrees with its vertex count, and triangles whose stored normals point away from the surface their winding describes (more than about 120 degrees off). Inside-out geometry is the usual cause and `compute_normals` with Flip Orientation is the usual fix. |
| `uvs` | bool | `true` |  | Flags a UV buffer whose length disagrees with the vertex count. Geometry carrying no UVs at all is not flagged by default: that warning normally depends on the source file format expecting them, and cooked geometry has no source format. Turn on Require UVs to flag it anyway. Use `uv_project` to add UVs. |
| `require_uvs` | bool | `false` |  | shown only while `uvs` is on; Off by default. When on, flags any mesh with no texture coordinates at all, regardless of source format, so you can ask whether cooked geometry lacks UVs before texturing or exporting it. Leave it off for procedural geometry that is legitimately UV-less. Only has an effect while UVs is on. |
| `topology` | bool | `true` |  | Flags the structural defects: an empty or non-triangulated index buffer, an index pointing past the end of the vertices, zero-area triangles, edges shared by three or more triangles, and boundary edges. Boundary edges mean an open mesh, which warns here with no way to allow it, so expect them on a plane or a scan. |
| `materials` | bool | `true` |  | Flags a mesh pointing at a material index its set does not have. `merge` already clears such a reference to none as it concatenates, so in practice this catches imports. |
| `budget` | bool | `true` |  | Turns the Budget comparison on. It is on by default but silent until Budget itself is above 0, so switching it off only matters once you have set a number. |
| `triangle_budget` | int | `0` | 0 to 2000000000 | shown only while `budget` is on; How many triangles this geometry is allowed. 0 means no limit, which is why the check says nothing until you set one. Going over is a warning up to 20 percent above the number and an error beyond that. The count is the whole input set, not per mesh. |

*Bypassed: passes `geometry` straight through.*

