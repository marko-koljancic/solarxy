use egui_wgpu::ScreenDescriptor;

pub struct EguiRenderer {
    ctx: egui::Context,
    winit_state: egui_winit::State,
    renderer: egui_wgpu::Renderer,
}

impl EguiRenderer {
    pub fn new(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
        window: &winit::window::Window,
    ) -> Self {
        let ctx = egui::Context::default();
        let viewport_id = ctx.viewport_id();
        let winit_state =
            egui_winit::State::new(ctx.clone(), viewport_id, window, None, None, None);
        let renderer = egui_wgpu::Renderer::new(
            device,
            surface_format,
            egui_wgpu::RendererOptions::default(),
        );

        Self {
            ctx,
            winit_state,
            renderer,
        }
    }

    /// Forward a winit event to egui. Returns `true` if egui consumed the event.
    pub fn on_window_event(
        &mut self,
        window: &winit::window::Window,
        event: &winit::event::WindowEvent,
    ) -> bool {
        self.winit_state.on_window_event(window, event).consumed
    }

    /// Returns true if egui wants exclusive pointer input (hovering a panel, etc.).
    pub fn wants_pointer_input(&self) -> bool {
        self.ctx.wants_pointer_input()
    }

    /// Returns true if egui wants exclusive keyboard input (typing in a text field, etc.).
    pub fn wants_keyboard_input(&self) -> bool {
        self.ctx.wants_keyboard_input()
    }

    /// Run the egui frame, tessellate, and render into the given surface view.
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        window: &winit::window::Window,
        view: &wgpu::TextureView,
        screen: ScreenDescriptor,
    ) {
        let raw_input = self.winit_state.take_egui_input(window);
        let full_output = self.ctx.run(raw_input, |_ctx| {
            // Empty UI — future issues will add panels here
        });

        self.winit_state
            .handle_platform_output(window, full_output.platform_output);

        let tris = self
            .ctx
            .tessellate(full_output.shapes, full_output.pixels_per_point);

        for (id, image_delta) in &full_output.textures_delta.set {
            self.renderer
                .update_texture(device, queue, *id, image_delta);
        }
        self.renderer
            .update_buffers(device, queue, encoder, &tris, &screen);

        let mut pass = encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui render pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            })
            .forget_lifetime();
        self.renderer.render(&mut pass, &tris, &screen);
        drop(pass);

        for id in &full_output.textures_delta.free {
            self.renderer.free_texture(id);
        }
    }
}
