use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
const HINTS_MODEL: &str = "\
    M Mode  1-5 Inspect  S Shaded  X Ghost  N Normals  U UV  B Bg  G Grid  A Axes  \
    I IBL  E/Shift+E Exposure\n\
    Shift+W Weight  Shift+B Bounds  Shift+D Bloom  Shift+O SSAO  Shift+T Tone  \
    Shift+I IBL Mode  Shift+V Valid\n\
    Shift+A Local Axes  Shift+L Lights  Shift+S Save  V Turn  P/O Proj  \
    C Cap  H Frame  Tab Panel  ? Hints\n\
    F1 Single  F2 V-Split  F3 H-Split  F10 Menu  F11 FS  \u{2318}+L Link";
#[cfg(not(target_os = "macos"))]
const HINTS_MODEL: &str = "\
    M Mode  1-5 Inspect  S Shaded  X Ghost  N Normals  U UV  B Bg  G Grid  A Axes  \
    I IBL  E/Shift+E Exposure\n\
    Shift+W Weight  Shift+B Bounds  Shift+D Bloom  Shift+O SSAO  Shift+T Tone  \
    Shift+I IBL Mode  Shift+V Valid\n\
    Shift+A Local Axes  Shift+L Lights  Shift+S Save  V Turn  P/O Proj  \
    C Cap  H Frame  Tab Panel  ? Hints\n\
    F1 Single  F2 V-Split  F3 H-Split  F10 Menu  F11 FS  Ctrl+L Link";

#[cfg(target_os = "macos")]
const HINTS_NO_MODEL: &str = "\u{2318}+O Open  \u{2318}+Shift+O HDRI  ? Hints";
#[cfg(not(target_os = "macos"))]
const HINTS_NO_MODEL: &str = "Ctrl+O Open  Ctrl+Shift+O HDRI  ? Hints";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ToastSeverity {
    #[default]
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Debug)]
pub(super) struct Toast {
    pub message: String,
    pub severity: ToastSeverity,
    pub created: Instant,
    pub duration: Duration,
}

pub(super) struct HudCtx<'a> {
    pub avg_ms: f32,
    pub fps: u32,
    pub toast: Option<&'a Toast>,
    pub loading_message: Option<&'a String>,
    pub has_model: bool,
    pub hints_visible: bool,
    pub fps_hud_visible: bool,
    pub backend_info: &'a str,
    pub pane_label: &'a str,
    pub cameras_linked: Option<bool>,
    pub validation_counts: (usize, usize),
}

#[derive(Debug, Default)]
pub(super) struct HudResult {
    pub toast_dismissed: bool,
}

pub(super) fn overlay_frame() -> egui::Frame {
    egui::Frame::NONE
        .fill(egui::Color32::from_black_alpha(160))
        .corner_radius(egui::CornerRadius::same(3))
        .inner_margin(egui::Margin::same(4))
}

#[allow(clippy::too_many_arguments)]
fn draw_fps_hud(
    ctx: &egui::Context,
    avg_ms: f32,
    fps: u32,
    backend_info: &str,
    pane_label: &str,
    cameras_linked: Option<bool>,
    has_model: bool,
    validation_counts: (usize, usize),
) {
    let screen = ctx.content_rect();
    let default_pos = egui::pos2(screen.right() - 8.0, screen.top() + 8.0);
    egui::Window::new("fps_hud")
        .title_bar(false)
        .resizable(false)
        .collapsible(false)
        .movable(true)
        .default_pos(default_pos)
        .pivot(egui::Align2::RIGHT_TOP)
        .order(egui::Order::Foreground)
        .frame(overlay_frame())
        .show(ctx, |ui| {
            ui.label(
                egui::RichText::new(format!("{avg_ms:.1} ms  {fps} fps"))
                    .small()
                    .color(egui::Color32::from_white_alpha(200)),
            );
            if !backend_info.is_empty() {
                ui.label(
                    egui::RichText::new(backend_info)
                        .small()
                        .color(egui::Color32::from_white_alpha(160)),
                );
            }
            if has_model && !pane_label.is_empty() {
                ui.label(
                    egui::RichText::new(pane_label)
                        .small()
                        .color(egui::Color32::from_white_alpha(180)),
                );
            }
            if let Some(linked) = cameras_linked {
                let text = if linked {
                    "Cameras: Linked"
                } else {
                    "Cameras: Independent"
                };
                ui.label(
                    egui::RichText::new(text)
                        .small()
                        .color(egui::Color32::from_white_alpha(140)),
                );
            }
            let (errors, warnings) = validation_counts;
            if errors > 0 || warnings > 0 {
                let mut parts = Vec::new();
                if errors > 0 {
                    parts.push(format!(
                        "\u{2715} {} error{}",
                        errors,
                        if errors == 1 { "" } else { "s" }
                    ));
                }
                if warnings > 0 {
                    parts.push(format!(
                        "\u{26a0} {} warning{}",
                        warnings,
                        if warnings == 1 { "" } else { "s" }
                    ));
                }
                let color = if errors > 0 {
                    egui::Color32::from_rgb(255, 100, 100)
                } else {
                    egui::Color32::from_rgb(255, 200, 80)
                };
                ui.label(egui::RichText::new(parts.join("  ")).small().color(color));
            }
        });
}

