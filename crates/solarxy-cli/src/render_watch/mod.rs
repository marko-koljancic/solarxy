//! The live window: a render, on screen, as it converges.
//!
//! # Why it owns its own device
//!
//! The feature specification said the window would share the device the
//! renderer already created. It cannot, cheaply: that device is a local of the
//! render, requested with no compatible surface, and sharing it means threading
//! a window's surface into a library signature so a headless render can be
//! watched. What this does instead is take the picture the render already hands
//! every sink and upload it, which needs a device of its own and needs nothing
//! at all from the render's.
//!
//! What that costs is a second adapter and a second device on one machine, for
//! a textured quad. What it buys is that the render library gained a callback
//! and nothing else, and that a window failing to open is a warning rather than
//! a render that would not start.
//!
//! # Why the loop is pumped rather than run
//!
//! `winit` wants to own the process's loop, and the render already does. So the
//! window is advanced with `pump_app_events` at a zero timeout from inside the
//! sink, which is the same shape the dashboard uses on the terminal: the render
//! calls the surface, not the other way round. On macOS that pump has to happen
//! on the main thread, which is where a command-line render runs, and is the
//! reason this cannot later move to a worker without a redesign.
//!
//! The same shape bounds how fluid the chrome can be: a drag or a wheel is
//! answered at the next pump, and pumps happen when the render reports.
//! While samples are landing that is often; between them the picture holds
//! still, and after the render finishes the hold loop pumps on its own clock
//! and the window is as responsive as any other.

mod view;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use solarxy_render::{AovKind, Preview, PreviewFormat, RenderProgress, RenderSink};
use winit::application::ApplicationHandler;
use winit::event::{MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::platform::pump_events::EventLoopExtPumpEvents;
use winit::window::{Window, WindowId};

use solarxy_render::{PassKind, PassSelector};
use view::{Rect, ViewTransform};

/// The longest edge the window opens at.
///
/// A still may be eight thousand pixels an edge and a screen is not. The window
/// is for watching a render arrive, so it takes the picture's proportions and a
/// size that fits somewhere, and the file is where the pixels are.
const MAX_EDGE: u32 = 1280;

/// How long the window is given to settle before the first frame.
///
/// Only used while waiting for the platform to hand over a window, which it
/// does on its own schedule rather than when asked.
const SETTLE: std::time::Duration = std::time::Duration::from_millis(16);

/// The picture, as the window holds it.
struct Frame {
    width: u32,
    height: u32,
    /// Eight bits a channel, in the order a texture takes them.
    rgba: Vec<u8>,
    /// Whether the render is a float one, whose preview here is clamped and
    /// encoded while the file keeps the real values. The chrome says so.
    float: bool,
}

/// The planes as they last arrived, kept raw so switching passes replays
/// them from here rather than asking the render for anything.
///
/// Only what the run requested is ever present: an unrequested plane is
/// `None` on the preview itself, so holding these costs nothing a run did
/// not ask to produce.
struct Latest {
    width: u32,
    height: u32,
    color: Vec<u8>,
    format: PreviewFormat,
    aux: Option<Vec<u8>>,
    depth: Option<Vec<u8>>,
}

impl Latest {
    fn from_preview(image: &Preview<'_>) -> Self {
        Self {
            width: image.width,
            height: image.height,
            color: image.pixels.to_vec(),
            format: image.format,
            aux: image.aux.map(<[u8]>::to_vec),
            depth: image.depth.map(<[u8]>::to_vec),
        }
    }
}

/// The beauty, as display bytes.
///
/// A float image is clamped and encoded, which for a scene-referred one is
/// the whole of the tone mapping. Said plainly in the reference: a float
/// render is judged from its file, and this is a preview.
///
/// Shared with the browser's still dialog, which shows a float render the
/// same way for the same reason. Two display transforms would make the two
/// surfaces disagree about a render neither is authoritative about.
fn beauty_rgba8(latest: &Latest) -> (Vec<u8>, bool) {
    match latest.format {
        PreviewFormat::Rgba8 => (latest.color.clone(), false),
        PreviewFormat::Rgba32F => (solarxy_render::float_to_rgba8(&latest.color), true),
    }
}

/// The selected pass as the picture the window shows.
///
/// A pass whose plane is somehow absent falls back to the beauty rather
/// than to a blank: the selector should never offer one, and if it did the
/// honest recovery is the picture that always exists.
fn frame_for(pass: PassKind, latest: &Latest) -> Frame {
    let (rgba, float) = match pass {
        PassKind::Beauty => beauty_rgba8(latest),
        PassKind::Albedo => match latest.aux.as_ref() {
            Some(aux) => (solarxy_render::albedo_rgba8(aux), false),
            None => beauty_rgba8(latest),
        },
        PassKind::Normal => match latest.aux.as_ref() {
            Some(aux) => (solarxy_render::normal_rgba8(aux), false),
            None => beauty_rgba8(latest),
        },
        PassKind::Depth => match latest.depth.as_ref() {
            Some(depth) => (solarxy_render::depth_rgba8(depth), false),
            None => beauty_rgba8(latest),
        },
    };
    Frame {
        width: latest.width,
        height: latest.height,
        rgba,
        float,
    }
}

/// The window size a picture wants: its own proportions, bounded.
fn window_size(width: u32, height: u32) -> (u32, u32) {
    let (w, h) = (width.max(1), height.max(1));
    if w <= MAX_EDGE && h <= MAX_EDGE {
        return (w, h);
    }
    if w >= h {
        (MAX_EDGE, (MAX_EDGE * h / w).max(1))
    } else {
        ((MAX_EDGE * w / h).max(1), MAX_EDGE)
    }
}

/// What one paint hands the device beyond the picture: the chrome's shapes,
/// where they go, and the textures they brought with them.
type ChromeDraw<'a> = (
    &'a [egui::ClippedPrimitive],
    &'a egui_wgpu::ScreenDescriptor,
    &'a egui::TexturesDelta,
);

