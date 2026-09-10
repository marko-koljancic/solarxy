//! One control per parameter type.
//!
//! **Dispatched from the declaration, never from the node.** A control
//! chosen by node identity would make a new node type need desktop work,
//! which is the thing this release is proving it does not. The match below
//! is exhaustive with no catch-all, so a `ParamType` added in Rust fails
//! the build here rather than drawing a blank row. The browser prints the
//! unknown type's name in that case (`ParameterPanel.tsx`'s `default`
//! arm), which is a reasonable fallback for a shipped surface and a poor
//! one for the contract this release is trying to hold.
//!
//! ## What a widget writes, and what it must do first
//!
//! **A numeric widget clamps to the hard range before it writes.** The
//! engine's `SetParam` conforms the value to the declared type and does
//! not clamp; the clamp lives in the *resolver*, on the read side. So an
//! out-of-range write is stored verbatim, the row reads it back and shows
//! it, and the cook resolves something else: the field says nine and the
//! geometry is four, with nothing anywhere reporting a problem. The
//! browser's `NumberField` clamps for exactly this reason, and its
//! `VectorInput` passes only a step, which leaves a typed component
//! unclamped while a scalar beside it is clamped. That inconsistency is
//! not worth reproducing, so the vectors clamp here too.
//!
//! **A slider appears only where the declaration gives a soft range**, and
//! it appears *beside* the field rather than instead of it, which is the
//! browser's `FloatInput` layout. `ParamSpec::hard`'s own doc comment says
//! the hard range doubles as the slider range; following the comment
//! rather than the code would put a slider on every ranged parameter,
//! which is a lot of rows that the browser draws as bare fields.

use egui::Ui;
use solarxy_graph::document::NodeId;
use solarxy_graph::params::{AssetId, ParamSource, ParamValue};
use solarxy_graph::registry::NodeTypeDescriptor;
use solarxy_graph::registry::param_spec::{NodePathAccept, ParamSpec, ParamType, Unit};

use super::draft::{self, Draft};
use super::drag::{self, DragKind, NumericDrag};
use crate::gui::theme::Theme;

/// Which family of control a parameter type gets.
///
/// Named apart from the drawing so the contract can be asserted without a
/// frame: every type resolves to a family, and the resolution is
/// exhaustive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ControlKind {
    /// A field, with a slider beside it where a soft range is declared.
    Number,
    Toggle,
    Line,
    /// Prose over several lines.
    Multiline,
    /// A free attribute name.
    Attribute,
    /// A program, in a fixed-width field.
    Snippet,
    /// One labelled field per component.
    Vector(usize),
    Colour,
    Choice,
    /// A staged file.
    Asset,
    /// A press, not a value.
    Action,
    /// A reference to another node.
    NodePath,
}

/// The control a parameter type gets.
///
/// **Exhaustive on purpose.** A variant added in Rust stops this
/// compiling, which is the criterion: a parameter type with no control is
/// a build failure rather than a blank row.
#[must_use]
pub(super) fn control_kind(ty: &ParamType) -> ControlKind {
    match ty {
        ParamType::Float | ParamType::Int => ControlKind::Number,
        ParamType::Bool => ControlKind::Toggle,
        ParamType::Text => ControlKind::Line,
        ParamType::MultilineText => ControlKind::Multiline,
        ParamType::AttributeName => ControlKind::Attribute,
        ParamType::Snippet => ControlKind::Snippet,
        ParamType::Vec2 => ControlKind::Vector(2),
        ParamType::Vec3 => ControlKind::Vector(3),
        ParamType::Vec4 => ControlKind::Vector(4),
        ParamType::Color => ControlKind::Colour,
        ParamType::Enum { .. } => ControlKind::Choice,
        ParamType::AssetRef { .. } => ControlKind::Asset,
        ParamType::Action => ControlKind::Action,
        ParamType::NodePath { .. } => ControlKind::NodePath,
    }
}

/// The value a row shows: the node's own literal, or the declared default.
///
/// An expression source falls through to the default, which is why the
/// expression row is a branch of its own rather than a value this can
/// answer.
#[must_use]
pub(super) fn shown_value(spec: &ParamSpec, stored: Option<&ParamSource>) -> ParamValue {
    match stored {
        Some(ParamSource::Literal(value)) => value.clone(),
        _ => spec.default.clone(),
    }
}

/// The unit suffix a number wears.
#[must_use]
pub(super) fn unit_suffix(unit: Unit) -> &'static str {
    match unit {
        Unit::Degrees => "\u{b0}",
        Unit::Meters => " m",
        Unit::None | Unit::Normalized => "",
    }
}

/// Clamp a number to the declared hard range.
///
/// **Before the write, not after it.** See the module docs: the engine
/// stores what it is given and the resolver clamps on the way out, so an
/// unclamped widget leaves the panel and the cook disagreeing permanently.
#[must_use]
pub(super) fn clamp(value: f64, spec: &ParamSpec) -> f64 {
    spec.range
        .as_ref()
        .map_or(value, |range| value.clamp(range.hard.0, range.hard.1))
}

