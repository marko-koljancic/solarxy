use super::MOD;

#[derive(Default)]
pub(super) struct KeyboardShortcutsModalState {
    pub open: bool,
}

struct Entry {
    keys: &'static str,
    action: &'static str,
}

struct Section {
    title: &'static str,
    entries: &'static [Entry],
}

const SECTIONS: &[Section] = &[
    Section {
        title: "File",
        entries: &[
            Entry {
                keys: "__MOD__+O",
                action: "Open model",
            },
            Entry {
                keys: "__MOD__+Shift+O",
                action: "Import HDRI",
            },
            Entry {
                keys: "C",
                action: "Save screenshot",
            },
            Entry {
                keys: "Shift+S",
                action: "Save preferences",
            },
            Entry {
                keys: "__MOD__+,",
                action: "Preferences",
            },
        ],
    },
    Section {
        title: "Window & Layout",
        entries: &[
            Entry {
                keys: "F1",
                action: "Single viewport",
            },
            Entry {
                keys: "F2",
                action: "Split vertical",
            },
            Entry {
                keys: "F3",
                action: "Split horizontal",
            },
            Entry {
                keys: "F10",
                action: "Toggle menu bar",
            },
            Entry {
                keys: "F11",
                action: "Toggle fullscreen",
            },
            Entry {
                keys: "Tab",
                action: "Toggle sidebar",
            },
            Entry {
                keys: "`",
                action: "Toggle console",
            },
            Entry {
                keys: "?",
                action: "Open this Keyboard Shortcuts window",
            },
            Entry {
                keys: "Esc",
                action: "Dismiss open modal",
            },
        ],
    },
    Section {
        title: "Navigation",
        entries: &[
            Entry {
                keys: "Left drag",
                action: "Orbit camera",
            },
            Entry {
                keys: "Middle / Shift+drag",
                action: "Pan",
            },
            Entry {
                keys: "Wheel",
                action: "Zoom",
            },
            Entry {
                keys: "H",
                action: "Frame model",
            },
            Entry {
                keys: "V",
                action: "Toggle turntable",
            },
            Entry {
                keys: "__MOD__+L",
                action: "Link cameras (split view)",
            },
            Entry {
                keys: "P / O",
                action: "Perspective / Orthographic",
            },
        ],
    },
    Section {
        title: "Shading & Inspection",
        entries: &[
            Entry {
                keys: "S",
                action: "Shaded",
            },
            Entry {
                keys: "X",
                action: "Ghosted",
            },
            Entry {
                keys: "1",
                action: "Inspection: Shaded",
            },
            Entry {
                keys: "2",
                action: "Inspection: Material ID",
            },
            Entry {
                keys: "3",
                action: "Inspection: UV Map",
            },
            Entry {
                keys: "4",
                action: "Inspection: Texel Density",
            },
            Entry {
                keys: "5",
                action: "Inspection: Depth",
            },
            Entry {
                keys: "M / Shift+M",
                action: "Cycle material override",
            },
        ],
    },
    Section {
        title: "Show / Overlays",
        entries: &[
            Entry {
                keys: "G",
                action: "Grid",
            },
            Entry {
                keys: "A",
                action: "Axis gizmo",
            },
            Entry {
                keys: "Shift+A",
                action: "Local axes",
            },
            Entry {
                keys: "N",
                action: "Cycle normals mode",
            },
            Entry {
                keys: "U",
                action: "Cycle UV overlay",
            },
            Entry {
                keys: "B",
                action: "Cycle background",
            },
            Entry {
                keys: "Shift+B",
                action: "Cycle bounds mode",
            },
            Entry {
                keys: "Shift+W",
                action: "Cycle wireframe weight",
            },
            Entry {
                keys: "Shift+V",
                action: "Validation overlay",
            },
        ],
    },
    Section {
        title: "Lighting & Post-Processing",
        entries: &[
            Entry {
                keys: "I",
                action: "Cycle IBL",
            },
            Entry {
                keys: "Shift+I",
                action: "Cycle IBL mode",
            },
            Entry {
                keys: "Shift+L",
                action: "Lock lights",
            },
            Entry {
                keys: "Shift+D",
                action: "Toggle bloom",
            },
            Entry {
                keys: "Shift+O",
                action: "Toggle SSAO",
            },
            Entry {
                keys: "Shift+T",
                action: "Cycle tone mapping",
            },
            Entry {
                keys: "E / Shift+E",
                action: "Exposure \u{00B1} / reset",
            },
        ],
    },
];

pub(super) fn draw_keyboard_shortcuts_modal(
    ctx: &egui::Context,
    state: &mut KeyboardShortcutsModalState,
) {
    if !state.open {
        return;
    }

    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        state.open = false;
        return;
    }

    let mut open = state.open;
    let default_pos = ctx.content_rect().center() - egui::vec2(240.0, 280.0);

    egui::Window::new("Keyboard Shortcuts")
        .open(&mut open)
        .resizable(true)
        .collapsible(false)
        .default_width(480.0)
        .default_height(560.0)
        .default_pos(default_pos)
        .movable(true)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (i, section) in SECTIONS.iter().enumerate() {
                    if i > 0 {
                        ui.add_space(2.0);
                        ui.separator();
                    }
                    ui.add_space(4.0);
                    ui.heading(section.title);
                    ui.add_space(4.0);
                    egui::Grid::new(format!("shortcuts_grid_{}", section.title))
                        .num_columns(2)
                        .spacing([16.0, 4.0])
                        .show(ui, |ui| {
                            for entry in section.entries {
                                let keys = entry.keys.replace("__MOD__", MOD);
                                ui.label(
                                    egui::RichText::new(keys)
                                        .monospace()
                                        .color(egui::Color32::from_rgb(255, 220, 120)),
                                );
                                ui.label(entry.action);
                                ui.end_row();
                            }
                        });
                    ui.add_space(4.0);
                }
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new("User-remappable shortcuts land in 0.6.0.")
                        .small()
                        .italics()
                        .color(egui::Color32::from_white_alpha(140)),
                );
            });
        });

    state.open = open && state.open;
}
