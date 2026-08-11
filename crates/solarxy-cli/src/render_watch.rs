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

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use solarxy_render::{Preview, PreviewFormat, RenderProgress, RenderSink};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::platform::pump_events::EventLoopExtPumpEvents;
use winit::window::{Window, WindowId};

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
}

impl Frame {
    fn from_preview(image: &Preview<'_>) -> Self {
        let rgba = match image.format {
            PreviewFormat::Rgba8 => image.pixels.to_vec(),
            // Clamped and encoded, which for a scene-referred image is the
            // whole of the tone mapping. Said plainly in the reference: a float
            // render is judged from its file, and this is a preview.
            PreviewFormat::Rgba32F => image
                .pixels
                .chunks_exact(16)
                .flat_map(|p| {
                    let channel = |i: usize| {
                        let v = f32::from_le_bytes([p[i], p[i + 1], p[i + 2], p[i + 3]])
                            .clamp(0.0, 1.0);
                        // sRGB, so a scene-linear plane is not shown twice as
                        // dark as the file it is a preview of.
                        let encoded = if v <= 0.003_130_8 {
                            v * 12.92
                        } else {
                            1.055 * v.powf(1.0 / 2.4) - 0.055
                        };
                        (encoded * 255.0).round().clamp(0.0, 255.0) as u8
                    };
                    [channel(0), channel(4), channel(8), 255]
                })
                .collect(),
        };
        Self {
            width: image.width,
            height: image.height,
            rgba,
        }
    }

    /// The window size this picture wants: its own proportions, bounded.
    fn window_size(&self) -> (u32, u32) {
        let (w, h) = (self.width.max(1), self.height.max(1));
        if w <= MAX_EDGE && h <= MAX_EDGE {
            return (w, h);
        }
        if w >= h {
            (MAX_EDGE, (MAX_EDGE * h / w).max(1))
        } else {
            ((MAX_EDGE * w / h).max(1), MAX_EDGE)
        }
    }
}

/// What is needed to put a picture on a window, once there is one.
struct Gpu {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// The uploaded picture and its binding, rebuilt when the size changes.
    texture: Option<(wgpu::Texture, wgpu::BindGroup, u32, u32)>,
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
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let (pipeline, layout, sampler) = blit_pipeline(&device, format);

        Ok(Self {
            surface,
            device,
            queue,
            config,
            pipeline,
            layout,
            sampler,
            texture: None,
        })
    }
}

/// The pipeline that puts a picture on a target, and the two things it binds.
///
/// Free of any window, so the same shader and the same layout a window uses can
/// be built against a plain texture and read back. That is the only way to
/// check the one thing in this file that is easy to get wrong and impossible to
/// notice from a screenshot description: whether the picture arrives the right
/// way up.
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

    /// Draw whatever is uploaded, letterboxed into the window.
    fn draw(&mut self, picture: (u32, u32)) {
        let Some((_, bind, _, _)) = self.texture.as_ref() else {
            return;
        };
        let Ok(surface) = self.surface.get_current_texture() else {
            // A surface that is not ready is not an error here: the next tile
            // will ask again, and a render must not stop because a window did.
            return;
        };
        let view = surface
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("watch blit"),
            });
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
            // The letterbox, as a viewport rather than as shader arithmetic:
            // the quad is always the whole clip space and the rectangle it is
            // drawn into is what keeps the picture's proportions.
            let (x, y, w, h) = letterbox(
                picture,
                (self.config.width.max(1), self.config.height.max(1)),
            );
            pass.set_viewport(x, y, w, h, 0.0, 1.0);
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, bind, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit(Some(encoder.finish()));
        surface.present();
    }
}

/// The largest rectangle of `into` with the proportions of `picture`, centred.
fn letterbox(picture: (u32, u32), into: (u32, u32)) -> (f32, f32, f32, f32) {
    let (pw, ph) = (picture.0.max(1) as f32, picture.1.max(1) as f32);
    let (ww, wh) = (into.0.max(1) as f32, into.1.max(1) as f32);
    let scale = (ww / pw).min(wh / ph);
    let (w, h) = (pw * scale, ph * scale);
    (((ww - w) / 2.0).max(0.0), ((wh - h) / 2.0).max(0.0), w, h)
}

