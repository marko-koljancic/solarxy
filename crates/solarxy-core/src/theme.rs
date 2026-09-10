//! The interface palette: one definition, three shells.
//!
//! Every surface that draws Solarxy chrome reads its colors from here: the
//! egui desktop GUI, the analyze TUI, and (via generated CSS) the web
//! frontend. Before this module the four surfaces each hand-authored their
//! own values and drifted apart; the review "change" category ended up
//! green on desktop and error-red on web.
//!
//! # Two tiers
//!
//! [`prim::PRIMITIVES`] is tier 1: a raw ramp with no UI meaning. [`Roles`] is
//! tier 2: the semantic layer everything actually reads (`surface_app`,
//! `ink_primary`, `accent`). A role either points at a named primitive or
//! carries a literal. This mirrors the web's `tokens.css` architecture so
//! the generated CSS keeps its `var(--n-800)` indirection instead of
//! flattening to hex.
//!
//! # Regenerating the CSS
//!
//! ```text
//! cargo run -p solarxy-core --example gen_tokens > web/src/styles/tokens.generated.css
//! ```
//!
//! `tests/tokens_drift.rs` asserts the checked-in file matches this
//! module, so changing a color without regenerating fails CI.
//!
//! No egui, no ratatui, no wgpu, and no feature gate: this is plain data
//! that every shell can reach.

/// An 8-bit-per-channel sRGB color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    /// Build from a `0xRRGGBB` literal.
    pub const fn hex(v: u32) -> Self {
        Self {
            r: ((v >> 16) & 0xff) as u8,
            g: ((v >> 8) & 0xff) as u8,
            b: (v & 0xff) as u8,
        }
    }

    /// The `#rrggbb` form the generated CSS emits.
    pub fn css(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    pub const fn to_array(self) -> [u8; 3] {
        [self.r, self.g, self.b]
    }
}

macro_rules! primitives {
    ($($name:literal => $konst:ident = $hex:literal),* $(,)?) => {
        $(pub const $konst: Rgb = Rgb::hex($hex);)*

        /// Every tier-1 primitive in declaration order, as (CSS custom
        /// property name without the leading `--`, value).
        pub const PRIMITIVES: &[(&str, Rgb)] = &[$(($name, $konst)),*];
    };
}

/// Tier 1: the raw ramps. No UI meaning; roles point at these.
pub mod prim {
    use super::Rgb;

    primitives! {
        // Neutral ramp, dark UI.
        "n-900" => N_900 = 0x1e1e1e,
        "n-850" => N_850 = 0x242424,
        "n-800" => N_800 = 0x2a2a2a,
        "n-750" => N_750 = 0x333333,
        "n-700" => N_700 = 0x3a3a3a,
        "n-600" => N_600 = 0x404040,
        "n-550" => N_550 = 0x4a4a4a,
        "n-500" => N_500 = 0x555555,
        "n-400" => N_400 = 0x999999,
        "n-300" => N_300 = 0x888888,
        "n-200" => N_200 = 0xaaaaaa,
        "n-100" => N_100 = 0xe0e0e0,
        "n-000" => N_000 = 0xffffff,

        // Warm paper ramp, light UI. The MPW "Balanced Editorial" palette
        // from koljam.com: cream paper, near-black ink.
        "w-000" => W_000 = 0xfbf9f4,
        "w-050" => W_050 = 0xf4f1ea,
        "w-100" => W_100 = 0xefebe0,
        "w-150" => W_150 = 0xece7dc,
        "w-200" => W_200 = 0xd8d2c6,
        "w-300" => W_300 = 0xc9c2b4,
        "w-350" => W_350 = 0xc4bdae,
        "w-400" => W_400 = 0xa09a8c,
        "w-500" => W_500 = 0x7a756a,
        "w-700" => W_700 = 0x4a463e,
        "w-900" => W_900 = 0x11110e,
        "w-950" => W_950 = 0x0a0a08,

        // Amber: the dark theme's accent family (Ayu).
        "amber-300" => AMBER_300 = 0xffb454,
        "amber-400" => AMBER_400 = 0xe6b450,
        "amber-600" => AMBER_600 = 0xd9a13f,

        // Terracotta: the light theme's accent family (MPW). The deep clay
        // is the only AA-safe text accent on cream (~5.5:1).
        "clay-500" => CLAY_500 = 0xb05a3a,
        "clay-600" => CLAY_600 = 0x9a4a2e,
        "clay-700" => CLAY_700 = 0x83402a,

        // Burnt orange: the attention family. Never used without a shape,
        // which is what keeps it apart from the light accent's hue.
        "burnt-400" => BURNT_400 = 0xe07a3f,
        "burnt-600" => BURNT_600 = 0xb85c1e,

        // Houdini blue: the display family.
        "disp-400" => DISP_400 = 0x58a6ff,
        "disp-600" => DISP_600 = 0x1f6feb,

        // Signal hues.
        "err-400" => ERR_400 = 0xe5484d,
        "err-600" => ERR_600 = 0xd1373c,
        "ok-400" => OK_400 = 0x55dd99,
        "ok-600" => OK_600 = 0x3aa76d,
    }
}