/// What is needed to put a picture on a window, once there is one.
struct Gpu {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// The image rectangle and the window size, as the shader reads them.
    view_uniform: wgpu::Buffer,
    /// The uploaded picture and its binding, rebuilt when the size changes.
    texture: Option<(wgpu::Texture, wgpu::BindGroup, u32, u32)>,
    /// The chrome's painter, and the non-sRGB view format it writes through,
    /// the same arrangement the desktop shell uses for the same reason: egui
    /// encodes its own colours, so handing it the sRGB view would encode
    /// them twice.
    chrome: egui_wgpu::Renderer,
    chrome_format: wgpu::TextureFormat,
}

impl Gpu {
    /// Bring a device up against this window's surface.
    ///
    /// Takes the window by reference and clones the handle for the surface,
    /// which needs an owned one to borrow the window for as long as it lives.
    fn new(window: &Arc<Window>) -> Result<Self, String> {
        let size = window.inner_size();
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let surface = instance
            .create_surface(Arc::clone(window))
            .map_err(|e| format!("no surface: {e}"))?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .map_err(|_| "no adapter for the window".to_owned())?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("solarxy-watch"),
            required_features: wgpu::Features::empty(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            // The same floor the render itself asks for. A quad needs less, and
            // asking for less here than there would let the window open on a
            // machine that cannot run the render it is watching.
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        }))
        .map_err(|e| format!("no device for the window: {e}"))?;

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .or_else(|| caps.formats.first().copied())
            .ok_or_else(|| "the surface offers no format".to_owned())?;
        let chrome_format = format.remove_srgb_suffix();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![chrome_format],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let (pipeline, layout, sampler) = blit_pipeline(&device, format);
        let view_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("watch view"),
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let chrome = egui_wgpu::Renderer::new(
            &device,
            chrome_format,
            egui_wgpu::RendererOptions::default(),
        );

        Ok(Self {
            surface,
            device,
            queue,
            config,
            pipeline,
            layout,
            sampler,
            view_uniform,
            texture: None,
            chrome,
            chrome_format,
        })
    }
}

/// The pipeline that puts a picture on a target, and the three things it binds.
///
/// Free of any window, so the same shader and the same layout a window uses can
/// be built against a plain texture and read back. That is the only way to
/// check the two things in this file that are easy to get wrong and impossible
/// to notice from a screenshot description: whether the picture arrives the
/// right way up, and whether the canvas stays out of it.
fn blit_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
) -> (wgpu::RenderPipeline, wgpu::BindGroupLayout, wgpu::Sampler) {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("watch blit"),
        source: wgpu::ShaderSource::Wgsl(include_str!("render_watch.wgsl").into()),
    });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("watch blit"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("watch blit"),
        bind_group_layouts: &[&layout],
        push_constant_ranges: &[],
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("watch blit"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(format.into())],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    });
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("watch blit"),
        // Nearest, not linear: a preview scaled up should show what the render
        // produced rather than a smoothed guess at it, and at the moment a
        // tile lands the honest picture is the blocky one.
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    (pipeline, layout, sampler)
}