/// The slider range a numeric parameter offers, or nothing.
///
/// **The soft range alone.** `ParamSpec::hard`'s own doc comment says the
/// hard range doubles as the slider range; the browser's `FloatInput`
/// renders a slider only when `spec.soft` is present, and the registry
/// declares plenty of hard-only parameters that would otherwise sprout
/// one. A rule rather than an inline condition because a frame is the
/// only other way to ask it.
#[must_use]
pub(super) fn slider_range(spec: &ParamSpec) -> Option<(f64, f64)> {
    spec.range.as_ref().and_then(|range| range.soft)
}

/// The drag speed a numeric field moves at.
///
/// The declared step where there is one; otherwise the browser's own
/// fallback, one for an integer and a hundredth for a float.
#[must_use]
pub(super) fn drag_speed(spec: &ParamSpec) -> f64 {
    spec.step.unwrap_or(if matches!(spec.ty, ParamType::Int) {
        1.0
    } else {
        0.01
    })
}

/// What a control asked for.
#[derive(Debug, Clone)]
pub(super) enum ControlEdit {
    /// Values to stream while a gesture is open: no write, no undo entry.
    Preview(Vec<(String, ParamValue)>),
    /// A gesture was abandoned, so its preview has to be dropped.
    Clear(Vec<String>),
    /// Parameters to write together.
    ///
    /// Usually one. It is a list because a single pick can decide two: an
    /// attribute name picked from the offered lanes also retypes the
    /// node's sibling type enum, and writing them together is what makes
    /// that one undo step. The browser writes those as two commands, and
    /// says so, because it has no batched parameter write; this shell has
    /// one.
    Write(Vec<(String, ParamValue)>),
    /// An action parameter was pressed.
    Invoke,
    /// The asset control wants a file chooser.
    ChooseAsset,
}

/// What a control needs that its own declaration does not carry.
pub(super) struct ControlEnv<'a> {
    /// Which node these rows belong to, so a draft can be told from one
    /// on the same parameter of a different node.
    pub node: NodeId,
    /// Node-path candidates, resolved from the **root** graph by the
    /// caller.
    pub candidates: &'a [(NodeId, String)],
    /// Staged asset hashes to the name they were staged under, so a file
    /// restored from a scene reads as its name rather than as a hash.
    pub assets: &'a [(String, String)],
    /// The attribute lanes on the node's upstream geometry, as name and
    /// type key. A courtesy an attribute-name row offers; free text stays
    /// first-class, because reserved names and forward references are both
    /// legal.
    pub lanes: &'a [(String, String)],
    /// Every parameter this node type declares, so an attribute pick can
    /// find the sibling type enum it should retype.
    pub specs: &'a [ParamSpec],
    /// The node's last cook error. A wrangle parse error is a cook error,
    /// which is where a snippet's bad line comes from; there is no second
    /// channel.
    pub error: Option<&'a str>,
    pub theme: Theme,
}

/// One parameter to write.
fn one(spec: &ParamSpec, value: ParamValue) -> ControlEdit {
    ControlEdit::Write(vec![(spec.key.clone(), value)])
}

/// The gesture bundle a numeric control drives.
pub(super) struct Gesture<'a> {
    pub draft: &'a mut Option<Draft>,
    pub drag: &'a mut Option<NumericDrag>,
    pub ctx: solarxy_graph::document::GraphContext,
}