/// One tier-2 semantic role.
#[derive(Debug, Clone, Copy)]
pub struct Role {
    pub rgb: Rgb,
    /// The tier-1 primitive this role names, when it is a direct
    /// reference; `None` when the role is authored as a literal. Only the
    /// CSS generator reads this; Rust consumers want [`Role::rgb`].
    pub primitive: Option<&'static str>,
}

impl Role {
    /// A role that points at a named primitive. The name and value are
    /// cross-checked by `every_role_reference_resolves`.
    const fn of(name: &'static str, rgb: Rgb) -> Self {
        Self {
            rgb,
            primitive: Some(name),
        }
    }

    /// A role with no tier-1 equivalent.
    const fn lit(hex: u32) -> Self {
        Self {
            rgb: Rgb::hex(hex),
            primitive: None,
        }
    }
}

macro_rules! roles {
    ($($field:ident => $css:literal),* $(,)?) => {
        /// Tier 2: the semantic layer. Every shell reads these, never the
        /// primitives directly.
        #[derive(Debug, Clone, Copy)]
        pub struct Roles {
            $(pub $field: Role,)*
        }

        impl Roles {
            /// Every role in declaration order, as (CSS custom property
            /// name without the leading `--`, role). Drives the generator.
            pub fn entries(&self) -> Vec<(&'static str, Role)> {
                vec![$(($css, self.$field)),*]
            }
        }
    };
}

roles! {
    surface_canvas => "surface-canvas",
    surface_dot => "surface-dot",
    surface_app => "surface-app",
    surface_raised => "surface-raised",
    surface_overlay => "surface-overlay",
    surface_sunken => "surface-sunken",

    ink_primary => "ink-primary",
    ink_secondary => "ink-secondary",
    ink_tertiary => "ink-tertiary",
    ink_muted => "ink-muted",
    ink_strong => "ink-strong",
    ink_on_accent => "ink-on-accent",
    ink_on_pastel => "ink-on-pastel",

    border_subtle => "border-subtle",
    border_default => "border-default",
    border_strong => "border-strong",
    divider => "divider",

    accent => "accent",
    accent_hover => "accent-hover",
    accent_pressed => "accent-pressed",
    accent_hairline => "accent-hairline",

    state_attention => "state-attention",
    ink_on_attention => "ink-on-attention",

    display => "display",

    status_error => "status-error",
    status_success => "status-success",

    hover_bg => "hover-bg",
    selection => "selection",
    scrim => "scrim",

    node_glyph_ink => "node-glyph-ink",
    node_type_label => "node-type-label",
    node_desc => "node-desc",
    node_param => "node-param",
    node_slot => "node-slot",
}

