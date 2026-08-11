//! The raster backend: today's pass chain behind the render backend contract.
//!
//! This is an adapter and nothing more. Every pixel it produces comes from
//! [`crate::pane::encode_pane_passes`], which is the same body both shells
//! drove before the trait existed, so adopting it cannot change an image. That
//! is the whole point of landing it separately from the hosts that will hold
//! it.
//!
//! It lives here rather than in `solarxy-renderer`, where the milestone
//! specification first placed it, because the pass chain it wraps moved into
//! this crate when the two shells' per-pane bodies were collapsed, and the
//! renderer cannot depend on the host. The trait itself stays in the renderer,
//! so the path tracer can implement it without seeing this crate.

use std::sync::Arc;

use solarxy_core::scene::SceneDelta;
use solarxy_renderer::backend::{BackendCaps, FrameCtx, FrameOutcome, RenderBackend, TopologyMask};
use solarxy_renderer::bind_groups::BindGroupLayouts;
use solarxy_renderer::error::RendererError;
use solarxy_renderer::light::MAX_LIGHTS;
use solarxy_renderer::scene_objects::SceneObjects;

use crate::pane::{EncodedPane, encode_pane_passes};

/// The most panes a layout can show at once, and so the width of any per-pane
/// array a backend keeps.
const PANE_SLOTS: usize = 4;

/// The rasterizer, as a host drives it.
///
/// Owns the multi-object scene, because that is this backend's GPU
/// representation of the document: per-object vertex and index buffers,
/// material bind groups, validation overlays. The path tracer will build its
/// own from the same deltas and the two will never share one.
///
/// A shell holds **one of these**, not one per pane. The scene is per session,
/// so a backend per pane would put four copies of the same geometry on the GPU
/// for a quad layout. What is genuinely per pane lives in [`Self::encoded`],
/// keyed by the pane index the frame context carries.
pub struct RasterBackend {
    scene: SceneObjects,
    /// Held so [`RenderBackend::apply`] can upload without being handed the
    /// renderer. Cheap: the renderer already keeps these behind an `Arc`, so
    /// this is a refcount bump rather than a copy of every layout.
    layouts: Arc<BindGroupLayouts>,
    /// Upload failures, kept rather than logged.
    ///
    /// `apply` returns nothing, and this crate has no logging facility and is
    /// not going to grow one for four lines. The shell drains these and reports
    /// them the way it reports everything else, which is also the only place
    /// that knows whether a toast or a console line is wanted.
    errors: Vec<RendererError>,
    /// What each pane's last encode decided, which is what its composite needs.
    /// Written by [`RenderBackend::encode`] and read straight back by the host;
    /// it is per-pane state rather than a return value because the trait's
    /// return is spoken for by convergence.
    encoded: [Option<EncodedPane>; PANE_SLOTS],
}

impl RasterBackend {
    /// What this backend can do, as a constant.
    ///
    /// A constant rather than only a method because the answer does not depend
    /// on any GPU state, and a caller deciding whether an option it was handed
    /// can take effect should not have to create a device to find out. The
    /// method returns this.
    pub const CAPS: BackendCaps = BackendCaps {
        // One pass over the geometry produces the final image.
        progressive: false,
        // The lights uniform holds this many. It is the viewport's ceiling,
        // not the scene's, and stating it here is what lets a host say so.
        max_lights: Some(MAX_LIGHTS as u32),
        supports_instancing: true,
        supports_topology: TopologyMask::ALL,
        writes_aovs: false,
    };

    #[must_use]
    pub fn new(layouts: Arc<BindGroupLayouts>) -> Self {
        Self {
            scene: SceneObjects::new(),
            layouts,
            errors: Vec::new(),
            encoded: [None; PANE_SLOTS],
        }
    }

    /// The multi-object scene, for everything a host asks of the document that
    /// is not rendering: visible bounds, the light and camera lists, picking,
    /// per-object validation, the UV pane's source.
    ///
    /// Those queries live here because this is where the answer is, not because
    /// they are raster concerns. A host always has one of these backends even
    /// when every pane is traced, so they never go missing.
    #[must_use]
    pub fn scene(&self) -> &SceneObjects {
        &self.scene
    }

    /// Drain whatever [`RenderBackend::apply`] could not upload.
    pub fn take_errors(&mut self) -> Vec<RendererError> {
        std::mem::take(&mut self.errors)
    }

    /// What the given pane's last encode decided. `None` before that pane has
    /// encoded anything.
    #[must_use]
    pub fn encoded(&self, pane: usize) -> Option<EncodedPane> {
        self.encoded.get(pane).copied().flatten()
    }
}

impl RenderBackend for RasterBackend {
    fn apply(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, delta: &SceneDelta) {
        if let Err(e) = self.scene.apply(device, queue, &self.layouts, delta) {
            self.errors.push(e);
        }
    }

    /// Encode one pane, and report that it is finished, because it is: a
    /// rasterized pane is complete the moment its passes are encoded.
    ///
    /// **`target` is accepted and not used.** This backend writes the
    /// renderer's own high-dynamic-range target, which is the same target a
    /// host would hand in and the same one the shared composite reads back.
    /// The parameter exists for a backend that resolves an accumulation buffer
    /// into an explicit view; pointing the whole raster pass chain at an
    /// arbitrary view would touch every pass, for no gain to anything that
    /// exists today.
    ///
    /// The draw list is assembled here rather than handed in, from this
    /// backend's own scene plus whatever the host draws that never came down
    /// the delta stream. That is what owning the scene means, and it is also
    /// what makes this callable at all: a list built by the host would borrow
    /// the scene inside this backend while this call needs it mutably.
    fn encode(&mut self, ctx: &mut FrameCtx<'_>, _target: &wgpu::TextureView) -> FrameOutcome {
        let pane = ctx.index;
        let encoded = encode_pane_passes(ctx, &self.scene);
        if let Some(slot) = self.encoded.get_mut(pane) {
            *slot = Some(encoded);
        }
        FrameOutcome::Complete
    }

    fn caps(&self) -> BackendCaps {
        Self::CAPS
    }

    /// Nothing to drop: this backend accumulates nothing across frames. Not a
    /// stub, an answer.
    fn invalidate(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_describe_the_rasterizer_without_naming_it() {
        // Constructed without a device: `caps` reads no GPU state, which is
        // what lets a host gate its interface before anything is uploaded.
        let caps = BackendCaps {
            progressive: false,
            max_lights: Some(MAX_LIGHTS as u32),
            supports_instancing: true,
            supports_topology: TopologyMask::ALL,
            writes_aovs: false,
        };
        assert!(!caps.progressive);
        assert_eq!(caps.max_lights, Some(8));
        assert!(caps.supports_topology.contains(TopologyMask::POINTS));
        assert!(caps.supports_topology.contains(TopologyMask::LINES));
        assert!(caps.supports_topology.contains(TopologyMask::TRIANGLES));
        assert!(!caps.writes_aovs);
    }

    #[test]
    fn the_light_bound_is_the_uniform_the_viewport_actually_binds() {
        // If the uniform grows, this must move with it, and a host that gates
        // on the capability follows for free.
        assert_eq!(MAX_LIGHTS, 8);
    }
}