/// Draw one parameter's control and answer what it asked for.
#[allow(clippy::too_many_lines)]
pub(super) fn draw(
    ui: &mut Ui,
    spec: &ParamSpec,
    stored: Option<&ParamSource>,
    env: &ControlEnv<'_>,
    gesture: &mut Gesture<'_>,
) -> Option<ControlEdit> {
    let value = shown_value(spec, stored);
    match control_kind(&spec.ty) {
        ControlKind::Number => {
            let stored = number_of(&value);
            numeric_row(ui, spec, env, gesture, &[stored], 0)
        }
        ControlKind::Toggle => {
            let mut on = matches!(value, ParamValue::Bool(true));
            ui.checkbox(&mut on, "")
                .changed()
                .then(|| one(spec, ParamValue::Bool(on)))
        }
        ControlKind::Choice => {
            let ParamType::Enum { variants } = &spec.ty else {
                return None;
            };
            let current = match &value {
                ParamValue::Enum(key) => key.clone(),
                _ => String::new(),
            };
            let shown = variants
                .iter()
                .find(|v| v.key == current)
                .map_or_else(|| current.clone(), |v| v.label.clone());
            let mut picked = None;
            egui::ComboBox::from_id_salt(ui.id().with(&spec.key))
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    for variant in variants {
                        if ui
                            .selectable_label(variant.key == current, &variant.label)
                            .clicked()
                        {
                            picked = Some(variant.key.clone());
                        }
                    }
                });
            picked.map(|key| one(spec, ParamValue::Enum(key)))
        }
        ControlKind::Vector(size) => {
            let mut parts = match &value {
                ParamValue::Vec2(v) => v.to_vec(),
                ParamValue::Vec3(v) => v.to_vec(),
                ParamValue::Vec4(v) => v.to_vec(),
                _ => vec![0.0; size],
            };
            parts.resize(size, 0.0);
            let mut edit = None;
            ui.horizontal(|ui| {
                for slot in 0..size {
                    ui.label(
                        egui::RichText::new(component_label(slot))
                            .color(env.theme.muted)
                            .size(9.0),
                    );
                    if let Some(asked) = numeric_row(ui, spec, env, gesture, &parts, slot) {
                        edit = Some(asked);
                    }
                }
            });
            edit
        }
        ControlKind::Colour => {
            let mut rgba = match &value {
                ParamValue::Color(c) => *c,
                _ => [0.0, 0.0, 0.0, 1.0],
            };
            // Alpha passes through untouched, as it does on the browser:
            // the picker edits three channels and the fourth stays the
            // parameter's own.
            let mut rgb = [rgba[0], rgba[1], rgba[2]];
            ui.color_edit_button_rgb(&mut rgb).changed().then(|| {
                rgba[0] = rgb[0];
                rgba[1] = rgb[1];
                rgba[2] = rgb[2];
                one(spec, ParamValue::Color(rgba))
            })
        }
        ControlKind::Line => {
            let stored = text_of(&value);
            text_row(ui, spec, env, gesture.draft, &stored, TextShape::Line)
        }
        ControlKind::Attribute => {
            let stored = text_of(&value);
            let mut edit = None;
            ui.horizontal(|ui| {
                edit = text_row(ui, spec, env, gesture.draft, &stored, TextShape::Line);
                // Picking is immediate where typing is drafted, and the
                // asymmetry is deliberate: a pick is a complete choice
                // rather than a half-typed word, so waiting for a blur
                // would only make the list feel broken.
                if let Some(picked) = lane_menu(ui, spec, env) {
                    *gesture.draft = None;
                    edit = Some(picked);
                }
            });
            edit
        }
        ControlKind::Multiline => {
            let stored = text_of(&value);
            text_row(ui, spec, env, gesture.draft, &stored, TextShape::Prose)
        }
        ControlKind::Snippet => {
            let stored = text_of(&value);
            snippet_row(ui, spec, env, gesture.draft, &stored)
        }
        ControlKind::Asset => {
            let hash = match &value {
                ParamValue::Asset(id) => id.0.clone(),
                _ => String::new(),
            };
            let name = asset_label(&hash, env.assets);
            let mut edit = None;
            ui.horizontal(|ui| {
                if ui
                    .small_button(if hash.is_empty() {
                        "Select File"
                    } else {
                        "Change"
                    })
                    .clicked()
                {
                    edit = Some(ControlEdit::ChooseAsset);
                }
                ui.label(egui::RichText::new(name).color(env.theme.muted).size(10.0))
                    .on_hover_text(&hash);
                if !hash.is_empty() && ui.small_button("\u{d7}").on_hover_text("Clear").clicked() {
                    edit = Some(one(spec, ParamValue::Asset(AssetId(String::new()))));
                }
            });
            edit
        }
        ControlKind::NodePath => {
            let current = match &value {
                ParamValue::NodeRef(id) => *id,
                _ => None,
            };
            let shown = match current {
                None => "None".to_string(),
                Some(id) => env
                    .candidates
                    .iter()
                    .find(|(node, _)| *node == id)
                    .map_or_else(
                        // A target that has gone stays selectable and says
                        // so, rather than being normalized away on a
                        // redraw: that would write a document change from
                        // a repaint and lose a reference that restoring
                        // the node would have fixed.
                        || format!("Missing node {}", id.0),
                        |(_, name)| name.clone(),
                    ),
            };
            let mut picked = None;
            egui::ComboBox::from_id_salt(ui.id().with(&spec.key))
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    if ui.selectable_label(current.is_none(), "None").clicked() {
                        picked = Some(ParamValue::NodeRef(None));
                    }
                    for (id, name) in env.candidates {
                        if ui.selectable_label(current == Some(*id), name).clicked() {
                            picked = Some(ParamValue::NodeRef(Some(*id)));
                        }
                    }
                });
            picked.map(|value| one(spec, value))
        }
        ControlKind::Action => ui
            .small_button(&spec.label)
            .clicked()
            .then_some(ControlEdit::Invoke),
    }
}

/// Whether a node may be offered for a node-path parameter.
///
/// **The root graph only**, which is the browser's rule
/// (`ParameterPanel.tsx`'s `NodePathField` reads `selectGraph(s, "root")`).
/// Offering nested containers would give the two shells different
/// candidate lists for the same document.
#[must_use]
pub(super) fn accepts(accept: &NodePathAccept, desc: &NodeTypeDescriptor) -> bool {
    match accept {
        NodePathAccept::Opens(kind) => desc.opens == Some(*kind),
        NodePathAccept::TypeIs(type_id) => desc.type_id == type_id.as_str(),
    }
}

/// What a staged asset reads as: the name it was staged under, else a
/// shortened hash, else that there is no file.
#[must_use]
pub(super) fn asset_label(hash: &str, assets: &[(String, String)]) -> String {
    if hash.is_empty() {
        return "no file".to_string();
    }
    assets.iter().find(|(id, _)| id == hash).map_or_else(
        || {
            let head: String = hash.chars().take(10).collect();
            format!("{head}\u{2026}")
        },
        |(_, name)| name.clone(),
    )
}

