//! The single IBL and lighting chokepoint, and the one place a scene's
//! environment reaches it.

use solarxy_core::preferences::{BackgroundMode, CustomBackground, IblMode};
use solarxy_core::scene::{BackgroundKind, SceneDelta, SceneOp};
use solarxy_renderer::environment::{EnvironmentOutcome, EnvironmentTracker, SceneEnvironment};
use solarxy_renderer::frame::Renderer;
use solarxy_renderer::ibl::IblState;
use solarxy_renderer::scene::{
    BackgroundModeExt, create_light_bind_group, create_light_bind_group_selective,
};

use crate::view::HostViewState;

/// The IBL the current mode actually shades with.
#[must_use]
pub fn active_ibl(renderer: &Renderer) -> &IblState {
    match renderer.ibl_res.ibl_mode {
        IblMode::Off => &renderer.ibl_res.ibl_fallback,
        IblMode::Diffuse | IblMode::Full => &renderer.ibl_res.ibl,
    }
}

/// Retarget the skybox at the active IBL's equirect, rebuild the light bind
/// group for the current IBL mode, and push the IBL-derived scalars.
///
/// **The single mutation path for lighting state.** Anything IBL-derived has
/// to ride this function rather than being written where it is computed, or it
/// updates on the next camera-driven frame instead of immediately — and under
/// Lock Lights there may not be a next camera-driven frame at all. Triggered
/// by an HDRI load, an IBL mode toggle, and a background change.
///
/// # On the uniform write
///
/// This writes the whole `LightsUniform`. The desktop shell used to write two
/// partial ranges here (the ambient average, then the environment intensity,
/// which are not contiguous) while the web shell wrote the struct whole. The
/// full write is the superset and is equivalent, because the CPU struct is
/// authoritative at every point in both shells: it is assigned wholesale at
/// construction, in the per-frame rig update, and in the per-pane rig setup,
/// and the two partial writes each mirrored a CPU field assignment at the
/// matching offset. Nothing mutates the GPU copy behind the CPU struct, so
/// widening the write cannot change what the shader reads.
///
/// `env` is not optional. Both shells own their scene environment for the
/// whole session, so there is no state in which the skybox half of this
/// function has to run while the lighting half is skipped.
pub fn rebuild_light_bind_group(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &mut Renderer,
    env: &mut SceneEnvironment,
    hdri_intensity: f32,
) {
    // The skybox pass samples the active IBL's source equirect, so it is
    // retargeted here: an HDRI load or an IBL swap has to keep the visible sky
    // in step with the lighting it came from.
    renderer.skybox_bind_group = renderer.ibl_res.ibl.equirect.as_ref().map(|eq| {
        solarxy_renderer::skybox::create_skybox_bind_group(device, &renderer.layouts.skybox, eq)
    });

    let ibl_avg = active_ibl(renderer).irradiance_average;
    env.light_bind_group = match renderer.ibl_res.ibl_mode {
        IblMode::Off => create_light_bind_group(
            device,
            &renderer.layouts,
            &env.light_buffer,
            &renderer.ibl_res.ibl_fallback,
            &renderer.ibl_res.brdf_lut,
            &renderer.ibl_res.ltc,
        ),
        IblMode::Diffuse => create_light_bind_group_selective(
            device,
            &renderer.layouts,
            &env.light_buffer,
            &renderer.ibl_res.ibl,
            &renderer.ibl_res.ibl_fallback,
            &renderer.ibl_res.brdf_lut,
            &renderer.ibl_res.ltc,
        ),
        IblMode::Full => create_light_bind_group(
            device,
            &renderer.layouts,
            &env.light_buffer,
            &renderer.ibl_res.ibl,
            &renderer.ibl_res.brdf_lut,
            &renderer.ibl_res.ltc,
        ),
    };

    env.lights_uniform.ibl_avg_r = ibl_avg[0];
    env.lights_uniform.ibl_avg_g = ibl_avg[1];
    env.lights_uniform.ibl_avg_b = ibl_avg[2];
    env.lights_uniform.set_ibl_intensity(hdri_intensity);
    queue.write_buffer(
        &env.light_buffer,
        0,
        bytemuck::bytes_of(&env.lights_uniform),
    );
}

/// What applying a scene's environment changed, for the caller to act on.
///
/// Returned rather than written, because the two things a shell does with it
/// are the two things that are genuinely per shell: one holds a traced
/// backend whose environment copy is now stale, and one has to tell a
/// JavaScript frontend that the view moved. Neither belongs in this crate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EnvironmentApplied {
    /// A `SetEnvironment` op was present, so the environment was applied.
    /// False means the delta carried none and nothing was touched.
    pub applied: bool,
    /// The environment actually moved, so a traced backend's copy of it is
    /// stale. Distinct from `applied`: an op that installs the same image at
    /// the same rotation applies and changes nothing.
    pub tracer_dirty: bool,
}

/// Apply a scene delta's environment to the renderer, the scene environment
/// and the view state, then run the lighting chokepoint.
///
/// This was written twice, once per graphical shell, in bodies that differed
/// only in a comment, the order of one assignment, and the two per-shell
/// reactions that are now the return value. It is one function as of 0.10.0.
///
/// `custom_backgrounds` is the shell's user-defined background registry, used
/// only when the environment clears and the fallback sky has to be resolved.
/// The browser has none and passes an empty slice; when the desktop's are
/// retired the parameter goes with them.
///
/// # Why the headless renderer does not call this
///
/// `crate::headless` has its own environment application and keeps it. It has
/// no view state to write rotation and intensity into, no panes whose
/// background follows the scene, and it renders once rather than continuously,
/// so "the last op wins and there is no dedupe" is correct there and wrong
/// here. Sharing them would mean a parameter that is `None` on one caller and
/// the whole point on the other.
pub fn apply_scene_environment(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &mut Renderer,
    env: &mut SceneEnvironment,
    tracker: &mut EnvironmentTracker,
    view: &mut HostViewState,
    custom_backgrounds: &[CustomBackground],
    delta: &SceneDelta,
) -> EnvironmentApplied {
    let mut result = EnvironmentApplied::default();

    for op in &delta.ops {
        let SceneOp::SetEnvironment {
            hdri,
            rotation,
            intensity,
            background,
        } = op
        else {
            continue;
        };
        result.applied = true;

        // Rotation and intensity write through to the display settings the
        // shell's own sliders read, so the node and the sliders show one
        // value rather than fighting over two.
        view.display.hdri_rotation = *rotation;
        view.display.hdri_intensity = *intensity;

        match tracker.apply(device, queue, &mut renderer.ibl_res, hdri.as_ref()) {
            // Fall through to the rebuild anyway: rotation or intensity may
            // have moved even when the image did not.
            EnvironmentOutcome::Unchanged => {}
            EnvironmentOutcome::HdriInstalled => {
                result.tracer_dirty = true;
                if *background == BackgroundKind::HdriSky {
                    view.pane_settings[0].background_mode = BackgroundMode::HDRI_SKY;
                }
            }
            // "No environment" is not "a black environment": fall back to the
            // procedural sky the pane's own background derives, which is what
            // clearing the environment by hand does on either shell.
            EnvironmentOutcome::Cleared => {
                result.tracer_dirty = true;
                let (top, bottom) = view.pane_settings[0]
                    .background_mode
                    .resolve(custom_backgrounds)
                    .sky_colors();
                renderer.ibl_res.ibl = IblState::from_sky_colors(device, queue, top, bottom);
            }
        }

        rebuild_light_bind_group(device, queue, renderer, env, view.display.hdri_intensity);
    }

    result
}
