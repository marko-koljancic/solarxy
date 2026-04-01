use std::collections::VecDeque;
use std::time::{Duration, Instant};

use wgpu_text::glyph_brush::{ab_glyph::FontRef, HorizontalAlign, Layout, Section, Text, VerticalAlign};
use wgpu_text::BrushBuilder;
use wgpu_text::TextBrush;

use crate::format_number;

use super::resources::ModelStats;

struct Toast {
    message: String,
    color: [f32; 4],
    created: Instant,
    duration: Duration,
}

pub struct HudRenderer {
    brush: TextBrush<FontRef<'static>>,
    hints_visible: bool,
    scale_factor: f64,
    filename: String,
    mesh_count: usize,
    stats_text: String,
    toast: Option<Toast>,
    loading_message: Option<String>,
    frame_times: VecDeque<f32>,
}

impl HudRenderer {
    pub fn new(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
        width: u32,
        height: u32,
        stats: Option<&ModelStats>,
        scale_factor: f64,
    ) -> Self {
        let font_bytes: &[u8] = include_bytes!("../../res/Lilex/static/Lilex-Medium.ttf");
        let brush = BrushBuilder::using_font_bytes(font_bytes)
            .expect("Failed to load font")
            .build(device, width, height, surface_format);

        let stats_text = Self::format_stats(stats);

        HudRenderer {
            brush,
            hints_visible: true,
            scale_factor,
            filename: String::new(),
            mesh_count: 0,
            stats_text,
            toast: None,
            loading_message: None,
            frame_times: VecDeque::with_capacity(30),
        }
    }

    fn format_stats(stats: Option<&ModelStats>) -> String {
        match stats {
            Some(s) => format!(
                "Polys {}  Tris {}  Verts {}",
                format_number(s.polys),
                format_number(s.tris),
                format_number(s.verts),
            ),
            None => String::new(),
        }
    }

    pub fn update_stats(&mut self, stats: Option<&ModelStats>) {
        self.stats_text = Self::format_stats(stats);
    }

    pub fn update_model_info(&mut self, filename: &str, mesh_count: usize) {
        self.filename = Self::truncate_filename(filename, 30);
        self.mesh_count = mesh_count;
    }

    fn truncate_filename(name: &str, max_chars: usize) -> String {
        if name.chars().count() > max_chars {
            let truncated: String = name.chars().take(max_chars - 3).collect();
            format!("{truncated}...")
        } else {
            name.to_string()
        }
    }

    pub fn resize(&mut self, width: u32, height: u32, queue: &wgpu::Queue) {
        self.brush.resize_view(width as f32, height as f32, queue);
    }

    pub fn set_scale_factor(&mut self, scale_factor: f64) {
        self.scale_factor = scale_factor;
    }

    pub fn toggle_hints(&mut self) {
        self.hints_visible = !self.hints_visible;
    }

    pub fn set_capture_message(&mut self, filename: String) {
        let msg = format!("Saved {}", filename);
        self.toast = Some(Toast {
            message: msg,
            color: [0.0, 0.4, 0.0, 1.0],
            created: Instant::now(),
            duration: Duration::from_secs(2),
        });
    }

    pub fn set_toast(&mut self, msg: &str, color: [f32; 4]) {
        self.toast = Some(Toast {
            message: msg.to_string(),
            color,
            created: Instant::now(),
            duration: Duration::from_secs(3),
        });
    }

    pub fn set_loading_message(&mut self, msg: &str) {
        self.loading_message = Some(msg.to_string());
    }

    pub fn clear_loading_message(&mut self) {
        self.loading_message = None;
    }