fn draw_toast(ctx: &egui::Context, toast: &Toast) -> bool {
    let (icon, icon_color) = match toast.severity {
        ToastSeverity::Error => ("\u{2715}", egui::Color32::from_rgb(255, 100, 100)),
        ToastSeverity::Warning => ("\u{26A0}", egui::Color32::from_rgb(255, 200, 80)),
        ToastSeverity::Success => ("\u{2713}", egui::Color32::from_rgb(100, 220, 120)),
        ToastSeverity::Info => ("\u{2139}", egui::Color32::from_rgb(120, 180, 255)),
    };

    let content = ctx.content_rect();
    let banner_width = (content.width() - 40.0).max(200.0);
    let anchor_pos = egui::pos2(content.center().x, content.top() + 8.0);

    let mut dismissed = false;
    egui::Area::new(egui::Id::new("toast_banner"))
        .fixed_pos(anchor_pos)
        .pivot(egui::Align2::CENTER_TOP)
        .order(egui::Order::Foreground)
        .interactable(true)
        .show(ctx, |ui| {
            ui.set_width(banner_width);
            let frame_resp = egui::Frame::NONE
                .fill(egui::Color32::from_rgba_unmultiplied(30, 30, 40, 220))
                .corner_radius(egui::CornerRadius::same(4))
                .inner_margin(egui::Margin::symmetric(12, 6))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(icon).color(icon_color).strong());
                        ui.label(
                            egui::RichText::new(&toast.message)
                                .color(egui::Color32::from_white_alpha(220)),
                        );
                    });
                })
                .response
                .interact(egui::Sense::click());
            if frame_resp.hovered() {
                ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if frame_resp.clicked() {
                dismissed = true;
            }
        });
    dismissed
}

pub(super) fn draw_hud_overlays(ctx: &egui::Context, hud: &HudCtx) -> HudResult {
    let mut result = HudResult::default();

    if hud.fps_hud_visible {
        draw_fps_hud(
            ctx,
            hud.avg_ms,
            hud.fps,
            hud.backend_info,
            hud.pane_label,
            hud.cameras_linked,
            hud.has_model,
            hud.validation_counts,
        );
    }

    if let Some(toast) = hud.toast
        && draw_toast(ctx, toast)
    {
        result.toast_dismissed = true;
    }

    if let Some(msg) = hud.loading_message {
        egui::Area::new(egui::Id::new("loading_overlay"))
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                overlay_frame().show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(msg)
                            .size(16.0)
                            .color(egui::Color32::from_rgb(128, 179, 255)),
                    );
                });
            });
    } else if !hud.has_model {
        egui::Area::new(egui::Id::new("drop_overlay"))
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                overlay_frame().show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(
                            "Drop a 3D model to view\n(.obj  .stl  .ply  .gltf  .glb)",
                        )
                        .size(16.0)
                        .color(egui::Color32::from_white_alpha(140)),
                    );
                });
            });
    }

    if hud.hints_visible {
        let hints: &'static str = if hud.has_model {
            HINTS_MODEL
        } else {
            HINTS_NO_MODEL
        };
        egui::Area::new(egui::Id::new("hints_overlay"))
            .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -8.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                ui.set_max_width(ctx.content_rect().width().min(900.0));
                overlay_frame().show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(hints)
                            .small()
                            .color(egui::Color32::from_white_alpha(160)),
                    );
                });
            });
    }

    result
}
