//! What a document says a render should be.
//!
//! A pure function of the document, with no device and no shell state, which
//! is what lets the parity check at the bottom of this file run on every
//! `cargo test`. The other half of the cross-shell question, whether identical
//! settings become identical pixels, needs a real adapter on three surfaces
//! and is a written check instead.

use solarxy_graph::document::{GraphContext, NodeId};
use solarxy_graph::nodes::RenderSettings;
use solarxy_renderer::pathtrace::backend::TraceSettings;
use solarxy_renderer::pathtrace::denoise::DenoiseSettings;

/// A throwaway engine holding the open model as the one-node document the
/// terminal renders through: companions collected and staged, the document
/// synthesized, and the cook driven to quiescence, all before the job
/// starts. Returns the staging warnings for the shell to surface.
///
/// Bytes are re-read from the model's path rather than reconstructed from
/// the GPU-side scene, which is simpler and less surprising: the import
/// cooks the same input the terminal would read.
pub(super) fn engine_for_model(
    path_str: &str,
) -> Result<(Box<solarxy_graph::Engine>, Vec<String>), String> {
    use solarxy_graph::model_document;

    let path = std::path::Path::new(path_str);
    let bytes =
        std::fs::read(path).map_err(|e| format!("Couldn't read {}: {e}", path.display()))?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("model")
        .to_string();

    let mut engine = solarxy_graph::Engine::new().map_err(|e| e.to_string())?;
    let companions =
        solarxy_formats::companions::collect(path, &ext, &bytes).map_err(|e| e.to_string())?;
    let warnings = companions.warnings;
    for asset in companions.assets {
        engine.stage_asset(asset.name, String::new(), asset.bytes);
    }
    model_document::synthesize_model_document(&mut engine, &name, &ext, bytes)
        .map_err(|e| e.to_string())?;
    model_document::cook_to_quiescence(&mut engine, &mut || false, &mut |_, _| {})
        .map_err(|e| e.to_string())?;
    Ok((Box::new(engine), warnings))
}

/// The render node's settings, or the defaults when the scene has none.
///
/// The selection rule is the headless command's: zero render nodes means
/// the defaults with a note, one means that one, and several is a refusal
/// because a still renders exactly one and picking silently would render
/// the wrong one convincingly.
pub(super) fn resolve_still_settings(
    engine: &solarxy_graph::Engine,
) -> Result<(RenderSettings, Option<String>), String> {
    let graph = engine
        .document()
        .graph(GraphContext::Root)
        .map_err(|e| e.to_string())?;
    let render_nodes: Vec<NodeId> = graph
        .nodes()
        .filter(|n| n.type_id == "render")
        .map(|n| n.id)
        .collect();
    match render_nodes.len() {
        0 => Ok((
            default_still_settings(),
            Some("The scene has no render node; rendering at the defaults".to_owned()),
        )),
        1 => engine
            .render_settings(GraphContext::Root, render_nodes[0])
            .map(|s| (s, None)),
        n => Err(format!(
            "The scene has {n} render nodes; the still renders exactly one"
        )),
    }
}

/// What a document with no render node renders at.
///
/// The node type's own defaults, read from its descriptor rather than
/// restated, so the desktop and the headless command answer with one set
/// of values by construction. Both carried a hand-written literal until
/// the render node's third version, kept in step only by a test comparing
/// fields one at a time.
pub(super) fn default_still_settings() -> RenderSettings {
    RenderSettings::defaults()
}

/// The tracer configured the way the render node asks for.
///
/// Destructured exhaustively on purpose, and that is the point of the
/// function rather than a style choice: a value added to `RenderSettings`
/// stops this compiling until this shell says what happens to it. The
/// alternative is what the camera's aperture did earlier in this release,
/// where a value resolved correctly out of the document and then reached no
/// renderer at all, and every test passed because they all used an aperture
/// of zero. A test cannot catch that on its own; it can only catch a value
/// wired to the wrong place.
///
/// The three shells cannot share this. `solarxy-host` is where shared host
/// behaviour goes and it deliberately has no `solarxy-graph` dependency,
/// while the engine must not see the renderer, so the boundary forbids a
/// common home in both directions.
pub(super) fn trace_settings_for(settings: &RenderSettings) -> TraceSettings {
    let RenderSettings {
        // The shot itself: read by the still spec and the job's camera, not
        // by the tracer.
        camera: _,
        width: _,
        height: _,
        engine: _,
        samples,
        bounces,
        transmissive_bounces,
        firefly_clamp,
        seed,
        denoise,
        denoise_until_samples,
        // The four that steer the filter travel by their own setter rather
        // than on here, because the filter is configured apart from the walk.
        // See `denoise_settings_for` below.
        denoise_strength: _,
        denoise_sigma_color: _,
        denoise_normal_power: _,
        denoise_sigma_albedo: _,
        denoise_level_falloff: _,
        // The film back reaches the tracer too: its kernel is what withholds
        // the environment from uncovered camera rays and counts the matte.
        transparent_background,
        // What leaves beside the picture is the still spec's business, not
        // the tracer's.
        aov_albedo: _,
        aov_normal: _,
        aov_depth: _,
    } = *settings;
    let samples = samples.max(1);
    TraceSettings {
        samples,
        // The browser paces at one sample per animation frame; native has no
        // frame to pace against, so a larger chunk is the same work in fewer
        // submissions.
        chunk: 8.min(samples),
        bounces,
        transmissive_bounces,
        firefly_clamp,
        seed,
        denoise,
        denoise_until_samples,
        transparent_background,
        // The lens is set immediately after this, by `set_lens`. Everything
        // else keeps the tracer's own default.
        ..TraceSettings::default()
    }
}