/// One numeric field, with its slider where a soft range is declared, and
/// the whole preview-and-commit lane behind it.
///
/// `values` is every component of the parameter and `slot` says which one
/// this field edits, so a vector row calls this once per component and the
/// preview it streams is still a whole value.
fn numeric_row(
    ui: &mut Ui,
    spec: &ParamSpec,
    env: &ControlEnv<'_>,
    gesture: &mut Gesture<'_>,
    values: &[f64],
    slot: usize,
) -> Option<ControlEdit> {
    let int = matches!(spec.ty, ParamType::Int);
    let stored = values.get(slot).copied().unwrap_or_default();
    let owned = gesture
        .drag
        .as_ref()
        .is_some_and(|d| d.owns(env.node, &spec.key) && d.slot() == slot);
    let mut current = drag::shown(gesture.drag.as_ref(), env.node, &spec.key, slot, stored);

    let mut response = None;
    ui.horizontal(|ui| {
        // The slider rides beside the field rather than replacing it, so a
        // soft-ranged parameter can still be typed a value outside the
        // slider's reach and inside its hard range.
        if let Some((low, high)) = slider_range(spec).filter(|_| values.len() == 1) {
            let slider = ui.add(egui::Slider::new(&mut current, low..=high).show_value(false));
            if slider.dragged() || slider.drag_stopped() || slider.changed() {
                response = Some(slider);
            }
        }
        let field = ui.add(
            egui::DragValue::new(&mut current)
                .speed(drag_speed(spec))
                .suffix(unit_suffix(spec.unit)),
        );
        if response.is_none() || field.dragged() || field.drag_stopped() || field.changed() {
            response = Some(field);
        }
    });
    let response = response?;

    // The middle button is this shell's own gesture: egui's drag value
    // reads the primary button, so the decade ladder has to be driven by
    // hand or it does not exist.
    if !owned
        && response.hovered()
        && ui.input(|i| i.pointer.button_pressed(egui::PointerButton::Middle))
        && let Some(origin) = ui.input(|i| i.pointer.latest_pos())
    {
        *gesture.drag = Some(NumericDrag::begin(
            gesture.ctx,
            env.node,
            &spec.key,
            values.to_vec(),
            slot,
            DragKind::Precision {
                origin,
                last_change_y: origin.y,
                decade: drag::DEFAULT_DECADE,
            },
        ));
        return None;
    }
    if owned
        && gesture
            .drag
            .as_ref()
            .and_then(NumericDrag::decade)
            .is_some()
    {
        return advance_precision(ui, spec, env, gesture, int);
    }

    // **A widget gesture ends when the button is up, not only when egui
    // says so.** A pointer released outside the window, or a frame the
    // widget did not see, leaves `drag_stopped` unreported and the gesture
    // open for good, previewing a value nothing will ever commit or clear.
    // The browser closes the same hole with a window-level listener.
    if owned
        && !response.dragged()
        && !ui.input(|i| i.pointer.button_down(egui::PointerButton::Primary))
    {
        return finish(spec, gesture, int);
    }

    // An ordinary drag: open a gesture on the first frame that moves, keep
    // previewing while it is held, and write once when it lets go.
    if response.dragged() {
        let live = clamp(current, spec);
        if !owned {
            *gesture.drag = Some(NumericDrag::begin(
                gesture.ctx,
                env.node,
                &spec.key,
                values.to_vec(),
                slot,
                DragKind::Widget,
            ));
        }
        let held = gesture.drag.as_mut()?;
        held.set(slot, live);
        return Some(ControlEdit::Preview(vec![(
            spec.key.clone(),
            compose(spec, held.values(), int),
        )]));
    }
    if response.drag_stopped() {
        return finish(spec, gesture, int);
    }
    // A typed value, or an arrow key: no gesture, one write.
    if response.changed() {
        let mut parts = values.to_vec();
        if let Some(part) = parts.get_mut(slot) {
            *part = clamp(current, spec);
        }
        return Some(one(spec, compose(spec, &parts, int)));
    }
    None
}

/// Carry a precision drag one frame, and end it on release or on escape.
fn advance_precision(
    ui: &Ui,
    spec: &ParamSpec,
    env: &ControlEnv<'_>,
    gesture: &mut Gesture<'_>,
    int: bool,
) -> Option<ControlEdit> {
    // Escape abandons, and the preview has to be dropped explicitly:
    // nothing else will.
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        let key = spec.key.clone();
        *gesture.drag = None;
        return Some(ControlEdit::Clear(vec![key]));
    }
    if !ui.input(|i| i.pointer.button_down(egui::PointerButton::Middle)) {
        return finish(spec, gesture, int);
    }
    let pointer = ui.input(|i| i.pointer.latest_pos())?;
    let held = gesture.drag.as_mut()?;
    let raw = held.advance(pointer, int);
    held.set(held.slot(), clamp(raw, spec));
    if let Some(rung) = held.decade() {
        draw_ladder(ui, pointer, rung, env.theme);
    }
    Some(ControlEdit::Preview(vec![(
        spec.key.clone(),
        compose(spec, held.values(), int),
    )]))
}