/// The four review-annotation category colors.
///
/// These are the single strongest visual-correlation cue in the product:
/// the same hue must color a viewport pin and its panel chip, on every
/// shell. Keeping them here is the whole reason this module exists.
#[derive(Debug, Clone, Copy)]
pub struct ReviewColors {
    pub info: Rgb,
    pub warning: Rgb,
    pub question: Rgb,
    pub change: Rgb,
}

impl ReviewColors {
    /// In `AnnotationCategory` order, as (CSS custom property name without
    /// the leading `--`, value).
    pub fn entries(&self) -> Vec<(&'static str, Rgb)> {
        vec![
            ("cat-info", self.info),
            ("cat-warning", self.warning),
            ("cat-question", self.question),
            ("cat-change", self.change),
        ]
    }
}

/// The wire colours: one hue per family of port data types.
///
/// A wire's colour says what family of value travels down it, and the
/// handle's *shape* separates the members of a family, so the encoding
/// survives a reader who cannot tell the hues apart. That is why there are
/// ten of these for thirteen data types: the scalars share a hue and are
/// told apart by a diamond, the vectors share one, and image and material
/// are both hexagons with different hues.
///
/// These live here for the same reason the review categories do. A wire on
/// the browser canvas and the same wire on the desktop canvas must be the
/// one colour, and until 0.10.0 they could not be: the values were hex
/// literals in the browser, copied by hand into two more places, guarded
/// by nothing.
///
/// **Identical in both themes today**, deliberately, because that is what
/// shipped and this release moved them without retuning them. They are a
/// set that accreted one colour at a time rather than one designed as a
/// family, so a later pass may well give the light theme its own values;
/// the shape here already allows that.
#[derive(Debug, Clone, Copy)]
pub struct WireColors {
    pub geometry: Rgb,
    pub light: Rgb,
    pub report: Rgb,
    /// `float` and `int`, told apart by the diamond handle.
    pub scalar: Rgb,
    pub boolean: Rgb,
    /// `vec2`, `vec3` and `vec4`.
    pub vector: Rgb,
    pub color: Rgb,
    pub text: Rgb,
    pub image: Rgb,
    /// A copper hue owned by nothing else. The hexagon groups it with
    /// image as a resource handle, so the hue is what separates them.
    pub material: Rgb,
}

impl WireColors {
    /// The values every theme currently shares.
    const fn shared() -> Self {
        Self {
            geometry: Rgb::hex(0x5aa0ff),
            light: Rgb::hex(0xffcc66),
            report: Rgb::hex(0x4dd0c8),
            scalar: Rgb::hex(0x7fd962),
            boolean: Rgb::hex(0xff8a80),
            vector: Rgb::hex(0xb39ddb),
            color: Rgb::hex(0xf5a623),
            text: Rgb::hex(0x9aa0a6),
            image: Rgb::hex(0xe879c8),
            material: Rgb::hex(0xc96f4a),
        }
    }

    /// In family order, as (CSS custom property name without the leading
    /// `--`, value).
    pub fn entries(&self) -> Vec<(&'static str, Rgb)> {
        vec![
            ("wire-geometry", self.geometry),
            ("wire-light", self.light),
            ("wire-report", self.report),
            ("wire-scalar", self.scalar),
            ("wire-bool", self.boolean),
            ("wire-vector", self.vector),
            ("wire-color", self.color),
            ("wire-text", self.text),
            ("wire-image", self.image),
            ("wire-material", self.material),
        ]
    }
}