/// The window, and everything it does in response to the platform.
struct WatchApp {
    window: Option<Arc<Window>>,
    gpu: Option<Gpu>,
    frame: Option<Frame>,
    /// The size the window should open at, once there is a picture to size it
    /// by. Before the first tile there is nothing to be the shape of.
    wanted: Option<(u32, u32)>,
    /// Set when the reader closed the window or pressed escape.
    dismissed: bool,
    /// Said once. A window that cannot open should say so and then be quiet.
    complained: bool,
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
        match event {
            WindowEvent::CloseRequested => self.dismissed = true,
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state.is_pressed()
                    && matches!(
                        event.logical_key,
                        Key::Named(NamedKey::Escape) | Key::Character(_)
                    )
                    && matches!(
                        event.logical_key.to_text(),
                        None | Some("q") | Some("Q") | Some("\u{1b}")
                    )
                {
                    self.dismissed = true;
                }
            }
            WindowEvent::Resized(size) => {
                if let Some(gpu) = self.gpu.as_mut() {
                    gpu.resize(size.width, size.height);
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

    fn paint(&mut self) {
        let (Some(gpu), Some(frame)) = (self.gpu.as_mut(), self.frame.as_ref()) else {
            return;
        };
        gpu.draw((frame.width, frame.height));
    }
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
    /// # Errors
    /// If the platform will not give an event loop at all.
    pub fn new(cancel: Arc<AtomicBool>) -> Result<Self, String> {
        let events = EventLoop::new().map_err(|e| format!("no event loop: {e}"))?;
        Ok(Self {
            events,
            app: WatchApp {
                window: None,
                gpu: None,
                frame: None,
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
    pub fn hold(&mut self) {
        if self.app.window.is_none() || self.app.dismissed {
            return;
        }
        // An interrupt ends the hold too. Without that the only way out is the
        // window, and a person who reaches for the keyboard they started the
        // render from would find the command ignoring them.
        while !self.app.dismissed && !self.cancel.load(Ordering::Relaxed) {
            self.pump(std::time::Duration::from_millis(50));
            self.app.paint();
        }
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
        let frame = Frame::from_preview(image);
        if self.app.window.is_none() {
            self.app.wanted = Some(frame.window_size());
        }
        self.app.frame = Some(frame);
        // The platform hands over a window on its own schedule rather than when
        // asked, so the first picture is given a moment to get one.
        let wait = if self.app.window.is_none() {
            SETTLE
        } else {
            std::time::Duration::ZERO
        };
        self.pump(wait);
        if let (Some(gpu), Some(frame)) = (self.app.gpu.as_mut(), self.app.frame.as_ref()) {
            gpu.upload(frame);
        }
        self.app.paint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A picture keeps its proportions inside the window, whichever way round
    /// the two are, and is centred in whatever is left.
    #[test]
    fn the_letterbox_keeps_the_pictures_proportions() {
        // Wider than the window: bars above and below.
        let (x, y, w, h) = letterbox((400, 100), (800, 800));
        assert!(
            (w - 800.0).abs() < 0.01 && (h - 200.0).abs() < 0.01,
            "{w}x{h}"
        );
        assert!(x.abs() < 0.01 && (y - 300.0).abs() < 0.01, "{x},{y}");

        // Taller: bars at the sides.
        let (x, y, w, h) = letterbox((100, 400), (800, 800));
        assert!(
            (w - 200.0).abs() < 0.01 && (h - 800.0).abs() < 0.01,
            "{w}x{h}"
        );
        assert!((x - 300.0).abs() < 0.01 && y.abs() < 0.01, "{x},{y}");

        // Exactly the shape of the window: no bars at all.
        let (x, y, w, h) = letterbox((640, 480), (1280, 960));
        assert!(x.abs() < 0.01 && y.abs() < 0.01, "{x},{y}");
        assert!(
            (w - 1280.0).abs() < 0.01 && (h - 960.0).abs() < 0.01,
            "{w}x{h}"
        );
    }

    /// A still may be eight thousand pixels an edge; a screen is not. What
    /// survives the bound is the shape.
    #[test]
    fn a_large_render_opens_at_a_size_that_fits_a_screen() {
        let huge = Frame {
            width: 8192,
            height: 4608,
            rgba: Vec::new(),
        };
        let (w, h) = huge.window_size();
        assert_eq!(w, MAX_EDGE);
        assert_eq!(h, 720, "the shape did not survive the bound");

        // And a picture that already fits is left alone, so an ordinary render
        // opens at its own size.
        let small = Frame {
            width: 640,
            height: 480,
            rgba: Vec::new(),
        };
        assert_eq!(small.window_size(), (640, 480));
    }

    /// The picture arrives the right way up, and the right way round.
    ///
    /// The one thing here that is easy to get wrong and impossible to see from
    /// a description of a screenshot: texture space runs down and clip space
    /// runs up, so a fullscreen triangle that forgets to flip draws a render
    /// upside down and nothing else in the file would notice.
    ///
    /// Built against a plain texture rather than a surface, so it needs no
    /// window and no display, and skips where there is no adapter the way
    /// every other test that wants a device does.
    #[test]
    fn the_picture_arrives_the_right_way_up() {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let Ok(adapter) =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
        else {
            assert!(
                std::env::var("SOLARXY_REQUIRE_GPU").as_deref() != Ok("1"),
                "SOLARXY_REQUIRE_GPU=1 but no usable GPU adapter"
            );
            eprintln!("skipping: no GPU adapter available");
            return;
        };
        let Ok((device, queue)) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("watch blit test"),
                required_features: wgpu::Features::empty(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::Off,
            }))
        else {
            eprintln!("skipping: no device");
            return;
        };

        // Four distinct corners, so every way of getting this wrong shows.
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

        let (pipeline, layout, sampler) = blit_pipeline(&device, FORMAT);
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
            ],
        });

        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("target"),
            size: wgpu::Extent3d {
                width: 2,
                height: 2,
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
        // only the first eight bytes of each row are the picture.
        let read = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: 256 * 2,
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
                    rows_per_image: Some(2),
                },
            },
            wgpu::Extent3d {
                width: 2,
                height: 2,
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
        let data = slice.get_mapped_range().to_vec();

        let pixel = |x: usize, y: usize| -> [u8; 4] {
            let at = y * 256 + x * 4;
            [data[at], data[at + 1], data[at + 2], data[at + 3]]
        };
        assert_eq!(pixel(0, 0), corners[0], "the top left corner moved");
        assert_eq!(pixel(1, 0), corners[1], "the top right corner moved");
        assert_eq!(pixel(0, 1), corners[2], "the bottom left corner moved");
        assert_eq!(pixel(1, 1), corners[3], "the bottom right corner moved");
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
        let frame = Frame::from_preview(&Preview {
            width: 1,
            height: 1,
            pixels: &pixels,
            format: PreviewFormat::Rgba32F,
        });
        // Linear 0.5 is about 188 of 255 through the sRGB transfer, not 128.
        assert_eq!(frame.rgba[0], 188, "{:?}", &frame.rgba[..4]);
        assert_eq!(frame.rgba[3], 255);
    }
}