/// How the render node asks for the filter to be steered.
///
/// Strength multiplies the colour tolerance rather than being a fifth
/// independent number, because that tolerance is the value that most changes
/// the outcome: expressing it any other way would leave the advanced heading
/// holding a value the everyday control could contradict.
pub(super) fn denoise_settings_for(settings: &RenderSettings) -> DenoiseSettings {
    DenoiseSettings {
        sigma_color: settings.denoise_sigma_color * settings.denoise_strength,
        normal_power: settings.denoise_normal_power,
        sigma_albedo: settings.denoise_sigma_albedo,
        level_falloff: settings.denoise_level_falloff,
    }
}

#[cfg(test)]
mod tests {
    use solarxy_graph::nodes::RenderEngine;

    use super::*;

    /// Every value the render node authors for the walk reaches the tracer.
    ///
    /// The values are deliberately nothing like the defaults. A test written
    /// with the defaults passes with the assignment deleted, which is exactly
    /// how the camera's aperture reached no renderer for a whole release
    /// while every test stayed green.
    #[test]
    fn every_authored_value_reaches_the_tracer() {
        let mut s = RenderSettings::defaults();
        s.samples = 91;
        s.bounces = 13;
        s.transmissive_bounces = 7;
        s.firefly_clamp = 3.5;
        s.seed = 4242;
        s.denoise = true;
        s.transparent_background = true;

        let t = trace_settings_for(&s);
        assert_eq!(t.samples, 91);
        assert_eq!(t.bounces, 13);
        assert_eq!(t.transmissive_bounces, 7);
        assert!((t.firefly_clamp - 3.5).abs() < f32::EPSILON);
        assert_eq!(t.seed, 4242);
        assert!(t.denoise);
        assert!(t.transparent_background, "the film back reaches the kernel");
        // This shell's own pacing rather than the node's: eight samples to a
        // submission, native having no frame to pace against.
        assert_eq!(t.chunk, 8);
    }

    /// The four steering values reach the filter, and strength multiplies the
    /// one it is documented to multiply.
    ///
    /// Deliberately distinct values, so a field wired to its neighbour fails
    /// rather than passing on a coincidence.
    #[test]
    fn the_steering_values_reach_the_filter() {
        let mut s = RenderSettings::defaults();
        s.denoise_sigma_color = 2.0;
        s.denoise_normal_power = 33.0;
        s.denoise_sigma_albedo = 0.5;
        s.denoise_level_falloff = 3.0;
        s.denoise_strength = 1.5;

        let d = denoise_settings_for(&s);
        assert!(
            (d.sigma_color - 3.0).abs() < f32::EPSILON,
            "strength multiplies the colour tolerance: 2.0 at 1.5 is 3.0"
        );
        assert!((d.normal_power - 33.0).abs() < f32::EPSILON);
        assert!((d.sigma_albedo - 0.5).abs() < f32::EPSILON);
        assert!((d.level_falloff - 3.0).abs() < f32::EPSILON);
    }

    /// At the default strength the filter runs at exactly its measured values.
    ///
    /// The multiplier is what makes this worth asserting: a strength that
    /// defaulted to anything but one would silently retune every existing
    /// render the moment the control shipped.
    #[test]
    fn the_defaults_are_the_measured_values_untouched() {
        let d = denoise_settings_for(&RenderSettings::defaults());
        assert_eq!(d, DenoiseSettings::default());
    }

    /// The threshold reaches the walk's settings, where the gate reads it.
    #[test]
    fn the_denoise_threshold_reaches_the_tracer() {
        let mut s = RenderSettings::defaults();
        s.denoise = true;
        s.denoise_until_samples = 40;
        let t = trace_settings_for(&s);
        assert_eq!(t.denoise_until_samples, 40);
        assert!(t.filtering_at(40));
        assert!(!t.filtering_at(41));
    }

    /// A render shorter than a chunk submits the render, not the chunk.
    #[test]
    fn the_chunk_never_outruns_the_render() {
        let mut s = RenderSettings::defaults();
        s.samples = 3;
        assert_eq!(trace_settings_for(&s).chunk, 3);

        s.samples = 0;
        let t = trace_settings_for(&s);
        assert_eq!(
            (t.samples, t.chunk),
            (1, 1),
            "a render of no samples still draws one, rather than looping on a \
             chunk of zero"
        );
    }