/// The pastel body fill a node wears, one per registry category, plus the
/// three a root container wears according to the network it opens.
///
/// These live here for the same reason the wire colours do, and they
/// arrived later for a worse reason: they were hand-authored CSS in the
/// browser, thirty-six declarations across two themes, and the desktop
/// canvas needed the same thirty. Two hand-authored copies of one family
/// is the pair this release exists to remove, and the milestone's own
/// theme rule already said a canvas colour authors here.
///
/// **Keyed by name, not by `Category`.** This crate cannot see the node
/// registry, which sits above it, so the mapping from a category to its
/// fill is `solarxy_studio::types::category_fill`, exactly as
/// `wire_color` maps a data type onto [`WireColors`].
///
/// **Three of them are a container's, chosen by what it opens** rather
/// than by its type id. The browser selects them from a type-id class and
/// therefore has to learn a new one whenever a network kind arrives; the
/// rule here asks the descriptor instead, which is the same correction
/// 0.10.0 applied to the engine.
#[derive(Debug, Clone, Copy)]
pub struct NodeCategoryFills {
    pub generators: Rgb,
    pub attribute: Rgb,
    pub transform: Rgb,
    pub copy: Rgb,
    pub topology: Rgb,
    pub shaders: Rgb,
    pub import: Rgb,
    pub export: Rgb,
    pub utility: Rgb,
    /// Shares its value with the light-node background, so a light node
    /// reads the same on the canvas as it does everywhere else.
    pub lights: Rgb,
    /// A cool slate, deliberately not the lights cream: cameras and lights
    /// sit side by side in the root graph and are the two things placed
    /// there that are not geometry, so they must read apart at a glance.
    pub cameras: Rgb,
    /// A container with no opened kind to go on, and the value the
    /// neutral node background carries.
    pub container: Rgb,
    /// A container that opens a surface network: the neutral tile, so the
    /// commonest container is the quiet one.
    pub container_sop: Rgb,
    /// A container that opens an image network: green-teal, away from
    /// transform's cyan.
    pub container_cop: Rgb,
    /// A container that opens a material network: the shaders rose family
    /// it contains.
    pub container_mat: Rgb,
    pub cop_generate: Rgb,
    pub cop_adjust: Rgb,
    pub cop_composite: Rgb,
}

impl NodeCategoryFills {
    const fn dark() -> Self {
        Self {
            generators: Rgb::hex(0xc9dcf2),
            attribute: Rgb::hex(0xf2cfd6),
            transform: Rgb::hex(0xc5e6e6),
            copy: Rgb::hex(0xd9ecc4),
            topology: Rgb::hex(0xc8e8d6),
            shaders: Rgb::hex(0xf0cfe3),
            import: Rgb::hex(0xddd0ee),
            export: Rgb::hex(0xf4d7b0),
            utility: Rgb::hex(0xeed9c9),
            lights: Rgb::hex(0xf8e7b0),
            cameras: Rgb::hex(0xcfd8e8),
            container: Rgb::hex(0xe5e7eb),
            container_sop: Rgb::hex(0xe5e7eb),
            container_cop: Rgb::hex(0xbfe0d6),
            container_mat: Rgb::hex(0xeecdde),
            cop_generate: Rgb::hex(0xe9ddc3),
            cop_adjust: Rgb::hex(0xcfdce8),
            cop_composite: Rgb::hex(0xd2e4dc),
        }
    }

    const fn light() -> Self {
        Self {
            generators: Rgb::hex(0xdcebfb),
            attribute: Rgb::hex(0xfbdfe6),
            transform: Rgb::hex(0xd8f1f1),
            copy: Rgb::hex(0xe6f5d4),
            topology: Rgb::hex(0xd9f3e4),
            shaders: Rgb::hex(0xf9dcef),
            import: Rgb::hex(0xeae0f7),
            export: Rgb::hex(0xfce4c2),
            utility: Rgb::hex(0xf7e6d7),
            lights: Rgb::hex(0xfff7de),
            cameras: Rgb::hex(0xdee6f3),
            container: Rgb::hex(0xf8f8f8),
            container_sop: Rgb::hex(0xf8f8f8),
            container_cop: Rgb::hex(0xd9f0e8),
            container_mat: Rgb::hex(0xf8def0),
            cop_generate: Rgb::hex(0xf3e9d2),
            cop_adjust: Rgb::hex(0xdfe8f4),
            cop_composite: Rgb::hex(0xdef0e8),
        }
    }