/// The `View` uniform's bytes: the image rectangle, then the window size.
///
/// Packed by hand because this crate carries no byte-cast dependency for the
/// sake of eight floats, on the little-endian layout every shipped target
/// shares.
fn view_uniform_bytes(rect: Rect, window: (u32, u32)) -> [u8; 32] {
    let values: [f32; 8] = [
        rect.x,
        rect.y,
        rect.w,
        rect.h,
        window.0 as f32,
        window.1 as f32,
        0.0,
        0.0,
    ];
    let mut bytes = [0u8; 32];
    for (slot, value) in values.iter().enumerate() {
        bytes[slot * 4..slot * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}

impl Gpu {
    fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    /// Put a picture on the device, reusing the texture when the size holds.
    fn upload(&mut self, frame: &Frame) {
        let wanted = (frame.width.max(1), frame.height.max(1));
        let fresh = !matches!(self.texture, Some((_, _, w, h)) if (w, h) == wanted);
        if fresh {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("watch picture"),
                size: wgpu::Extent3d {
                    width: wanted.0,
                    height: wanted.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                // The picture is display-referred already, so the view is the
                // sRGB one and the shader does no encoding of its own.
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("watch picture"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: self.view_uniform.as_entire_binding(),
                    },
                ],
            });
            self.texture = Some((texture, bind, wanted.0, wanted.1));
        }
        let Some((texture, _, _, _)) = self.texture.as_ref() else {
            return;
        };
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &frame.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(wanted.0 * 4),
                rows_per_image: Some(wanted.1),
            },
            wgpu::Extent3d {
                width: wanted.0,
                height: wanted.1,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Draw whatever is uploaded into its rectangle, the canvas around it,
    /// and the chrome on top.
    fn draw(&mut self, rect: Rect, chrome: Option<ChromeDraw<'_>>) {
        let Some((_, bind, _, _)) = self.texture.as_ref() else {
            return;
        };
        let Ok(surface) = self.surface.get_current_texture() else {
            // A surface that is not ready is not an error here: the next tile
            // will ask again, and a render must not stop because a window did.
            return;
        };
        self.queue.write_buffer(
            &self.view_uniform,
            0,
            &view_uniform_bytes(rect, (self.config.width.max(1), self.config.height.max(1))),
        );
        let view = surface
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("watch blit"),
            });
        if let Some((shapes, screen, delta)) = chrome {
            for (id, image_delta) in &delta.set {
                self.chrome
                    .update_texture(&self.device, &self.queue, *id, image_delta);
            }
            self.chrome
                .update_buffers(&self.device, &self.queue, &mut encoder, shapes, screen);
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("watch blit"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, bind, &[]);
            pass.draw(0..3, 0..1);
        }
        if let Some((shapes, screen, _)) = chrome {
            let chrome_view = surface.texture.create_view(&wgpu::TextureViewDescriptor {
                format: Some(self.chrome_format),
                ..Default::default()
            });
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("watch chrome"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &chrome_view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                })
                .forget_lifetime();
            self.chrome.render(&mut pass, shapes, screen);
            drop(pass);
        }
        self.queue.submit(Some(encoder.finish()));
        surface.present();
        if let Some((_, _, delta)) = chrome {
            for id in &delta.free {
                self.chrome.free_texture(id);
            }
        }
    }
}

/// The chrome's two halves: the context that lays it out and the winit
/// bridge that feeds it events.
struct Chrome {
    ctx: egui::Context,
    state: egui_winit::State,
}

/// The window, and everything it does in response to the platform.
struct WatchApp {
    window: Option<Arc<Window>>,
    gpu: Option<Gpu>,
    chrome: Option<Chrome>,
    frame: Option<Frame>,
    /// The raw planes the last preview carried, replayed on a pass switch.
    latest: Option<Latest>,
    /// Which passes this run can show, and which one the reader chose.
    selector: PassSelector,
    /// The pass the uploaded picture was converted from, so a switch or a
    /// fresh preview is noticed and anything else is not re-uploaded.
    shown: Option<PassKind>,
    /// Where the picture sits: the fit until the reader pans or zooms.
    view: ViewTransform,
    /// The pointer, in window pixels, as the zoom anchor and the drag origin.
    cursor: (f32, f32),
    /// The cursor position a drag last panned from, while a button is down.
    drag: Option<(f32, f32)>,
    /// Set when an event moved something visible, so the pump repaints once
    /// rather than every handler painting on its own.
    wants_paint: bool,
    /// The size the window should open at, once there is a picture to size it
    /// by. Before the first tile there is nothing to be the shape of.
    wanted: Option<(u32, u32)>,
    /// Set when the reader closed the window or pressed escape.
    dismissed: bool,
    /// Said once. A window that cannot open should say so and then be quiet.
    complained: bool,
}

