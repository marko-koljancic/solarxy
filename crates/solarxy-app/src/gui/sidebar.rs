use solarxy_core::preferences::{MaterialOverride, ToneMode};
use solarxy_core::view_config::{
    MAX_BLOOM_STRENGTH, MAX_BLOOM_THRESHOLD, MAX_SSAO_STRENGTH, MIN_BLOOM_STRENGTH,
    MIN_BLOOM_THRESHOLD, MIN_SSAO_STRENGTH,
};

use super::intent::{DisplayChange, Intent, Intents, PostChange};
use super::settings::PanelSettings;

/// A labelled combo over `T::ALL`, returning the variant the user picked.
///
/// Returns rather than writing through a mutable borrow: a panel reads the
/// current value and asks for a new one, and never holds a handle on the
/// shell's state.
pub(super) fn combo_with_tooltip<T>(
    ui: &mut egui::Ui,
    label: &str,
    shortcut: &str,
    current: T,
    all: &[T],
) -> Option<T>
where
    T: Copy + PartialEq + std::fmt::Display,
{
    let mut picked = None;
    ui.horizontal(|ui| {
        let mut value = current;
        egui::ComboBox::from_id_salt(label)
            .selected_text(current.to_string())
            .width(140.0)
            .show_ui(ui, |ui| {
                for &variant in all {
                    if ui
                        .selectable_value(&mut value, variant, variant.to_string())
                        .changed()
                    {
                        picked = Some(variant);
                    }
                }
            });
        ui.label(label).on_hover_text(shortcut);
    });
    picked
}

/// A labelled checkbox, returning the new value when the user flipped it.
fn checkbox_with_tooltip(
    ui: &mut egui::Ui,
    value: bool,
    label: &str,
    shortcut: &str,
) -> Option<bool> {
    let mut flipped = None;
    ui.horizontal(|ui| {
        let mut current = value;
        if ui.checkbox(&mut current, label).changed() {
            flipped = Some(current);
        }
        ui.small(shortcut)
            .on_hover_text(format!("Shortcut: {shortcut}"));
    });
    flipped
}

/// A slider bound to a copy of `value`, returning the new value when the
/// user moved it.
fn slider(
    ui: &mut egui::Ui,
    value: f32,
    build: impl FnOnce(&mut f32) -> egui::Slider<'_>,
) -> Option<f32> {
    let mut current = value;
    let changed = ui.add(build(&mut current)).changed();
    changed.then_some(current)
}