    /// In registry-category order, as (CSS custom property name without
    /// the leading `--`, value).
    ///
    /// The three `cop_` names carry an underscore because the browser
    /// derives the property name from the `snake_case` category id, and a
    /// name that does not match the id resolves to nothing.
    pub fn entries(&self) -> Vec<(&'static str, Rgb)> {
        vec![
            ("node-cat-generators", self.generators),
            ("node-cat-attribute", self.attribute),
            ("node-cat-transform", self.transform),
            ("node-cat-copy", self.copy),
            ("node-cat-topology", self.topology),
            ("node-cat-shaders", self.shaders),
            ("node-cat-import", self.import),
            ("node-cat-export", self.export),
            ("node-cat-utility", self.utility),
            ("node-cat-lights", self.lights),
            ("node-cat-cameras", self.cameras),
            ("node-cat-container", self.container),
            ("node-cat-container-sop", self.container_sop),
            ("node-cat-container-cop", self.container_cop),
            ("node-cat-container-mat", self.container_mat),
            ("node-cat-cop_generate", self.cop_generate),
            ("node-cat-cop_adjust", self.cop_adjust),
            ("node-cat-cop_composite", self.cop_composite),
        ]
    }
}

/// A complete interface palette. `Copy`: pass it by value freely.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    pub roles: Roles,
    pub review: ReviewColors,
    pub wire: WireColors,
    pub node_cat: NodeCategoryFills,
}

impl Palette {
    /// Neutral grey with an amber accent.
    pub const fn dark() -> Self {
        use prim::*;
        Self {
            dark: true,
            roles: Roles {
                surface_canvas: Role::of("n-850", N_850),
                surface_dot: Role::lit(0x3c3c3c),
                surface_app: Role::of("n-800", N_800),
                surface_raised: Role::of("n-750", N_750),
                surface_overlay: Role::of("n-700", N_700),
                surface_sunken: Role::of("n-900", N_900),

                ink_primary: Role::of("n-100", N_100),
                ink_secondary: Role::of("n-200", N_200),
                ink_tertiary: Role::of("n-300", N_300),
                ink_muted: Role::of("n-400", N_400),
                ink_strong: Role::of("n-000", N_000),
                ink_on_accent: Role::lit(0x111111),
                ink_on_pastel: Role::lit(0x1f2937),

                border_subtle: Role::of("n-600", N_600),
                border_default: Role::of("n-550", N_550),
                border_strong: Role::of("n-500", N_500),
                divider: Role::of("n-500", N_500),

                accent: Role::of("amber-400", AMBER_400),
                accent_hover: Role::of("amber-300", AMBER_300),
                accent_pressed: Role::of("amber-600", AMBER_600),
                // The dark accent clears 3:1 on its own, so hairlines can
                // use it directly; the light theme substitutes a deeper one.
                accent_hairline: Role::of("amber-400", AMBER_400),

                state_attention: Role::of("burnt-400", BURNT_400),
                // Dark ink on the bright dark-theme orange (5.6:1).
                ink_on_attention: Role::lit(0x1a1a1a),

                display: Role::of("disp-400", DISP_400),

                status_error: Role::of("err-400", ERR_400),
                status_success: Role::of("ok-400", OK_400),

                hover_bg: Role::of("n-700", N_700),
                // Amber at ~20% over the app surface: the selection reads as
                // selected without borrowing a signal hue.
                selection: Role::lit(0x504632),
                // The ground a modal or the first-run tour dims the app to.
                // Opaque here; the alpha is applied at the call site.
                scrim: Role::lit(0x080806),

                node_glyph_ink: Role::lit(0x1f2937),
                node_type_label: Role::lit(0x8a8f98),
                node_desc: Role::lit(0x5fb3a1),
                node_param: Role::lit(0x7fa8cc),
                node_slot: Role::lit(0x1f2937),
            },
            review: ReviewColors {
                info: Rgb::hex(0x5c9eff),
                warning: Rgb::hex(0xffb23d),
                question: Rgb::hex(0xa06dff),
                // Teal, deliberately. The old desktop green sat on top of
                // the success signal and the old web red was bit-identical
                // to err-400; "change" needs a hue no other state owns.
                change: Rgb::hex(0x2dd4bf),
            },
            wire: WireColors::shared(),
            node_cat: NodeCategoryFills::dark(),
        }
    }

