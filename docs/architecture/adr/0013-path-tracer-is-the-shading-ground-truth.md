# 0013. The path tracer is the shading ground truth and the rasterizer approximates it

- **Status**: accepted
- **Date**: 2026-09-07

## Context

Solarxy shades the same material through two independent implementations: the raster path in
`crates/solarxy-renderer/src/shaders/shader.wgsl` and the path tracer's BSDF in
`crates/solarxy-renderer/src/shaders/pathtrace/bsdf.wgsl`. One authored material feeds both.

They disagree, and not only in the ways two renderers are expected to.

`thickness` is a Beer-Lambert path length in the rasterizer, where
`crates/solarxy-renderer/src/shaders/shader.wgsl:911` computes attenuation as
`through *= exp(-sigma * max(material.thickness, 0.0))` and the parameter genuinely is the
distance light travelled. In the tracer it is a boolean: `bsdf.wgsl:665` reads
`surf.thin_film = m.thickness == 0.0` and the absorption distance comes from the ray's own
chord. One parameter, one help string, two incompatible physical meanings.

`emissive_strength` is filled into the raster material uniform at
`crates/solarxy-renderer/src/material.rs:129`, declared in the WGSL struct at
`shader.wgsl:179`, and read by no raster shader anywhere. The tracer honours it, as do the
glTF importer, the exporter and the node parameter. A scene authored with an emissive
multiplier can therefore differ between viewport and render by the full value of that
multiplier.

There are more. The tracer's texture atlas is `Rgba8Unorm` with a single mip level and
filters in encoded sRGB before decoding the blended result, while the raster path uses sRGB
texture views so the hardware decodes each texel before the bilinear blend and filters a full
mip chain. The tracer derives a per-triangle tangent from the triangle's UVs at every hit
rather than carrying a vertex tangent.

Without a stated authority, each of these is arguable in both directions, so none of them
gets fixed.

## Options considered

### Option A: the path tracer is ground truth

The tracer defines correct shading. The rasterizer is a real-time approximation of it, and
every divergence is either a listed and justified approximation or a defect against the
rasterizer.

### Option B: the rasterizer is authoritative

The interactive viewport defines the look, and the tracer must match what the artist saw
while authoring.

Appealing for a tool whose primary surface is a viewport. But it means the physically correct
implementation is the one that has to introduce error, and every future correctness
improvement to the tracer becomes a regression against the reference.

### Option C: two deliberate products

Preview and final-quality render are simply allowed to differ, with the permitted divergence
set written down.

This is honest about the present state but it gives up the property that makes a renderer
trustworthy: that the picture converges on something defensible. It also makes every new
divergence a documentation task rather than a bug, which is how the current situation arose.

## Decision

The path tracer is the shading ground truth. The rasterizer is an approximation of it.

A difference between the two is one of exactly two things, and it must be labelled as one of
them: a deliberate real-time approximation, listed in
[06b-rendering-and-shading.md](../06b-rendering-and-shading.md) with the reason it is
acceptable, or a defect filed against the rasterizer.

There is one material contract. A parameter has one meaning, stated once, and both
implementations honour that meaning or are wrong.

## Consequences

The three divergences above become defects rather than curiosities. `thickness` needs one
meaning chosen and the other implementation corrected. `emissive_strength` needs the raster
shader to read it. The atlas filtering difference needs either a mip chain and linear
filtering in the tracer or an explicit entry in the approximation list.

This makes real-time work harder in a specific way: an approximation can no longer be
introduced quietly. It has to be named, justified, and written down, which is a real cost per
change and the entire point.

It also settles a question that would otherwise recur every time the tracer improves.
Improving the tracer can no longer be a regression, because the tracer is the reference.

Enforcement: nothing mechanical today. `crates/solarxy-renderer/tests/pathtrace_bsdf.rs` and
`pathtrace_light.rs` test the tracer against analytic expectations, and the golden-capture job
holds the rasterizer against its own past, but nothing compares the two to each other. An
agreement test on a shared scene is proposed in
[06b](../06b-rendering-and-shading.md) as part of the quality bar.