/// Escape or `q`, the two ways a reader cancels from the window. Read before
/// the chrome sees the key: stopping a render must never depend on which
/// widget has focus.
fn cancel_key(event: &winit::event::KeyEvent) -> bool {
    event.state.is_pressed()
        && matches!(
            event.logical_key,
            Key::Named(NamedKey::Escape) | Key::Character(_)
        )
        && matches!(
            event.logical_key.to_text(),
            None | Some("q") | Some("Q") | Some("\u{1b}")
        )
}

/// `f`, the way back to the letterbox fit.
fn fit_key(event: &winit::event::KeyEvent) -> bool {
    event.state.is_pressed() && matches!(event.logical_key.to_text(), Some("f") | Some("F"))
}

impl ApplicationHandler for WatchApp {
    fn resumed(&mut self, active: &ActiveEventLoop) {
        self.open(active);
    }

    /// Every turn of the pump, which is where the window is actually opened.
    ///
    /// `resumed` fires once, at the first pump, and the first pump happens
    /// while the render is still loading: there is no picture yet, so there is
    /// nothing to size a window by and nothing is created. Waiting for
    /// `resumed` to come round again waits forever, because it does not. This
    /// runs on every pump instead, and does nothing until there is a shape to
    /// open at.
    fn about_to_wait(&mut self, active: &ActiveEventLoop) {
        self.open(active);
    }