    /// Warm cream paper with a terracotta accent: the MPW "Balanced
    /// Editorial" palette from koljam.com, which is the light theme on
    /// every shell.
    pub const fn light() -> Self {
        use prim::*;
        Self {
            dark: false,
            roles: Roles {
                surface_canvas: Role::of("w-100", W_100),
                surface_dot: Role::of("w-200", W_200),
                surface_app: Role::of("w-050", W_050),
                surface_raised: Role::of("w-000", W_000),
                surface_overlay: Role::of("w-000", W_000),
                surface_sunken: Role::of("w-150", W_150),

                ink_primary: Role::of("w-900", W_900),
                ink_secondary: Role::of("w-700", W_700),
                ink_tertiary: Role::of("w-500", W_500),
                ink_muted: Role::of("w-400", W_400),
                ink_strong: Role::of("w-950", W_950),
                ink_on_accent: Role::of("w-050", W_050),
                ink_on_pastel: Role::lit(0x1f2937),

                border_subtle: Role::of("w-150", W_150),
                border_default: Role::of("w-200", W_200),
                border_strong: Role::of("w-300", W_300),
                divider: Role::of("w-350", W_350),

                accent: Role::of("clay-600", CLAY_600),
                accent_hover: Role::of("clay-500", CLAY_500),
                accent_pressed: Role::of("clay-700", CLAY_700),
                accent_hairline: Role::of("clay-600", CLAY_600),

                state_attention: Role::of("burnt-600", BURNT_600),
                // White on the deeper light-theme orange.
                ink_on_attention: Role::lit(0xffffff),

                display: Role::of("disp-600", DISP_600),

                status_error: Role::of("err-600", ERR_600),
                status_success: Role::of("ok-600", OK_600),

                hover_bg: Role::of("w-150", W_150),
                // Terracotta at ~18% over cream: a warm sand.
                selection: Role::lit(0xe4d3c8),
                // MPW's own scrim ink, so dimming cream paper stays warm
                // rather than going grey.
                scrim: Role::lit(0x11110e),

                node_glyph_ink: Role::lit(0x1f2937),
                node_type_label: Role::lit(0x6b7078),
                node_desc: Role::lit(0x2e7d6b),
                node_param: Role::lit(0x2f6390),
                node_slot: Role::lit(0x1f2937),
            },
            review: ReviewColors {
                // Darkened against the cream ground for AA contrast.
                info: Rgb::hex(0x2563c9),
                warning: Rgb::hex(0xb7791f),
                question: Rgb::hex(0x7c3aed),
                change: Rgb::hex(0x0f766e),
            },
            // The same set as the dark theme. See `WireColors`: they were
            // moved here rather than retuned, and the shape allows a light
            // set later without another migration.
            wire: WireColors::shared(),
            node_cat: NodeCategoryFills::light(),
        }
    }

    pub const fn for_dark(dark: bool) -> Self {
        if dark { Self::dark() } else { Self::light() }
    }
}