/// The floating decade ladder a precision drag scrubs against.
///
/// Drawn beside the pointer rather than beside the field, because the
/// gesture leaves the field almost immediately and a ladder anchored to
/// the row would be somewhere else by the second rung.
fn draw_ladder(ui: &Ui, pointer: egui::Pos2, selected: usize, theme: Theme) {
    let top = pointer.y - drag::ROW_HEIGHT * (drag::DEFAULT_DECADE as f32 + 0.5);
    egui::Area::new(ui.id().with("precision_ladder"))
        .order(egui::Order::Tooltip)
        .fixed_pos(egui::pos2(pointer.x + 24.0, top))
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for (index, decade) in drag::DECADES.iter().enumerate() {
                    let chosen = index == selected;
                    ui.allocate_ui(egui::vec2(64.0, drag::ROW_HEIGHT), |ui| {
                        ui.label(
                            egui::RichText::new(format!("{decade}"))
                                .monospace()
                                .size(11.0)
                                .color(if chosen { theme.fg } else { theme.muted })
                                .background_color(if chosen {
                                    theme.selection
                                } else {
                                    egui::Color32::TRANSPARENT
                                }),
                        );
                    });
                }
            });
        });
}

/// End a gesture: one write if it moved the value, a cleared preview if it
/// did not.
fn finish(spec: &ParamSpec, gesture: &mut Gesture<'_>, int: bool) -> Option<ControlEdit> {
    let held = gesture.drag.take()?;
    if !held.moved() {
        // A press and release that scrubbed nothing writes nothing, for
        // the same reason an untouched text field does. The preview still
        // has to go: one was streamed the moment the gesture opened.
        return Some(ControlEdit::Clear(vec![spec.key.clone()]));
    }
    Some(one(spec, compose(spec, held.values(), int)))
}

/// Rebuild a whole parameter value from its components.
fn compose(spec: &ParamSpec, parts: &[f64], int: bool) -> ParamValue {
    let at = |i: usize| clamp(parts.get(i).copied().unwrap_or_default(), spec);
    match control_kind(&spec.ty) {
        ControlKind::Vector(2) => ParamValue::Vec2([at(0), at(1)]),
        ControlKind::Vector(3) => ParamValue::Vec3([at(0), at(1), at(2)]),
        ControlKind::Vector(_) => ParamValue::Vec4([at(0), at(1), at(2), at(3)]),
        _ if int =>
        {
            #[allow(clippy::cast_possible_truncation)]
            ParamValue::Int(at(0).round() as i64)
        }
        _ => ParamValue::Float(at(0)),
    }
}

/// A parameter's number, whatever numeric shape it wears.
fn number_of(value: &ParamValue) -> f64 {
    match value {
        ParamValue::Float(v) => *v,
        #[allow(clippy::cast_precision_loss)]
        ParamValue::Int(v) => *v as f64,
        _ => 0.0,
    }
}

/// How a text row is shaped.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TextShape {
    /// One line. Enter is the commit key.
    Line,
    /// Several lines. Enter inserts a newline, so the commit key is the
    /// platform's command modifier with it, and blur commits too.
    Prose,
}

/// A text-like row: draft while typing, write once on the way out.
///
/// See [`super::draft`] for why typing never writes and why a commit
/// compares against what was last sent rather than against storage.
fn text_row(
    ui: &mut Ui,
    spec: &ParamSpec,
    env: &ControlEnv<'_>,
    draft: &mut Option<Draft>,
    stored: &str,
    shape: TextShape,
) -> Option<ControlEdit> {
    let mut text = draft::shown_text(draft.as_ref(), env.node, &spec.key, stored).to_string();
    let rows = text.lines().count().clamp(3, 12);
    let response = match shape {
        TextShape::Line => ui.add(egui::TextEdit::singleline(&mut text).desired_width(160.0)),
        TextShape::Prose => ui.add(
            egui::TextEdit::multiline(&mut text)
                .desired_rows(rows)
                .desired_width(f32::INFINITY),
        ),
    };
    resolve_text(ui, spec, env, draft, stored, shape, &response, text)
}

/// The half of a text row that is state rather than drawing, so the
/// snippet row can reuse it around its own editor.
fn resolve_text(
    ui: &Ui,
    spec: &ParamSpec,
    env: &ControlEnv<'_>,
    draft: &mut Option<Draft>,
    stored: &str,
    shape: TextShape,
    response: &egui::Response,
    text: String,
) -> Option<ControlEdit> {
    if response.changed() {
        if let Some(existing) = draft.as_mut().filter(|d| d.owns(env.node, &spec.key)) {
            existing.set(text);
        } else {
            let mut fresh = Draft::begin(env.node, &spec.key, stored);
            fresh.set(text);
            *draft = Some(fresh);
        }
        return None;
    }
    // Escape abandons. Checked before the commit, because egui surrenders
    // focus on Escape and the commit would otherwise write the very text
    // the user just asked to discard.
    let escaped = ui.input(|i| i.key_pressed(egui::Key::Escape));
    if escaped && (response.has_focus() || response.lost_focus()) {
        *draft = None;
        return None;
    }
    let commit_key = shape == TextShape::Prose
        && response.has_focus()
        && ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter));
    if !(response.lost_focus() || commit_key) {
        return None;
    }
    let out = draft
        .as_mut()
        .filter(|d| d.owns(env.node, &spec.key))
        .and_then(Draft::take_commit)
        .map(|text| one(spec, ParamValue::Text(text)));
    // The draft ends whether or not it wrote, which is what lets a name
    // the engine uniquified come back into the row.
    *draft = None;
    out
}