    fn window_event(&mut self, _active: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        // Closing and cancelling are the window's own contract, honoured
        // before the chrome is offered anything.
        match &event {
            WindowEvent::CloseRequested => {
                self.dismissed = true;
                return;
            }
            WindowEvent::KeyboardInput { event: key, .. } if cancel_key(key) => {
                self.dismissed = true;
                return;
            }
            _ => {}
        }
        // The chrome sees everything else first; what it consumes it keeps.
        let consumed = match (self.chrome.as_mut(), self.window.as_ref()) {
            (Some(chrome), Some(window)) => {
                let response = chrome.state.on_window_event(window, &event);
                self.wants_paint |= response.repaint;
                response.consumed
            }
            _ => false,
        };
        let sizes = self.sizes();
        match event {
            WindowEvent::Resized(size) => {
                if let Some(gpu) = self.gpu.as_mut() {
                    gpu.resize(size.width, size.height);
                }
                // Resizing refits, which is the contract, and also what keeps
                // a half-dragged view from surviving into a new window shape
                // it was never composed against.
                self.view.reset();
                self.wants_paint = true;
            }
            WindowEvent::KeyboardInput { event: key, .. } => {
                if !consumed && fit_key(&key) {
                    self.view.reset();
                    self.wants_paint = true;
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let here = (position.x as f32, position.y as f32);
                if let Some(from) = self.drag {
                    if consumed {
                        // A drag that wandered onto the chrome ends rather
                        // than fighting it over the pointer.
                        self.drag = None;
                    } else if let Some((pic, win)) = sizes {
                        self.view.pan((here.0 - from.0, here.1 - from.1), pic, win);
                        self.drag = Some(here);
                        self.wants_paint = true;
                    }
                }
                self.cursor = here;
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left | MouseButton::Middle,
                ..
            } => {
                if state.is_pressed() {
                    if !consumed {
                        self.drag = Some(self.cursor);
                    }
                } else {
                    // A release ends a drag wherever it lands, chrome
                    // included, or a button let go over a widget would leave
                    // the picture glued to the pointer.
                    self.drag = None;
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if !consumed && let Some((pic, win)) = sizes {
                    let factor = match delta {
                        MouseScrollDelta::LineDelta(_, y) => 1.2f32.powf(y),
                        MouseScrollDelta::PixelDelta(p) => (p.y as f32 / 200.0).exp(),
                    };
                    self.view.zoom_about(self.cursor, factor, pic, win);
                    self.wants_paint = true;
                }
            }
            WindowEvent::RedrawRequested => self.paint(),
            _ => {}
        }
    }
}

impl WatchApp {
    /// Open the window, once there is a picture to be the shape of.
    fn open(&mut self, active: &ActiveEventLoop) {
        if self.window.is_some() || self.complained {
            return;
        }
        let Some((width, height)) = self.wanted else {
            return;
        };
        let attributes = Window::default_attributes()
            .with_title("Solarxy render")
            .with_inner_size(winit::dpi::PhysicalSize::new(width, height));
        match active.create_window(attributes) {
            Ok(window) => {
                let window = Arc::new(window);
                match Gpu::new(&window) {
                    Ok(gpu) => {
                        let ctx = egui::Context::default();
                        let state = egui_winit::State::new(
                            ctx.clone(),
                            egui::ViewportId::ROOT,
                            &window,
                            None,
                            None,
                            None,
                        );
                        self.chrome = Some(Chrome { ctx, state });
                        self.gpu = Some(gpu);
                        self.window = Some(window);
                    }
                    // Said once and then left alone: a machine with no display
                    // would otherwise say it on every tile.
                    Err(why) => self.complain(&why),
                }
            }
            Err(e) => self.complain(&format!("no window: {e}")),
        }
    }

    fn complain(&mut self, why: &str) {
        if !self.complained {
            eprintln!("the render window could not open: {why}. The render continues.");
            self.complained = true;
        }
    }

    /// The picture and window sizes, once both exist.
    fn sizes(&self) -> Option<((u32, u32), (u32, u32))> {
        let frame = self.frame.as_ref()?;
        let gpu = self.gpu.as_ref()?;
        Some((
            (frame.width, frame.height),
            (gpu.config.width.max(1), gpu.config.height.max(1)),
        ))
    }

    /// Convert and upload the selected pass, when what is shown is stale.
    ///
    /// Stale means a fresh preview arrived or the reader switched passes;
    /// everything else finds the uploaded picture current and does nothing,
    /// which is what keeps one pass per tile the whole of the upload cost.
    fn sync_frame(&mut self) {
        let wanted = self.selector.selected();
        if self.shown == Some(wanted) {
            return;
        }
        let Some(latest) = self.latest.as_ref() else {
            return;
        };
        let Some(gpu) = self.gpu.as_mut() else {
            return;
        };
        let frame = frame_for(wanted, latest);
        gpu.upload(&frame);
        self.frame = Some(frame);
        self.shown = Some(wanted);
    }

    fn paint(&mut self) {
        let Some(window) = self.window.clone() else {
            return;
        };
        let (pic, float) = match self.frame.as_ref() {
            Some(f) => ((f.width, f.height), f.float),
            None => return,
        };
        let win = match self.gpu.as_ref() {
            Some(g) => (g.config.width.max(1), g.config.height.max(1)),
            None => return,
        };

        let chrome_out = if let Some(chrome) = self.chrome.as_mut() {
            let input = chrome.state.take_egui_input(&window);
            let zoom_percent = self.view.rect(pic, win).w / pic.0.max(1) as f32 * 100.0;
            let mut fit = false;
            let selector = &mut self.selector;
            let out = chrome.ctx.run(input, |ctx| {
                chrome_ui(ctx, selector, &mut fit, zoom_percent, float);
            });
            chrome
                .state
                .handle_platform_output(&window, out.platform_output);
            let shapes = chrome.ctx.tessellate(out.shapes, out.pixels_per_point);
            if fit {
                self.view.reset();
            }
            Some((shapes, out.textures_delta))
        } else {
            None
        };

        // The chrome may have switched passes this very frame; showing the
        // old one for even a tile would make the selector feel broken.
        self.sync_frame();

        let rect = self.view.rect(pic, win);
        let pixels_per_point = window.scale_factor() as f32;
        let Some(gpu) = self.gpu.as_mut() else {
            return;
        };
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [win.0, win.1],
            pixels_per_point,
        };
        match chrome_out.as_ref() {
            Some((shapes, delta)) => gpu.draw(rect, Some((shapes.as_slice(), &screen, delta))),
            None => gpu.draw(rect, None),
        }
    }
}

/// The closed pass dropdown's width. Explicit so the control cannot size
/// itself to whichever label is selected: every pass name fits, and the
/// strip holds still across selection and engine discovery.
const PASS_SELECTOR_WIDTH: f32 = 96.0;

/// The overlay strip: the pass selector, the way back to the fit, the zoom,
/// and the caveat a float preview carries.
fn chrome_ui(
    ctx: &egui::Context,
    selector: &mut PassSelector,
    fit: &mut bool,
    zoom_percent: f32,
    float: bool,
) {
    egui::TopBottomPanel::top("watch chrome")
        .frame(
            egui::Frame::new()
                .fill(egui::Color32::from_black_alpha(160))
                .inner_margin(egui::Margin::same(6)),
        )
        .show_separator_line(false)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                // The selector: one dropdown of fixed width, whatever is
                // selected and whatever the run turns out to offer, so the
                // strip's geometry never moves under a click or under the
                // engine becoming known mid-run. The popup lists the beauty
                // alone under raster (no other pass could exist), and under
                // tracing an unrequested pass is a disabled row naming the
                // flag that would have produced it, in the label rather
                // than behind a hover. Selection lands the same frame as
                // the click, which `paint` relies on.
                egui::ComboBox::from_id_salt("watch pass")
                    .selected_text(selector.selected().label())
                    .width(PASS_SELECTOR_WIDTH)
                    .show_ui(ui, |ui| {
                        for kind in PassKind::ALL {
                            if selector.available(kind) {
                                let chosen = selector.selected() == kind;
                                if ui.selectable_label(chosen, kind.label()).clicked() {
                                    selector.choose(kind);
                                }
                            } else if !selector.beauty_only()
                                && let Some(aov) = kind.aov()
                            {
                                let text = format!("{} (--aov {})", kind.label(), aov.as_str());
                                ui.add_enabled(false, egui::Button::selectable(false, text));
                            }
                        }
                    });
                ui.separator();
                if ui.button("Fit (F)").clicked() {
                    *fit = true;
                }
                ui.label(format!("{zoom_percent:.0}%"));
                if float {
                    ui.weak("clamped preview; the file holds the real values");
                }
            });
        });
}