/// Render the palette as the web frontend's `tokens.generated.css`.
///
/// Scope is deliberately narrow: only the colors the palette owns. Fonts,
/// motion, spacing, the legacy aliases, and the UX-spec-owned node
/// category pastels stay hand-authored in `tokens.css`, which imports the
/// generated file.
///
/// Driven by `examples/gen_tokens.rs`; pinned by `tests/tokens_drift.rs`.
pub fn generate_css() -> String {
    use std::fmt::Write as _;

    let mut css = String::new();

    // Written line-by-line rather than as one continued literal: `\` at a
    // line end eats the following indentation, which silently strips the
    // ` * ` gutter off every line of the emitted comment.
    for line in [
        "/* GENERATED by `cargo run -p solarxy-core --example gen_tokens`. Do not edit.",
        " *",
        " * The source of truth is `crates/solarxy-core/src/theme.rs`, which also",
        " * feeds the egui desktop GUI and the analyze TUI. Edit the palette there",
        " * and regenerate; `tests/tokens_drift.rs` fails CI otherwise.",
        " *",
        " * Imported by `tokens.css`, which keeps the hand-authored fonts, motion,",
        " * spacing and legacy aliases. */",
        "",
    ] {
        let _ = writeln!(css, "{line}");
    }

    css.push_str(":root {\n");
    css.push_str("  /* ---- Tier 1: primitives (theme-agnostic, no UI meaning) ---------- */\n\n");
    for (name, rgb) in prim::PRIMITIVES {
        let _ = writeln!(css, "  --{name}: {};", rgb.css());
    }
    css.push_str("}\n\n");

    // Dark is the default, so it lands on the bare `:root` and `body` too: a
    // page renders correctly before the prefs store applies a body class.
    css.push_str(":root,\nbody,\nbody.dark-theme {\n");
    write_theme(&mut css, &Palette::dark(), "dark");
    css.push_str("}\n\n");

    css.push_str("body.light-theme {\n");
    write_theme(&mut css, &Palette::light(), "light");
    css.push_str("}\n");

    css
}

