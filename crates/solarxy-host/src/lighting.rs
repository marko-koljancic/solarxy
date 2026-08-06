//! The single IBL and lighting chokepoint.

use solarxy_core::preferences::IblMode;
use solarxy_renderer::environment::SceneEnvironment;
use solarxy_renderer::frame::Renderer;
use solarxy_renderer::ibl::IblState;
use solarxy_renderer::scene::{create_light_bind_group, create_light_bind_group_selective};

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