/// A program's row: a line-number gutter beside the editor.
fn snippet_row(
    ui: &mut Ui,
    spec: &ParamSpec,
    env: &ControlEnv<'_>,
    draft: &mut Option<Draft>,
    stored: &str,
) -> Option<ControlEdit> {
    let text = draft::shown_text(draft.as_ref(), env.node, &spec.key, stored).to_string();
    // **The error belongs to the committed program.** While an edit is in
    // flight the message describes text that is no longer on screen, so
    // marking a line from it would point at the wrong one.
    let dirty = draft.as_ref().is_some_and(|d| d.owns(env.node, &spec.key));
    let bad = (!dirty)
        .then(|| {
            env.error
                .and_then(solarxy_studio::expression::error_position)
        })
        .flatten();
    let rows = text.lines().count().clamp(3, 12);
    let mut edit = None;
    ui.horizontal_top(|ui| {
        let mut buffer = text;
        let gutter = rows.max(buffer.lines().count());
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for line in 1..=gutter {
                let marked = bad.as_ref().is_some_and(|p| p.line == line);
                ui.label(
                    egui::RichText::new(format!("{line:>3}"))
                        .monospace()
                        .size(10.0)
                        .color(if marked {
                            env.theme.severity_error
                        } else {
                            env.theme.muted
                        }),
                );
            }
        });
        let response = ui.add(
            egui::TextEdit::multiline(&mut buffer)
                .desired_rows(rows)
                .desired_width(f32::INFINITY)
                .code_editor(),
        );
        edit = resolve_text(
            ui,
            spec,
            env,
            draft,
            stored,
            TextShape::Prose,
            &response,
            buffer,
        );
    });
    if let Some(message) = env.error.filter(|_| bad.is_some()) {
        ui.label(
            egui::RichText::new(message)
                .color(env.theme.severity_error)
                .size(9.0),
        );
    }
    edit
}

/// The lanes an attribute-name row offers, and what a pick writes.
fn lane_menu(ui: &mut Ui, spec: &ParamSpec, env: &ControlEnv<'_>) -> Option<ControlEdit> {
    if env.lanes.is_empty() {
        return None;
    }
    let mut picked = None;
    ui.menu_button("\u{25be}", |ui| {
        for (name, ty) in env.lanes {
            if ui.button(format!("{name}  {ty}")).clicked() {
                picked = Some((name.clone(), ty.clone()));
                ui.close();
            }
        }
    })
    .response
    .on_hover_text("Attribute lanes on this node's input");
    let (name, ty) = picked?;
    let mut writes = vec![(spec.key.clone(), ParamValue::Text(name))];
    // A pick keeps the lane's type without a second edit, and both writes
    // travel together so the pick is one undo step.
    if let Some(sibling) = sibling_type_param(env.specs, spec, &ty) {
        writes.push((sibling.key.clone(), ParamValue::Enum(ty)));
    }
    Some(ControlEdit::Write(writes))
}

/// The enum parameter a lane pick should retype: key `type`, in the **same
/// group** as the name parameter, declaring a variant equal to the lane's
/// type.
///
/// Nothing when the node declares no such parameter, and then a pick fills
/// only the name. Typing a name by hand never touches it: a half-typed
/// lane name matches nothing, and retyping the node from it would be a
/// guess.
#[must_use]
pub(super) fn sibling_type_param<'a>(
    specs: &'a [ParamSpec],
    name: &ParamSpec,
    lane_ty: &str,
) -> Option<&'a ParamSpec> {
    specs.iter().find(|spec| {
        spec.key == "type"
            && spec.group == name.group
            && matches!(&spec.ty, ParamType::Enum { variants }
                if variants.iter().any(|v| v.key == lane_ty))
    })
}

fn component_label(index: usize) -> &'static str {
    ["X", "Y", "Z", "W"].get(index).copied().unwrap_or("?")
}