fn write_theme(css: &mut String, palette: &Palette, label: &str) {
    use std::fmt::Write as _;

    let _ = writeln!(
        css,
        "  color-scheme: {};\n",
        if palette.dark { "dark" } else { "light" }
    );
    let _ = writeln!(
        css,
        "  /* ---- Tier 2: semantic roles ({label}) ---------------------------- */\n"
    );

    for (name, role) in palette.roles.entries() {
        let value = match role.primitive {
            Some(p) => format!("var(--{p})"),
            None => role.rgb.css(),
        };
        let _ = writeln!(css, "  --{name}: {value};");
    }

    css.push_str("\n  /* Review categories: the same hue colors a viewport pin and its\n");
    css.push_str("   * panel chip, on every shell. */\n");
    for (name, rgb) in palette.review.entries() {
        let _ = writeln!(css, "  --{name}: {};", rgb.css());
    }

    css.push_str("\n  /* Wire colours: one hue per family of port data type. The\n");
    css.push_str("   * handle's SHAPE separates the members of a family, so the\n");
    css.push_str("   * encoding survives a reader who cannot tell the hues apart. */\n");
    for (name, rgb) in palette.wire.entries() {
        let _ = writeln!(css, "  --{name}: {};", rgb.css());
    }

    css.push_str("\n  /* Node body fills: one pastel per registry category, plus the\n");
    css.push_str("   * three a root container wears according to the network it\n");
    css.push_str("   * opens. Dark text stays readable on every one of them. */\n");
    for (name, rgb) in palette.node_cat.entries() {
        let _ = writeln!(css, "  --{name}: {};", rgb.css());
    }

    // Derived rather than authored, so the two stay locked together when the
    // accent moves.
    css.push_str("\n  /* Derived from --accent. */\n");
    let _ = writeln!(
        css,
        "  --accent-subtle: color-mix(in srgb, var(--accent) {}%, transparent);",
        if palette.dark { 14 } else { 12 }
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `Role::of` name must resolve to a primitive holding exactly
    /// that value. Without this a typo would silently emit CSS pointing at
    /// one color while Rust consumers read another, which is precisely the
    /// class of drift this module exists to kill.
    #[test]
    fn every_role_reference_resolves() {
        for palette in [Palette::dark(), Palette::light()] {
            for (css_name, role) in palette.roles.entries() {
                let Some(name) = role.primitive else { continue };
                let found = prim::PRIMITIVES
                    .iter()
                    .find(|(n, _)| *n == name)
                    .unwrap_or_else(|| {
                        panic!("role --{css_name} names unknown primitive --{name}")
                    });
                assert_eq!(
                    found.1, role.rgb,
                    "role --{css_name} points at --{name} but carries a different value",
                );
            }
        }
    }

    #[test]
    fn primitive_names_are_unique() {
        let mut names: Vec<&str> = prim::PRIMITIVES.iter().map(|(n, _)| *n).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(before, names.len(), "duplicate primitive name");
    }

    /// The dark and light palettes must expose the same role set, or a
    /// component styled against one theme loses its color in the other.
    #[test]
    fn both_palettes_carry_the_same_roles() {
        let dark: Vec<&str> = Palette::dark()
            .roles
            .entries()
            .iter()
            .map(|(n, _)| *n)
            .collect();
        let light: Vec<&str> = Palette::light()
            .roles
            .entries()
            .iter()
            .map(|(n, _)| *n)
            .collect();
        assert_eq!(dark, light);
    }

    /// "Change" must not collide with the error or success signal on
    /// either theme. This is the drift that motivated the module: web
    /// shipped `--cat-change` bit-identical to `--err-400`.
    #[test]
    fn review_change_owns_its_own_hue() {
        for palette in [Palette::dark(), Palette::light()] {
            assert_ne!(palette.review.change, palette.roles.status_error.rgb);
            assert_ne!(palette.review.change, palette.roles.status_success.rgb);
        }
    }

    #[test]
    fn css_formats_as_six_digit_hex() {
        assert_eq!(Rgb::hex(0x2a2a2a).css(), "#2a2a2a");
        assert_eq!(Rgb::hex(0x00ff05).css(), "#00ff05");
    }

    #[test]
    fn hex_round_trips_through_channels() {
        let c = Rgb::hex(0x123456);
        assert_eq!(c.to_array(), [0x12, 0x34, 0x56]);
    }

    #[test]
    fn generated_css_emits_every_primitive_and_both_themes() {
        let css = generate_css();
        for (name, _) in prim::PRIMITIVES {
            assert!(css.contains(&format!("--{name}:")), "missing --{name}");
        }
        assert!(css.contains("body.dark-theme"));
        assert!(css.contains("body.light-theme"));
    }

    /// The bug that motivated the module: a token consumed by `styles.css`
    /// in three places but defined nowhere, silently riding a hardcoded
    /// grey fallback that ignored the theme entirely.
    #[test]
    fn generated_css_defines_hover_bg_in_both_themes() {
        assert_eq!(generate_css().matches("--hover-bg:").count(), 2);
    }

    #[test]
    fn generated_css_emits_each_role_once_per_theme() {
        let css = generate_css();
        for (name, _) in Palette::dark().roles.entries() {
            assert_eq!(
                css.matches(&format!("--{name}:")).count(),
                2,
                "--{name} should appear once per theme",
            );
        }
    }

    /// A role naming a primitive must emit the indirection, not a flattened
    /// hex: the two-tier architecture is the point.
    #[test]
    fn generated_css_keeps_the_primitive_indirection() {
        let css = generate_css();
        assert!(css.contains("--surface-app: var(--n-800);"));
        assert!(css.contains("--accent: var(--amber-400);"));
        assert!(css.contains("--accent: var(--clay-600);"));
        // A literal role stays a literal.
        assert!(css.contains("--surface-dot: #3c3c3c;"));
    }
}