/// The window, driven by the render that reports to it.
pub struct WatchSink {
    events: EventLoop<()>,
    app: WatchApp,
    cancel: Arc<AtomicBool>,
    /// Whether the reader's dismissal has already been passed on, so it is not
    /// stored on every later event.
    told: bool,
}

impl WatchSink {
    /// Open nothing yet. The window is sized by the first picture, which is the
    /// first moment anything knows what shape the render is.
    ///
    /// `requested` is what `--aov` asked for, which is what the pass
    /// selector offers; what the run can actually show arrives with the
    /// first preview, which names its engine.
    ///
    /// # Errors
    /// If the platform will not give an event loop at all.
    pub fn new(cancel: Arc<AtomicBool>, requested: &[AovKind]) -> Result<Self, String> {
        let events = EventLoop::new().map_err(|e| format!("no event loop: {e}"))?;
        Ok(Self {
            events,
            app: WatchApp {
                window: None,
                gpu: None,
                chrome: None,
                frame: None,
                latest: None,
                selector: PassSelector::new(requested),
                shown: None,
                view: ViewTransform::new(),
                cursor: (0.0, 0.0),
                drag: None,
                wants_paint: false,
                wanted: None,
                dismissed: false,
                complained: false,
            },
            cancel,
            told: false,
        })
    }

    /// Let the platform have a turn, and pass on a dismissal.
    fn pump(&mut self, wait: std::time::Duration) {
        self.events.pump_app_events(Some(wait), &mut self.app);
        // One paint per pump however many events wanted one, at the moment
        // the batch is done rather than inside it.
        if std::mem::take(&mut self.app.wants_paint) {
            self.app.paint();
        }
        if self.app.dismissed && !self.told {
            // The same flag the interrupt handler and the dashboard's quit key
            // set, so a render stops by one path however it was asked to.
            self.cancel.store(true, Ordering::Relaxed);
            self.told = true;
        }
    }

    /// Hold the finished picture until the reader is done with it.
    ///
    /// Called after the render returns. Without it the last thing a person sees
    /// is the window vanishing at the moment the image was complete, which is
    /// the one frame they were waiting for.
    ///
    /// Returns a line for the reader when there was nothing to hold: a window
    /// only opens once a preview arrives, so a render that ended before its
    /// first tile never had one, and flashing nothing says nothing. A window
    /// opened after the fact was considered and rejected, because this window
    /// draws passes and has no text surface to state a failure on; the error
    /// itself follows on standard error either way. The note is returned
    /// rather than printed here, because a dashboard may still be holding the
    /// screen it would land on.
    pub fn hold(&mut self) -> Option<String> {
        if self.app.window.is_none() {
            return Some(
                "no render window opened: the render ended before its first preview arrived"
                    .to_owned(),
            );
        }
        if self.app.dismissed {
            return None;
        }
        // An interrupt ends the hold too. Without that the only way out is the
        // window, and a person who reaches for the keyboard they started the
        // render from would find the command ignoring them.
        while !self.app.dismissed && !self.cancel.load(Ordering::Relaxed) {
            self.pump(std::time::Duration::from_millis(50));
            self.app.paint();
        }
        None
    }
}

impl RenderSink for WatchSink {
    fn report(&mut self, _progress: &RenderProgress) {
        // Every report, so the window answers a drag or a close without waiting
        // for the next tile. Zero timeout: this is the render's thread and the
        // render is what it is for.
        self.pump(std::time::Duration::ZERO);
    }