fn text_of(value: &ParamValue) -> String {
    match value {
        ParamValue::Text(text) => text.clone(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solarxy_graph::registry::param_spec::{EnumVariant, ParamRange};

    fn registry() -> solarxy_graph::registry::Registry {
        solarxy_graph::nodes::builtin_registry().expect("builtin registry")
    }

    fn float_spec() -> ParamSpec {
        ParamSpec::new(
            "size",
            "Size",
            "general",
            ParamType::Float,
            ParamValue::Float(1.0),
        )
    }

    /// Every variant of the vocabulary resolves to a control.
    ///
    /// The match is exhaustive, so this cannot rot: a sixteenth variant
    /// stops the crate compiling. What the list adds is the count, which
    /// is the figure the milestone states.
    #[test]
    fn every_parameter_type_has_a_control() {
        let every = [
            ParamType::Float,
            ParamType::Int,
            ParamType::Bool,
            ParamType::Text,
            ParamType::MultilineText,
            ParamType::AttributeName,
            ParamType::Snippet,
            ParamType::Vec2,
            ParamType::Vec3,
            ParamType::Vec4,
            ParamType::Color,
            ParamType::Enum {
                variants: vec![EnumVariant::new("a", "A")],
            },
            ParamType::AssetRef {
                accept: vec![".obj".to_string()],
            },
            ParamType::Action,
            ParamType::NodePath {
                accept: NodePathAccept::TypeIs("camera".to_string()),
            },
        ];
        assert_eq!(every.len(), 15, "the vocabulary is fifteen types");
        // The three vector widths are three controls, because a vector's
        // width is how many fields it draws.
        assert_eq!(control_kind(&ParamType::Vec2), ControlKind::Vector(2));
        assert_eq!(control_kind(&ParamType::Vec3), ControlKind::Vector(3));
        assert_eq!(control_kind(&ParamType::Vec4), ControlKind::Vector(4));
        // And the three text-shaped types are three controls rather than
        // one, even though all three store `ParamValue::Text`: the storage
        // is what they share, the widget is what they declare.
        assert_ne!(
            control_kind(&ParamType::Text),
            control_kind(&ParamType::MultilineText)
        );
        assert_ne!(
            control_kind(&ParamType::MultilineText),
            control_kind(&ParamType::Snippet)
        );
        assert_ne!(
            control_kind(&ParamType::Text),
            control_kind(&ParamType::AttributeName)
        );
    }

    /// Every type the shipped registry actually declares is reachable.
    ///
    /// Driven off the real registry rather than a fabricated node, in both
    /// directions: a type declared by no node would be a control nothing
    /// can reach, and a declared type with no control is the failure the
    /// exhaustive match already refuses.
    #[test]
    fn the_shipped_registry_declares_every_one_of_them() {
        let registry = registry();
        let mut declared = std::collections::BTreeSet::new();
        for desc in registry.descriptors() {
            for spec in &desc.params {
                declared.insert(spec.ty.describe());
            }
        }
        assert_eq!(
            declared.len(),
            15,
            "the registry declares {} of the fifteen types: {declared:?}",
            declared.len()
        );
    }

    /// A slider appears only where a soft range is declared.
    ///
    /// `ParamSpec::hard`'s own doc comment says the hard range doubles as
    /// the slider range. Following the comment rather than the browser
    /// would put a slider on parameters the browser draws as bare fields,
    /// and the registry declares plenty of them.
    #[test]
    fn a_hard_range_alone_is_not_a_slider() {
        let free = float_spec();
        assert_eq!(slider_range(&free), None, "no range at all, no slider");

        let hard_only = float_spec().hard(0.0, 100.0);
        assert_eq!(
            slider_range(&hard_only),
            None,
            "a hard range is validity, not a slider"
        );

        let both = float_spec().hard(0.0, 100.0).soft(0.0, 10.0);
        assert_eq!(slider_range(&both), Some((0.0, 10.0)));

        // And the distinction is one the shipped registry actually makes,
        // in both directions: a rule that only ever sees one case is a
        // rule nothing exercises.
        let registry = registry();
        let (hard, soft) = registry.descriptors().flat_map(|d| d.params.iter()).fold(
            (0, 0),
            |(hard, soft), spec| match spec.range.as_ref() {
                Some(r) if r.soft.is_some() => (hard, soft + 1),
                Some(_) => (hard + 1, soft),
                None => (hard, soft),
            },
        );
        assert!(
            hard > 0 && soft > 0,
            "the registry declares {hard} hard-only and {soft} soft ranges, \
             so the rule that separates them is not exercised by what ships"
        );
    }

    /// A widget clamps before it writes.
    ///
    /// `SetParam` conforms and stores; the clamp is in the resolver, on
    /// the read side. An unclamped widget therefore leaves a value in the
    /// document that the row shows and the cook never uses.
    #[test]
    fn a_value_is_clamped_to_the_declared_hard_range() {
        let mut spec = float_spec();
        spec.range = Some(ParamRange {
            hard: (0.5, 4.0),
            soft: None,
        });
        assert!((clamp(9.0, &spec) - 4.0).abs() < f64::EPSILON);
        assert!((clamp(-9.0, &spec) - 0.5).abs() < f64::EPSILON);
        assert!((clamp(2.0, &spec) - 2.0).abs() < f64::EPSILON);

        // The engine really does store the unclamped value, which is why
        // the widget has to do this. Asserted against the engine rather
        // than assumed, because if it ever starts clamping on write, this
        // clamp becomes belt and braces rather than the only one.
        let stored = solarxy_graph::registry::resolve::conform_value(
            &ParamValue::Float(9.0),
            &ParamType::Float,
        )
        .expect("a float conforms to a float");
        assert_eq!(stored, ParamValue::Float(9.0), "the write does not clamp");
        let read =
            solarxy_graph::registry::resolve::conform_and_clamp(&stored, &spec).expect("conforms");
        assert_eq!(read, ParamValue::Float(4.0), "the read does clamp");

        // No range declared means nothing to clamp to, not clamping to
        // zero.
        assert!((clamp(1e9, &float_spec()) - 1e9).abs() < f64::EPSILON);
    }

    /// A drag moves by the declared step, and by the browser's fallback
    /// where none is declared.
    #[test]
    fn a_drag_moves_by_the_declared_step() {
        let mut spec = float_spec();
        assert!((drag_speed(&spec) - 0.01).abs() < f64::EPSILON);
        spec.ty = ParamType::Int;
        assert!((drag_speed(&spec) - 1.0).abs() < f64::EPSILON);
        spec.step = Some(5.0);
        assert!((drag_speed(&spec) - 5.0).abs() < f64::EPSILON);
    }

    /// A row shows the node's own literal, or the declared default, and an
    /// expression falls through to the default rather than reading as a
    /// value.
    #[test]
    fn a_row_shows_the_override_or_the_default() {
        let spec = float_spec();
        assert_eq!(shown_value(&spec, None), ParamValue::Float(1.0));
        assert_eq!(
            shown_value(&spec, Some(&ParamSource::Literal(ParamValue::Float(3.0)))),
            ParamValue::Float(3.0)
        );
        assert_eq!(
            shown_value(
                &spec,
                Some(&ParamSource::Expression {
                    expr: "1 + 1".to_string()
                })
            ),
            ParamValue::Float(1.0),
            "an expression is not a value and falls through to the default"
        );
    }

    /// The unit suffix is the browser's, including that a normalized
    /// parameter wears none.
    #[test]
    fn the_unit_suffix_matches_the_browsers() {
        assert_eq!(unit_suffix(Unit::Degrees), "\u{b0}");
        assert_eq!(unit_suffix(Unit::Meters), " m");
        assert_eq!(unit_suffix(Unit::None), "");
        assert_eq!(unit_suffix(Unit::Normalized), "");
    }

    /// A node-path parameter offers exactly what its filter allows.
    #[test]
    fn a_node_path_offers_only_what_its_filter_allows() {
        let registry = registry();
        let opens_mat = NodePathAccept::Opens(solarxy_graph::document::ContextKind::Mat);
        let offered: Vec<&str> = registry
            .descriptors()
            .filter(|d| accepts(&opens_mat, d))
            .map(|d| d.type_id)
            .collect();
        assert!(
            !offered.is_empty(),
            "nothing opens a material network, so the filter is untested"
        );
        for type_id in &offered {
            let desc = registry.get(type_id).expect("just listed");
            assert_eq!(desc.opens, Some(solarxy_graph::document::ContextKind::Mat));
        }

        let by_type = NodePathAccept::TypeIs("camera".to_string());
        let cameras: Vec<&str> = registry
            .descriptors()
            .filter(|d| accepts(&by_type, d))
            .map(|d| d.type_id)
            .collect();
        assert_eq!(cameras, vec!["camera"]);

        // The two filters are not interchangeable: a camera opens no
        // network, so a picker asking for one gets nothing from the other.
        assert!(!accepts(
            &opens_mat,
            registry
                .get("camera")
                .expect("the camera type is registered")
        ));
    }

    /// Picking an attribute lane also retypes the node, where the node
    /// declares something to retype.
    ///
    /// Driven off the real registry in **both** directions: two shipped
    /// types pair a lane name with a sibling type enum, six declare a lane
    /// name with no such sibling, and a rule that fired on all eight or on
    /// none of them would pass a test written only one way round.
    #[test]
    fn a_lane_pick_retypes_the_node_only_where_the_node_says_how() {
        let registry = registry();
        let mut paired = Vec::new();
        let mut alone = Vec::new();
        for desc in registry.descriptors() {
            for spec in &desc.params {
                if !matches!(spec.ty, ParamType::AttributeName) {
                    continue;
                }
                // Every lane a wrangle can produce is one of these; the
                // pick carries the lane's own type.
                let found = ["float", "vec2", "vec3", "vec4"]
                    .iter()
                    .find_map(|ty| sibling_type_param(&desc.params, spec, ty));
                if found.is_some() {
                    paired.push(desc.type_id);
                } else {
                    alone.push(desc.type_id);
                }
            }
        }
        assert!(
            !paired.is_empty() && !alone.is_empty(),
            "the registry pairs {paired:?} and leaves {alone:?} alone, \
             so one direction of this rule is not exercised"
        );

        // And the three conditions are each load-bearing, which the
        // registry cannot show because no shipped node violates one.
        let name = ParamSpec::new(
            "attr_name",
            "Attribute",
            "attribute",
            ParamType::AttributeName,
            ParamValue::Text(String::new()),
        );
        let enum_spec = |key: &str, group: &str, variant: &str| {
            ParamSpec::new(
                key,
                "Type",
                group,
                ParamType::Enum {
                    variants: vec![EnumVariant::new(variant, variant)],
                },
                ParamValue::Enum(variant.to_string()),
            )
        };
        let right = [enum_spec("type", "attribute", "vec3")];
        assert!(sibling_type_param(&right, &name, "vec3").is_some());
        // A different key is a different parameter.
        let wrong_key = [enum_spec("kind", "attribute", "vec3")];
        assert!(sibling_type_param(&wrong_key, &name, "vec3").is_none());
        // A `type` in another group belongs to another family of rows.
        let wrong_group = [enum_spec("type", "displace", "vec3")];
        assert!(sibling_type_param(&wrong_group, &name, "vec3").is_none());
        // And a type the enum cannot express is not written as one.
        assert!(
            sibling_type_param(&right, &name, "quaternion").is_none(),
            "a lane type the sibling does not offer must not be written to it"
        );
    }

    /// A staged file reads as its name, an unstaged hash as a shortened
    /// hash, and an empty one as no file at all.
    #[test]
    fn an_asset_reads_as_its_name_when_the_engine_still_has_it() {
        let staged = [("abc123def456789".to_string(), "dragon.obj".to_string())];
        assert_eq!(asset_label("abc123def456789", &staged), "dragon.obj");
        assert_eq!(asset_label("", &staged), "no file");
        // A hash the manifest does not know still says something, because
        // a scene can carry a reference whose bytes are not staged yet.
        let orphan = asset_label("0123456789abcdef", &staged);
        assert!(
            orphan.starts_with("0123456789") && orphan != "no file",
            "an unstaged hash reads as {orphan}"
        );
    }
}
