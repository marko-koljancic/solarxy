//! Bringing a renderer up with no window, which every consumer that is not a
//! shell has had to do for itself.
//!
//! # What is shared and what is not
//!
//! The adapter and device request is **not** here, deliberately. It is fifteen
//! lines, and it is the one part callers genuinely differ on: a golden capture
//! wants a specific backend set, an endurance run wants high performance, a
//! test wants whatever the machine has. Sharing it would mean a parameter per
//! difference and a caller reading the parameters back out.
//!
//! What is shared is everything after: the fabricated surface configuration a
//! renderer sizes its targets and composite against, the renderer's own
//! initialization, and the model-independent scene half beside it. That is the
//! part that is identical everywhere, that has been copied five times, and that
//! silently changes meaning when the renderer grows a field.
//!
//! # The fabricated configuration is not a lie the caller has to maintain
//!
//! There is no surface, so nothing presents. The configuration exists because
//! [`Renderer::new`] reads its format and dimensions, and those two facts are
//! real: the format is what the caller will read pixels back as, and the
//! dimensions are a starting size that the first `resize_targets` replaces.
//! Everything else in it is inert.
//!
//! # Multisampling is not a preference here
//!
//! Four samples, matching both shells. A one-sample renderer takes a different
//! path through the resolve, so a headless render at one sample would not be
//! rendering what a shell renders, which is the entire point of rendering
//! headlessly.

use solarxy_core::aabb::AABB;
use solarxy_core::preferences::{BackgroundMode, IblMode, LineWeight, ToneMode};
use solarxy_core::scene::{BackgroundKind, SceneDelta, SceneOp};
use solarxy_renderer::environment::{SceneEnvironment, placeholder_bounds};
use solarxy_renderer::frame::{Renderer, RendererInit};
use solarxy_renderer::ibl::IblState;
use solarxy_renderer::scene::BackgroundModeExt;
use solarxy_renderer::visualization::VisualizationState;

/// The checker the UV modes sample. Compiled in rather than loaded, because a
/// headless caller has no asset directory and the renderer requires one.
const UV_CHECKER_PNG: &[u8] = include_bytes!("../../../res/textures/uv-checker_1k.png");

/// What a scene asked for through [`SceneOp::SetEnvironment`], after the
/// image it named has been installed.
///
/// Returned rather than acted on in full, because a caller has two decisions
/// left that this cannot make for it: the constant sky a path tracer falls
/// back to where there is no image, and whether the visible backdrop is the
/// image itself.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EnvironmentRequest {
    /// Whether an image is now live in the host's image-based lighting.
    pub image: bool,
    /// The backdrop the scene asked to be shot against. Meaningless without
    /// an image, exactly as [`BackgroundKind::HdriSky`] documents.
    pub background: BackgroundKind,
    /// Yaw on both the visible sky and the lighting, in radians.
    pub rotation: f32,
    /// Multiplier on the lighting contribution.
    pub intensity: f32,
}

impl Default for EnvironmentRequest {
    /// What a scene that authors no environment asks for: nothing, at the
    /// intensity that leaves an authored image alone.
    fn default() -> Self {
        Self {
            image: false,
            background: BackgroundKind::Keep,
            rotation: 0.0,
            intensity: 1.0,
        }
    }
}

/// A renderer and the scene state beside it, with no window and no surface.
///
/// The device and queue stay with the caller, who created them and usually
/// needs them for its own encoders. This owns only what it built.
pub struct HeadlessHost {
    pub renderer: Renderer,
    pub env: SceneEnvironment,
    /// The scene's extent, seeded to the shared placeholder. A caller that
    /// loads real geometry replaces it; until then it is what the camera frames
    /// and what the visualization sizes itself against.
    pub bounds: AABB,
    /// The format pixels come back as. Handed in rather than chosen, because
    /// the caller reads the bytes and has to agree with itself about them.
    pub format: wgpu::TextureFormat,
}