    fn preview(&mut self, image: &Preview<'_>) {
        // The selector asks whether the render writes passes, not which engine
        // drew it, so the engine is resolved to a capability here: the rule
        // lives in a crate that cannot see the engine enum at all.
        self.app
            .selector
            .saw_capability(solarxy_render::caps_of(image.engine).writes_aovs);
        let latest = Latest::from_preview(image);
        if self.app.window.is_none() {
            self.app.wanted = Some(window_size(latest.width, latest.height));
        }
        self.app.latest = Some(latest);
        // Fresh planes: whatever pass is on screen was converted from the
        // tile before this one.
        self.app.shown = None;
        // The platform hands over a window on its own schedule rather than when
        // asked, so the first picture is given a moment to get one.
        let wait = if self.app.window.is_none() {
            SETTLE
        } else {
            std::time::Duration::ZERO
        };
        self.pump(wait);
        self.app.sync_frame();
        self.app.paint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A still may be eight thousand pixels an edge; a screen is not. What
    /// survives the bound is the shape.
    #[test]
    fn a_large_render_opens_at_a_size_that_fits_a_screen() {
        let (w, h) = window_size(8192, 4608);
        assert_eq!(w, MAX_EDGE);
        assert_eq!(h, 720, "the shape did not survive the bound");

        // And a picture that already fits is left alone, so an ordinary render
        // opens at its own size.
        assert_eq!(window_size(640, 480), (640, 480));
    }

    /// Switching passes replays the planes the previews already carried:
    /// each pass converts to its own picture, and a pass whose plane is
    /// absent falls back to the beauty rather than to a blank.
    #[test]
    fn a_pass_switch_replays_the_cached_planes() {
        let latest = Latest {
            width: 1,
            height: 1,
            color: vec![10, 20, 30, 255],
            format: PreviewFormat::Rgba8,
            aux: Some(
                [0.5f32, 0.25, 1.0, 0.0]
                    .iter()
                    .flat_map(|v| v.to_le_bytes())
                    .collect(),
            ),
            depth: None,
        };

        let beauty = frame_for(PassKind::Beauty, &latest);
        assert_eq!(beauty.rgba, latest.color, "the beauty is the color plane");

        let albedo = frame_for(PassKind::Albedo, &latest);
        assert_ne!(
            albedo.rgba, beauty.rgba,
            "the albedo pass showed the beauty"
        );
        assert_eq!(albedo.rgba.len(), 4);

        // No depth plane arrived, so the honest recovery is the beauty.
        let depth = frame_for(PassKind::Depth, &latest);
        assert_eq!(depth.rgba, beauty.rgba, "an absent plane did not fall back");
    }

    /// A device against a plain texture, for the tests that draw.
    fn test_device() -> Option<(wgpu::Device, wgpu::Queue)> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let Ok(adapter) =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
        else {
            assert!(
                std::env::var("SOLARXY_REQUIRE_GPU").as_deref() != Ok("1"),
                "SOLARXY_REQUIRE_GPU=1 but no usable GPU adapter"
            );
            eprintln!("skipping: no GPU adapter available");
            return None;
        };
        let Ok(pair) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("watch blit test"),
            required_features: wgpu::Features::empty(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        })) else {
            eprintln!("skipping: no device");
            return None;
        };
        Some(pair)
    }

    /// Draw a 2x2 picture into `rect` of a `target` px square window and read
    /// the result back, padded rows and all.
    fn draw_and_read(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        rect: Rect,
        target_px: u32,
    ) -> Vec<u8> {
        const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
        let corners: [[u8; 4]; 4] = [
            [255, 0, 0, 255],   // top left, red
            [0, 255, 0, 255],   // top right, green
            [0, 0, 255, 255],   // bottom left, blue
            [255, 255, 0, 255], // bottom right, yellow
        ];
        let source: Vec<u8> = corners.iter().flatten().copied().collect();

        let picture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("source"),
            size: wgpu::Extent3d {
                width: 2,
                height: 2,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &picture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &source,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(8),
                rows_per_image: Some(2),
            },
            wgpu::Extent3d {
                width: 2,
                height: 2,
                depth_or_array_layers: 1,
            },
        );

        let (pipeline, layout, sampler) = blit_pipeline(device, FORMAT);
        let view_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("view"),
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(
            &view_buffer,
            0,
            &view_uniform_bytes(rect, (target_px, target_px)),
        );
        let view = picture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: view_buffer.as_entire_binding(),
                },
            ],
        });

        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("target"),
            size: wgpu::Extent3d {
                width: target_px,
                height: target_px,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
        // 256 bytes a row is the copy alignment, so the readback is padded and
        // only the leading bytes of each row are the picture.
        let read = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: 256 * u64::from(target_px),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.draw(0..3, 0..1);
        }
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &read,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(256),
                    rows_per_image: Some(target_px),
                },
            },
            wgpu::Extent3d {
                width: target_px,
                height: target_px,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(Some(encoder.finish()));

        let slice = read.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        // Polled rather than waited on, matching how every other readback in
        // this workspace drains one.
        loop {
            let _ = device.poll(wgpu::PollType::Poll);
            match rx.try_recv() {
                Ok(Ok(())) => break,
                Ok(Err(e)) => panic!("the readback failed: {e}"),
                Err(std::sync::mpsc::TryRecvError::Empty) => std::thread::yield_now(),
                Err(e) => panic!("the readback was dropped: {e}"),
            }
        }
        slice.get_mapped_range().to_vec()
    }

    /// The picture arrives the right way up, and the right way round.
    ///
    /// The one thing here that is easy to get wrong and impossible to see from
    /// a description of a screenshot: screen space runs down and clip space
    /// runs up, so a fullscreen triangle that forgets to flip draws a render
    /// upside down and nothing else in the file would notice.
    ///
    /// Built against a plain texture rather than a surface, so it needs no
    /// window and no display, and skips where there is no adapter the way
    /// every other test that wants a device does.
    #[test]
    fn the_picture_arrives_the_right_way_up() {
        let Some((device, queue)) = test_device() else {
            return;
        };
        // The rectangle is the whole window, so this is the fit at 1:1.
        let data = draw_and_read(
            &device,
            &queue,
            Rect {
                x: 0.0,
                y: 0.0,
                w: 2.0,
                h: 2.0,
            },
            2,
        );
        let pixel = |x: usize, y: usize| -> [u8; 4] {
            let at = y * 256 + x * 4;
            [data[at], data[at + 1], data[at + 2], data[at + 3]]
        };
        assert_eq!(pixel(0, 0), [255, 0, 0, 255], "the top left corner moved");
        assert_eq!(pixel(1, 0), [0, 255, 0, 255], "the top right corner moved");
        assert_eq!(
            pixel(0, 1),
            [0, 0, 255, 255],
            "the bottom left corner moved"
        );
        assert_eq!(
            pixel(1, 1),
            [255, 255, 0, 255],
            "the bottom right corner moved"
        );
    }

    /// The canvas surrounds the picture without showing through it.
    ///
    /// A 2x2 picture drawn into the middle of a 4x4 window: the border must be
    /// the checker's grey, the same grey all round at this scale since the
    /// cell is larger than the window, and the picture's own pixels must be
    /// exactly the picture, opaque.
    #[test]
    fn the_canvas_surrounds_an_opaque_picture() {
        let Some((device, queue)) = test_device() else {
            return;
        };
        let data = draw_and_read(
            &device,
            &queue,
            Rect {
                x: 1.0,
                y: 1.0,
                w: 2.0,
                h: 2.0,
            },
            4,
        );
        let pixel = |x: usize, y: usize| -> [u8; 4] {
            let at = y * 256 + x * 4;
            [data[at], data[at + 1], data[at + 2], data[at + 3]]
        };

        let corner = pixel(0, 0);
        for (x, y) in [(3, 0), (0, 3), (3, 3)] {
            assert_eq!(
                pixel(x, y),
                corner,
                "the canvas is not one grey inside one cell at ({x},{y})"
            );
        }
        assert!(
            corner[0] > 0 && corner[0] < 80 && corner[0] == corner[1] && corner[1] == corner[2],
            "the canvas is not a dark grey: {corner:?}"
        );
        assert_eq!(corner[3], 255, "the canvas is not opaque");

        // The picture itself, untouched by the canvas and opaque over it.
        assert_eq!(pixel(1, 1), [255, 0, 0, 255], "the picture's top left");
        assert_eq!(
            pixel(2, 2),
            [255, 255, 0, 255],
            "the picture's bottom right"
        );
    }

    /// Float pixels reach the window encoded rather than raw, or a
    /// scene-linear render would show far darker than the file it previews.
    #[test]
    fn a_float_picture_is_encoded_on_the_way_to_the_window() {
        let half = 0.5f32.to_le_bytes();
        let one = 1.0f32.to_le_bytes();
        let mut pixels = Vec::new();
        for channel in [half, half, half, one] {
            pixels.extend_from_slice(&channel);
        }
        let latest = Latest::from_preview(&Preview {
            width: 1,
            height: 1,
            pixels: &pixels,
            format: PreviewFormat::Rgba32F,
            aux: None,
            depth: None,
            engine: solarxy_render::RenderEngine::PathTraced,
        });
        let frame = frame_for(PassKind::Beauty, &latest);
        // Linear 0.5 is about 188 of 255 through the sRGB transfer, not 128.
        assert_eq!(frame.rgba[0], 188, "{:?}", &frame.rgba[..4]);
        assert_eq!(frame.rgba[3], 255);
        assert!(frame.float, "a float preview forgot what it was");
    }
}
