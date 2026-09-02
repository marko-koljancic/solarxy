//! The tracer configured the way the render node asks for.
//!
//! Outside the wasm cfg for the reason `camera_commit` is: the host that calls
//! it is wasm-only, and a mapping nothing can run natively is a mapping nothing
//! checks. It cannot move to `solarxy-host` either, which has no
//! `solarxy-graph` dependency by design, and it cannot move to the engine,
//! which must not see the renderer. So each of the three shells carries its
//! own, and each one carries the guard below.

use solarxy_graph::nodes::RenderSettings;
use solarxy_renderer::pathtrace::backend::TraceSettings;
use solarxy_renderer::pathtrace::denoise::DenoiseSettings;

/// What a still render asks the tracer for, from what the node says.
///
/// Destructured exhaustively on purpose, and that is what this function is for
/// rather than a style choice: a value added to `RenderSettings` stops this
/// compiling until this shell says what happens to it. Earlier in this release
/// the camera's aperture resolved correctly out of a document and then reached
/// no renderer at all, for the whole release, while every test stayed green
/// because they all used an aperture of zero. A test catches a value wired to
/// the wrong field; only the compiler catches one wired nowhere.
///
/// The pane preview is not this. It keeps its own settings, at its own sample
/// target and its own denoise preference, because a preview and a delivered
/// frame want different answers.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) fn trace_settings_for(
    settings: &RenderSettings,
    current: TraceSettings,
) -> TraceSettings {
    let RenderSettings {
        // The shot itself: the still spec's business and the job camera's.
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
        // The four that steer the filter are configured through their own
        // setter rather than here. See `denoise_settings_for` below.
        denoise_strength: _,
        denoise_sigma_color: _,
        denoise_normal_power: _,
        denoise_sigma_albedo: _,
        denoise_level_falloff: _,
        // The film back reaches the tracer too: its kernel is what withholds
        // the environment from uncovered camera rays and counts the matte.
        transparent_background,
        // What leaves beside the picture stays the still spec's business.
        aov_albedo: _,
        aov_normal: _,
        aov_depth: _,
    } = *settings;
    TraceSettings {
        samples: samples.max(1),
        // One sample per animation frame. The pacing that keeps the page
        // responsive, and the bound on how large any one dispatch is. It is
        // this shell's own decision rather than the node's, which is why it is
        // not read from the settings above.
        chunk: 1,
        bounces,
        transmissive_bounces,
        firefly_clamp,
        seed,
        denoise,
        denoise_until_samples,
        // Explicit rather than riding `current`, for the same reason as the
        // sample count: the still authors it and the pane preview's own
        // settings put it back to false on the preview's next encode.
        transparent_background,
        // Everything else carries over from what the backend already holds,
        // because this shell shares one tracer between the pane preview and the
        // still. The lens is installed right after this, from the camera the
        // shot names.
        //
        // That includes the preview's resolution scale, which is inert here
        // rather than inherited by mistake: the scale is applied only on the
        // whole-pane path, and a still is always a window on an image whose
        // size was authored. Read that before setting it: a still that reset it
        // would be reasserting the one thing it is already immune to, and the
        // preview reasserts its own on its next encode anyway.
        ..current
    }
}

/// How the render node asks for the filter to be steered.
///
/// Strength multiplies the colour tolerance rather than being a fifth
/// independent number, because that tolerance is the value that most changes
/// the outcome: any other expression of it would leave the advanced controls
/// holding a value the everyday one could contradict.
///
/// The pane preview does not go through this. It leaves the filter at the
/// measured defaults, because its denoise preference is a different setting for
/// a different purpose and entangling the two would make a still's tuning
/// silently reshape the viewport.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) fn denoise_settings_for(settings: &RenderSettings) -> DenoiseSettings {
    DenoiseSettings {
        sigma_color: settings.denoise_sigma_color * settings.denoise_strength,
        normal_power: settings.denoise_normal_power,
        sigma_albedo: settings.denoise_sigma_albedo,
        level_falloff: settings.denoise_level_falloff,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every value the render node authors for the walk reaches the tracer.
    ///
    /// The values are deliberately nothing like the defaults, because a test
    /// written with the defaults passes with the assignment deleted. That is
    /// the failure this exists for.
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

        let t = trace_settings_for(&s, TraceSettings::default());
        assert_eq!(t.samples, 91);
        assert_eq!(t.bounces, 13);
        assert_eq!(t.transmissive_bounces, 7);
        assert!((t.firefly_clamp - 3.5).abs() < f32::EPSILON);
        assert_eq!(t.seed, 4242);
        assert!(t.denoise);
        assert!(t.transparent_background, "the film back reaches the kernel");
    }

    /// The page's pacing is the page's, whatever the node says.
    #[test]
    fn the_still_paces_one_sample_a_frame() {
        let mut s = RenderSettings::defaults();
        s.samples = 1024;
        assert_eq!(trace_settings_for(&s, TraceSettings::default()).chunk, 1);
    }

    /// What the pane preview left behind does not decide a still.
    ///
    /// The two share one backend on this shell, so every value a still cares
    /// about has to be stated rather than inherited. The preview runs at a
    /// different sample target, a different chunk and its own denoise
    /// preference, and none of them survive into the render.
    #[test]
    fn a_still_states_its_own_terms_rather_than_the_previews() {
        let preview = TraceSettings {
            samples: 32,
            chunk: 4,
            denoise: true,
            bounces: 2,
            transmissive_bounces: 1,
            firefly_clamp: 99.0,
            seed: 7,
            resolution_scale: 0.5,
            ..TraceSettings::default()
        };
        let d = RenderSettings::defaults();
        let t = trace_settings_for(&d, preview);

        assert_eq!(t.samples, d.samples);
        assert_eq!(t.chunk, 1);
        assert_eq!(t.denoise, d.denoise);
        assert_eq!(t.bounces, d.bounces);
        assert_eq!(t.transmissive_bounces, d.transmissive_bounces);
        assert!((t.firefly_clamp - d.firefly_clamp).abs() < f32::EPSILON);
        assert_eq!(t.seed, d.seed);
        // The one exception, and it is deliberate: the scale is applied only
        // on the whole-pane path and a still is always a window on an image
        // whose size was authored, so carrying it costs nothing.
        assert!((t.resolution_scale - 0.5).abs() < f32::EPSILON);
    }

    /// A render of no samples still draws one, rather than looping on nothing.
    #[test]
    fn a_render_of_no_samples_still_draws_one() {
        let mut s = RenderSettings::defaults();
        s.samples = 0;
        assert_eq!(trace_settings_for(&s, TraceSettings::default()).samples, 1);
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
        let t = trace_settings_for(&s, TraceSettings::default());
        assert_eq!(t.denoise_until_samples, 40);
        assert!(t.filtering_at(40));
        assert!(!t.filtering_at(41));
    }
}