/// Render the sidebar's collapsible-panels content directly into the
/// provided `ui` — the SidePanel/ScrollArea shell is the caller's job.
/// Lives this way so `gui::dock` can host the sidebar as an `egui_dock`
/// tab (which provides its own `Ui`).
///
/// RC2: the sidebar is the canonical surface for **scene-global**
/// display / post-processing / material settings only. Per-pane view
/// state lives on the per-pane toolbar; validation and HDRI/IBL moved to
/// the Properties panel.
pub(super) fn draw_sidebar_content(
    ui: &mut egui::Ui,
    settings: PanelSettings<'_>,
    intents: &mut Intents,
) {
    let display = settings.display;
    let post = settings.post;
    let strengths = post.strengths();

    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.add_space(2.0);

        egui::CollapsingHeader::new("Display")
            .default_open(true)
            .show(ui, |ui| {
                if let Some(v) =
                    checkbox_with_tooltip(ui, display.lights_locked, "Lock Lights", "Shift+L")
                {
                    intents.raise(Intent::Display(DisplayChange::LightsLocked(v)));
                }
                if let Some(v) =
                    checkbox_with_tooltip(ui, display.turntable_active, "Turntable", "V")
                {
                    intents.raise(Intent::Display(DisplayChange::TurntableActive(v)));
                }
                if display.turntable_active {
                    ui.indent("turntable_indent", |ui| {
                        if let Some(v) = slider(ui, display.turntable_rpm, |v| {
                            egui::Slider::new(v, 1.0..=60.0)
                                .text("RPM")
                                .logarithmic(true)
                        }) {
                            intents.raise(Intent::Display(DisplayChange::TurntableRpm(v)));
                        }
                    });
                }
            });

        ui.separator();

        egui::CollapsingHeader::new("Post-Processing")
            .default_open(true)
            .show(ui, |ui| {
                if let Some(v) = checkbox_with_tooltip(ui, post.bloom_enabled, "Bloom", "Shift+D") {
                    intents.raise(Intent::Post(PostChange::Bloom(v)));
                }
                ui.add_enabled_ui(post.bloom_enabled, |ui| {
                    let mut next = strengths;
                    let strength = ui
                        .add(
                            egui::Slider::new(
                                &mut next.bloom_strength,
                                MIN_BLOOM_STRENGTH..=MAX_BLOOM_STRENGTH,
                            )
                            .text("Bloom Strength"),
                        )
                        .on_hover_text("How much of the blurred bright pass is added back.")
                        .changed();
                    let threshold = ui
                        .add(
                            egui::Slider::new(
                                &mut next.bloom_threshold,
                                MIN_BLOOM_THRESHOLD..=MAX_BLOOM_THRESHOLD,
                            )
                            .text("Bloom Threshold"),
                        )
                        .on_hover_text("Luminance a pixel has to exceed before it blooms.")
                        .changed();
                    if strength || threshold {
                        intents.raise(Intent::Post(PostChange::Strengths(next)));
                    }
                });
                if let Some(v) = checkbox_with_tooltip(ui, post.ssao_enabled, "SSAO", "Shift+O") {
                    intents.raise(Intent::Post(PostChange::Ssao(v)));
                }
                ui.add_enabled_ui(post.ssao_enabled, |ui| {
                    let mut next = strengths;
                    if ui
                        .add(
                            egui::Slider::new(
                                &mut next.ssao_strength,
                                MIN_SSAO_STRENGTH..=MAX_SSAO_STRENGTH,
                            )
                            .text("Occlusion Strength"),
                        )
                        .on_hover_text(
                            "How far the composite blends towards the occlusion buffer. \
                             The AO Preview inspection mode shows the raw buffer and is \
                             deliberately unaffected by this.",
                        )
                        .changed()
                    {
                        intents.raise(Intent::Post(PostChange::Strengths(next)));
                    }
                });
                if let Some(mode) =
                    combo_with_tooltip(ui, "Tone Map", "Shift+T", post.tone_mode, ToneMode::ALL)
                {
                    intents.raise(Intent::Post(PostChange::ToneMode(mode)));
                }
                let mut exposure = post.exposure;
                let row = ui.horizontal(|ui| {
                    ui.add(
                        egui::Slider::new(&mut exposure, 0.1..=10.0)
                            .text("Exposure")
                            .logarithmic(true),
                    )
                    .changed()
                });
                row.response.on_hover_text("E / Shift+E");
                if row.inner {
                    intents.raise(Intent::Post(PostChange::Exposure(exposure)));
                }
            });

        ui.separator();

        egui::CollapsingHeader::new("Material")
            .default_open(false)
            .show(ui, |ui| {
                let overridden = settings.active_pane().material_override != MaterialOverride::None;
                ui.add_enabled_ui(!overridden, |ui| {
                    if let Some(v) = slider(ui, display.roughness_scale, |v| {
                        egui::Slider::new(v, 0.0..=1.0).text("Roughness Scale")
                    }) {
                        intents.raise(Intent::Display(DisplayChange::RoughnessScale(v)));
                    }
                    if let Some(v) = slider(ui, display.metallic_scale, |v| {
                        egui::Slider::new(v, 0.0..=1.0).text("Metallic Scale")
                    }) {
                        intents.raise(Intent::Display(DisplayChange::MetallicScale(v)));
                    }
                    if ui.small_button("Reset").clicked() {
                        intents.raise(Intent::Display(DisplayChange::RoughnessScale(1.0)));
                        intents.raise(Intent::Display(DisplayChange::MetallicScale(1.0)));
                    }
                });
                if overridden {
                    ui.label("(disabled in override modes)");
                }
            });

        ui.add_space(8.0);
    });
}