    pub fn clear_expired_toast(&mut self) {
        if let Some(ref toast) = self.toast
            && toast.created.elapsed() > toast.duration
        {
            self.toast = None;
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        queue: &wgpu::Queue,
        screen_width: u32,
        screen_height: u32,
        view_mode: &str,
        projection: &str,
        normals: &str,
        bg_name: &str,
        bg_color: wgpu::Color,
        frame_ms: f32,
        has_model: bool,
        show_grid: bool,
        lights_locked: bool,
        show_axis_gizmo: bool,
        bounds_mode: &str,
        bounds_info: &str,
        line_weight: &str,
        ibl_enabled: bool,
    ) {
        let sf = self.scale_factor as f32;
        let font_size_main = 14.0 * sf;
        let font_size_hints = 13.0 * sf;
        let margin = 12.0 * sf;
        let line_gap = 4.0 * sf;
        let line_height = font_size_main + line_gap;

        if self.frame_times.len() >= 30 {
            self.frame_times.pop_front();
        }
        self.frame_times.push_back(frame_ms);
        let avg_ms: f32 = self.frame_times.iter().sum::<f32>() / self.frame_times.len() as f32;
        let fps = 1000.0 / avg_ms;
        let timing_text = format!("{:.1} ms  {} fps", avg_ms, fps as u32);

        let lum = 0.2126 * bg_color.r + 0.7152 * bg_color.g + 0.0722 * bg_color.b;
        let text_val = if lum > 0.5 { 0.0_f32 } else { 1.0_f32 };
        let text_color: [f32; 4] = [text_val, text_val, text_val, 1.0];
        let hint_color: [f32; 4] = [text_val, text_val, text_val, 0.6];

        let file_info = if has_model && !self.filename.is_empty() {
            let mesh_label = if self.mesh_count == 1 {
                "mesh"
            } else {
                "meshes"
            };
            format!(
                "{} \u{2014} {} {}",
                self.filename, self.mesh_count, mesh_label
            )
        } else {
            String::new()
        };

        let state_lines: Vec<String> = if has_model {
            let mut lines = vec![
                format!("Mode: {}", view_mode),
                format!("Proj: {}", projection),
                format!("Normals: {}", normals),
                format!("BG: {}", bg_name),
            ];
            if line_weight != "Medium" {
                lines.push(format!("Weight: {}", line_weight));
            }
            if !show_grid {
                lines.push("Grid: Off".to_string());
            }
            if lights_locked {
                lines.push("Lights: Locked".to_string());
            }
            if show_axis_gizmo {
                lines.push("Axes: On".to_string());
            }
            if bounds_mode != "Off" {
                lines.push(format!("Bounds: {}", bounds_mode));
            }
            if !ibl_enabled {
                lines.push("IBL: Off".to_string());
            }
            lines
        } else {
            Vec::new()
        };

        let mut sections: Vec<Section> = Vec::new();
        let mut y = margin;

        if has_model {
            if !file_info.is_empty() {
                sections.push(
                    Section::default()
                        .add_text(
                            Text::new(&file_info)
                                .with_scale(font_size_main)
                                .with_color(text_color),
                        )
                        .with_screen_position((margin, y))
                        .with_layout(Layout::default_single_line()),
                );
                y += line_height;
            }

            sections.push(
                Section::default()
                    .add_text(
                        Text::new(&self.stats_text)
                            .with_scale(font_size_main)
                            .with_color(text_color),
                    )
                    .with_screen_position((margin, y))
                    .with_layout(Layout::default_single_line()),
            );
            y += line_height;

            if !bounds_info.is_empty() {
                sections.push(
                    Section::default()
                        .add_text(
                            Text::new(bounds_info)
                                .with_scale(font_size_main)
                                .with_color(text_color),
                        )
                        .with_screen_position((margin, y))
                        .with_layout(Layout::default().h_align(HorizontalAlign::Left)),
                );
                let bounds_lines = bounds_info.lines().count().max(1);
                y += line_height * bounds_lines as f32;
            }
        }

        sections.push(
            Section::default()
                .add_text(
                    Text::new(&timing_text)
                        .with_scale(font_size_main)
                        .with_color(text_color),
                )
                .with_screen_position((margin, y))
                .with_layout(Layout::default_single_line()),
        );

        if has_model {
            let mut yr = margin;
            for line in &state_lines {
                sections.push(
                    Section::default()
                        .add_text(
                            Text::new(line)
                                .with_scale(font_size_main)
                                .with_color(text_color),
                        )
                        .with_screen_position((screen_width as f32 - margin, yr))
                        .with_layout(Layout::default_single_line().h_align(HorizontalAlign::Right)),
                );
                yr += line_height;
            }
        }

        let hints = if has_model {
            "W Mode  S Shaded  X Ghost  N Normals  U UV  B Bg  G Grid  A Axes  I IBL\nShift+W Weight  Shift+B Bounds  Shift+M Bloom  Shift+L Lights  Shift+S Save  V Turn  P/O Proj  C Cap  H Frame  ? Hints"
        } else {
            "? Hints"
        };
        let hint_section = Section::default()
            .add_text(
                Text::new(hints)
                    .with_scale(font_size_hints)
                    .with_color(hint_color),
            )
            .with_screen_position((screen_width as f32 / 2.0, screen_height as f32 - margin))
            .with_layout(
                Layout::default_wrap()
                    .h_align(HorizontalAlign::Center)
                    .v_align(VerticalAlign::Bottom),
            );

        if let Some(ref toast) = self.toast {
            sections.push(
                Section::default()
                    .add_text(
                        Text::new(&toast.message)
                            .with_scale(font_size_main)
                            .with_color(toast.color),
                    )
                    .with_screen_position((
                        screen_width as f32 / 2.0,
                        screen_height as f32 - margin - font_size_hints * 2.0 - margin,
                    ))
                    .with_layout(
                        Layout::default_single_line()
                            .h_align(HorizontalAlign::Center)
                            .v_align(VerticalAlign::Bottom),
                    ),
            );
        }

        if let Some(ref msg) = self.loading_message {
            let loading_color: [f32; 4] = [0.5, 0.7, 1.0, 0.9];
            let font_size_loading = 18.0 * sf;
            sections.push(
                Section::default()
                    .add_text(
                        Text::new(msg)
                            .with_scale(font_size_loading)
                            .with_color(loading_color),
                    )
                    .with_screen_position((screen_width as f32 / 2.0, screen_height as f32 / 2.0))
                    .with_layout(
                        Layout::default_single_line()
                            .h_align(HorizontalAlign::Center)
                            .v_align(VerticalAlign::Center),
                    ),
            );
        } else if !has_model && self.toast.is_none() {
            let drop_color: [f32; 4] = [text_val, text_val, text_val, 0.5];
            let font_size_drop = 18.0 * sf;
            sections.push(
                Section::default()
                    .add_text(
                        Text::new("Drop a 3D model to view\n(.obj  .stl  .ply  .gltf  .glb)")
                            .with_scale(font_size_drop)
                            .with_color(drop_color),
                    )
                    .with_screen_position((screen_width as f32 / 2.0, screen_height as f32 / 2.0))
                    .with_layout(
                        Layout::default_wrap()
                            .h_align(HorizontalAlign::Center)
                            .v_align(VerticalAlign::Center),
                    ),
            );
        }

        if self.hints_visible {
            sections.push(hint_section);
        }

        let section_refs: Vec<&Section> = sections.iter().collect();
        self.brush.queue(device, queue, section_refs).unwrap();

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("HUD Text Pass"),
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
                occlusion_query_set: None,
                timestamp_writes: None,
            });

            self.brush.draw(&mut pass);
        }
    }
}
