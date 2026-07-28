//! UV overlap GPU readback: thin delegation to the shared state machine
//! in `solarxy_renderer::frame::UvOverlapResources` (the arm/poll logic
//! moved there so the web host runs the identical code).

use super::{State, UvOverlapResources};

impl State {
    pub(super) fn poll_overlap_stats(&mut self) {
        self.renderer.uv_overlap.poll_readback(&self.device);
    }
}

pub(super) fn request_overlap_readback_impl(
    device: &wgpu::Device,
    uv_overlap: &mut UvOverlapResources,
    encoder: &mut wgpu::CommandEncoder,
) {
    uv_overlap.request_readback(device, encoder);
}