    #[test]
    fn defaults_match_the_headless_command() {
        // The headless command's `default_settings` is private to its
        // crate, so the agreement is pinned by value: a change there
        // must land here too, deliberately.
        let d = default_still_settings();
        assert_eq!((d.width, d.height), (1920, 1080));
        assert_eq!(d.engine, RenderEngine::Raster);
        assert_eq!((d.samples, d.bounces, d.transmissive_bounces), (64, 6, 4));
        assert!(!d.denoise);
        assert!(d.camera.is_none());
    }
}

/// The desktop and the headless command read one document the same way.
///
/// **This is the automatable half of the cross-shell parity question, and it is
/// worth being precise about which half.** Three shells rendering "the same
/// image" can fail in two independent places: they can read different settings
/// out of one scene, or they can turn identical settings into different pixels.
/// The first is a pure function of the document and is checked here on every
/// `cargo test`, with no device. The second needs a real adapter on each of
/// three surfaces, one of which is a browser, and lives in
/// `Docs/qa/render-checklist.md` as a written check with named scenes.
///
/// The settings half is also the half that actually drifts. Pixels differ by
/// float reassociation, which this release already ruled on and bounded; a
/// shell reading a different sample count out of the same node is a defect that
/// no tolerance excuses.
#[cfg(test)]
mod parity {
    use super::*;

    /// Both shipped samples, which between them carry the four features a
    /// render can diverge on: transmission and an area light in the first, a
    /// procedural texture network and a graded camera in the second.
    const SCENES: [&str; 2] = ["cornell-box.slxy", "procedural-lookdev.slxy"];

    fn sample(name: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../web/public/samples")
            .join(name)
    }

    #[test]
    fn both_native_shells_resolve_one_scene_the_same_way() {
        for name in SCENES {
            let path = sample(name);
            assert!(path.exists(), "the shipped sample {name} is missing");

            let loaded = solarxy_render::input::load(&path, None, &mut solarxy_render::Silent)
                .unwrap_or_else(|e| panic!("{name} did not load: {e}"));

            // The headless command's answer, and this shell's own, over one
            // cooked document.
            let mut warnings = Vec::new();
            let headless = solarxy_render::resolve_settings(
                &loaded.engine,
                &solarxy_render::RenderOptions::default(),
                &mut warnings,
            )
            .unwrap_or_else(|e| panic!("{name} would not resolve headlessly: {e}"));

            let (desktop, _note) = resolve_still_settings(&loaded.engine)
                .unwrap_or_else(|e| panic!("{name} would not resolve on the desktop: {e}"));

            // Field by field rather than by equality, so a failure names what
            // disagreed instead of printing two structs and leaving the reader
            // to diff them.
            assert_eq!(desktop.width, headless.width, "{name}: width");
            assert_eq!(desktop.height, headless.height, "{name}: height");
            assert_eq!(desktop.engine, headless.engine, "{name}: engine");
            assert_eq!(desktop.samples, headless.samples, "{name}: samples");
            assert_eq!(desktop.bounces, headless.bounces, "{name}: bounces");
            assert_eq!(
                desktop.transmissive_bounces, headless.transmissive_bounces,
                "{name}: transmissive bounces"
            );
            assert_eq!(desktop.denoise, headless.denoise, "{name}: denoise");
            assert_eq!(desktop.seed, headless.seed, "{name}: seed");
            // Bit patterns, because this is a parity assertion: both shells resolve
            // the field from one document, so anything short of identical is the
            // divergence the test exists to catch. The values are printed rather
            // than the bits so a failure is still readable.
            assert!(
                desktop.firefly_clamp.to_bits() == headless.firefly_clamp.to_bits(),
                "{name}: firefly clamp {} vs {}",
                desktop.firefly_clamp,
                headless.firefly_clamp
            );
            assert_eq!(desktop.camera, headless.camera, "{name}: camera");
            assert_eq!(
                desktop.aov_albedo, headless.aov_albedo,
                "{name}: albedo pass"
            );
            assert_eq!(
                desktop.aov_normal, headless.aov_normal,
                "{name}: normal pass"
            );
            assert_eq!(desktop.aov_depth, headless.aov_depth, "{name}: depth pass");
            assert_eq!(
                desktop.transparent_background, headless.transparent_background,
                "{name}: transparent background"
            );
        }
    }

    /// A scene with no render node falls to the node type's own descriptor on
    /// both shells, rather than to two hand-written literals that agreed until
    /// somebody changed one.
    #[test]
    fn a_scene_with_no_render_node_falls_to_one_set_of_defaults() {
        assert_eq!(default_still_settings(), RenderSettings::defaults());
    }
}
