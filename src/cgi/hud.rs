use wgpu_text::glyph_brush::{ab_glyph::FontRef, HorizontalAlign, Layout, Section, Text, VerticalAlign};
use wgpu_text::BrushBuilder;
use wgpu_text::TextBrush;

pub struct ModelStats {
    pub polys: usize,
    pub tris: usize,
    pub verts: usize,
}

impl ModelStats {
    pub fn from_path(path: &str) -> Self {
        let ext = std::path::Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match ext.as_str() {
            "stl" => Self::from_stl(path),
            "ply" => Self::from_ply(path),
            _ => Self::from_obj(path),
        }
    }

    fn from_obj(path: &str) -> Self {
        let (models_raw, _) = tobj::load_obj(
            path,
            &tobj::LoadOptions {
                triangulate: false,
                single_index: true,
                ..Default::default()
            },
        )
        .unwrap();

        let polys: usize = models_raw
            .iter()
            .map(|m| {
                if m.mesh.face_arities.is_empty() {
                    m.mesh.indices.len() / 3
                } else {
                    m.mesh.face_arities.len()
                }
            })
            .sum();

        let (models_tri, _) = tobj::load_obj(
            path,
            &tobj::LoadOptions {
                triangulate: true,
                single_index: true,
                ..Default::default()
            },
        )
        .unwrap();

        let tris: usize = models_tri.iter().map(|m| m.mesh.indices.len() / 3).sum();
        let verts: usize = models_tri.iter().map(|m| m.mesh.positions.len() / 3).sum();

        ModelStats { polys, tris, verts }
    }

    fn from_stl(path: &str) -> Self {
        let file = std::fs::File::open(path).unwrap();
        let mut reader = std::io::BufReader::new(file);
        let mesh = stl_io::read_stl(&mut reader).unwrap();

        let tris = mesh.faces.len();
        let verts = mesh.vertices.len();

        ModelStats {
            polys: tris,
            tris,
            verts,
        }
    }

    fn from_ply(path: &str) -> Self {
        let file = std::fs::File::open(path).unwrap();
        let mut reader = std::io::BufReader::new(file);
        let parser = ply_rs_bw::parser::Parser::<ply_rs_bw::ply::DefaultElement>::new();
        let ply = parser.read_ply(&mut reader).unwrap();

        let verts = ply.payload.get("vertex").map_or(0, |v| v.len());

        let mut polys = 0usize;
        let mut tris = 0usize;
        if let Some(faces) = ply.payload.get("face") {
            polys = faces.len();
            for face in faces {
                let n = face
                    .get("vertex_indices")
                    .or_else(|| face.get("vertex_index"))
                    .map(|p| match p {
                        ply_rs_bw::ply::Property::ListInt(v) => v.len(),
                        ply_rs_bw::ply::Property::ListUInt(v) => v.len(),
                        ply_rs_bw::ply::Property::ListShort(v) => v.len(),
                        ply_rs_bw::ply::Property::ListUShort(v) => v.len(),
                        ply_rs_bw::ply::Property::ListUChar(v) => v.len(),
                        ply_rs_bw::ply::Property::ListChar(v) => v.len(),
                        _ => 0,
                    })
                    .unwrap_or(0);
                if n >= 3 {
                    tris += n - 2;
                }
            }
        }

        ModelStats { polys, tris, verts }
    }
}

pub struct HudRenderer {
    brush: TextBrush<FontRef<'static>>,
    hints_visible: bool,
    scale_factor: f64,
    stats_text: String,
}

impl HudRenderer {
    pub fn new(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
        width: u32,
        height: u32,
        model_path: &str,
        scale_factor: f64,
    ) -> Self {
        let font_bytes: &[u8] = include_bytes!("../../res/Lilex/static/Lilex-Medium.ttf");
        let brush = BrushBuilder::using_font_bytes(font_bytes)
            .expect("Failed to load font")
            .build(device, width, height, surface_format);

        let stats = ModelStats::from_path(model_path);
        let stats_text = format!(
            "Polys {}  Tris {}  Verts {}",
            format_number(stats.polys),
            format_number(stats.tris),
            format_number(stats.verts),
        );

        HudRenderer {
            brush,
            hints_visible: true,
            scale_factor,
            stats_text,
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
    ) {
        let sf = self.scale_factor as f32;
        let font_size_main = 14.0 * sf;
        let font_size_hints = 13.0 * sf;
        let margin = 12.0 * sf;

        let black: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
        let hint_color: [f32; 4] = [0.0, 0.0, 0.0, 0.6];

        let stats_section = Section::default()
            .add_text(Text::new(&self.stats_text).with_scale(font_size_main).with_color(black))
            .with_screen_position((margin, margin))
            .with_layout(Layout::default_single_line());

        let state_text = format!("Mode: {}  Proj: {}  Normals: {}", view_mode, projection, normals);
        let state_section = Section::default()
            .add_text(Text::new(&state_text).with_scale(font_size_main).with_color(black))
            .with_screen_position((screen_width as f32 - margin, margin))
            .with_layout(Layout::default_single_line().h_align(HorizontalAlign::Right));

        let mut sections: Vec<&Section> = vec![&stats_section, &state_section];

        let hints = "W Mode  S Shaded  X Ghost  N Normals  H Frame  T F L R Views  P Persp  O Ortho  ? Hints";
        let hint_section = Section::default()
            .add_text(Text::new(hints).with_scale(font_size_hints).with_color(hint_color))
            .with_screen_position((screen_width as f32 / 2.0, screen_height as f32 - margin))
            .with_layout(
                Layout::default_single_line()
                    .h_align(HorizontalAlign::Center)
                    .v_align(VerticalAlign::Bottom),
            );

        if self.hints_visible {
            sections.push(&hint_section);
        }

        self.brush.queue(device, queue, sections).unwrap();

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

fn format_number(n: usize) -> String {
    let s = n.to_string();
    let mut result = String::new();
    let chars: Vec<char> = s.chars().collect();

    for (i, c) in chars.iter().enumerate() {
        if i > 0 && (chars.len() - i).is_multiple_of(3) {
            result.push(',');
        }
        result.push(*c);
    }

    result
}