impl HeadlessHost {
    /// Builds the renderer and the scene environment against a fabricated
    /// surface configuration.
    ///
    /// `width` and `height` are a starting size only. Anything that renders
    /// tiles resizes the targets per tile anyway, so passing the final image
    /// size buys nothing and passing a huge one allocates twice.
    ///
    /// Post-processing starts off. Screen-space ambient occlusion is wrong over
    /// a traced image and bloom is a look decision a caller makes, so neither
    /// is switched on by something whose job is only to exist.
    ///
    /// # Errors
    /// Whatever [`Renderer::new`] fails with. Nothing else here can fail: the
    /// environment, the visualization and the bounds are all infallible once a
    /// renderer exists.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
    ) -> Result<Self, solarxy_renderer::error::RendererError> {
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width.max(1),
            height: height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: wgpu::CompositeAlphaMode::Opaque,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        let background = BackgroundMode::GRADIENT.resolve(&[]);
        let (sky_top, sky_bottom) = background.sky_colors();
        let init = RendererInit {
            msaa_sample_count: 4,
            gradient_top: [0.35, 0.41, 0.47, 1.0],
            gradient_bottom: [0.66, 0.70, 0.72, 1.0],
            sky_top,
            sky_bottom,
            wireframe_color: background.wireframe_color(),
            wireframe_line_width: LineWeight::Medium.width_px(),
            bloom_enabled: false,
            ssao_enabled: false,
            tone_mode: ToneMode::AcesFilmic,
            exposure: 1.0,
            ibl_mode: IblMode::Full,
            uv_checker_png: UV_CHECKER_PNG,
        };
        let renderer = Renderer::new(device, queue, &config, &init)?;

        let bounds = placeholder_bounds();
        let vis = VisualizationState::new_from_parts(
            device,
            &renderer.layouts,
            &bounds,
            &[],
            None,
            background.grid_color(),
        );
        // The construction order below is preserved from the original
        // extraction and is golden-verified. Do not reorder it.
        let env = SceneEnvironment::new(
            device,
            queue,
            &renderer.layouts,
            &bounds,
            1.0,
            &renderer.ibl_res.brdf_lut,
            &renderer.ibl_res.ltc,
            1024,
            vis,
        );

        Ok(Self {
            renderer,
            env,
            bounds,
            format,
        })
    }

    /// Installs the scene's lighting environment out of the delta: the third
    /// surface's half of what both graphical shells already do with this op.
    ///
    /// A render is a property of the scene, so its environment comes from the
    /// document rather than from a constant the caller picks. Without this the
    /// image-based lighting stays at whatever the renderer was built with, and
    /// a scene lit by a sunset renders as though lit by a grey room, on both
    /// engines: image-based lighting is the raster path's main ambient term,
    /// and a tracer with no environment integrates against its fallback sky.
    ///
    /// The last op wins, matching the op's own "replace the whole environment"
    /// contract. There is no dedupe against what is already installed, because
    /// unlike a shell this applies a delta once and then renders.
    ///
    /// Clearing is a real answer and leaves the image-based lighting alone:
    /// "no environment" is not "a black environment", and what stands in for
    /// the absence is the caller's decision, not this one's.
    pub fn apply_scene_environment(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        delta: &SceneDelta,
    ) -> EnvironmentRequest {
        let mut request = EnvironmentRequest::default();
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
            request.rotation = *rotation;
            request.intensity = *intensity;
            request.background = *background;
            request.image = false;
            if let Some(image) = hdri {
                self.renderer.ibl_res.ibl = IblState::from_hdr_image(device, queue, image);
                request.image = true;
            }
        }
        if request.image {
            // The lighting chokepoint, which also retargets the skybox at the
            // equirect that was just installed. Running it only on an install
            // keeps a scene that authored nothing byte-identical to one that
            // never reached here.
            crate::rebuild_light_bind_group(
                device,
                queue,
                &mut self.renderer,
                &mut self.env,
                request.intensity,
            );
        }
        request
    }
}
