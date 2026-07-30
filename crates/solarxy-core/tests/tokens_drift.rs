//! Asserts the checked-in `web/src/styles/tokens.generated.css` matches what
//! `solarxy_core::theme` currently produces.
//!
//! The palette is the source of truth for the egui GUI, the analyze TUI AND
//! the web frontend. Changing a color without regenerating means two shells
//! disagree about what "accent" is, which is exactly the failure this module
//! was built to end: before it, the review "change" category shipped green on
//! desktop and error-red on web. That is caught here, as a failing test
//! carrying the regeneration command, rather than by a user noticing that a
//! pin and its panel chip are different colors.
//!
//! Mirrors `solarxy-graph/tests/registry_drift.rs`.

use std::path::PathBuf;

use solarxy_core::theme::{Palette, generate_css, prim};

fn workspace_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(|p| p.parent())
        .map(PathBuf::from)
        .expect("workspace root")
}

fn tokens_path() -> PathBuf {
    workspace_root().join("web/src/styles/tokens.generated.css")
}

/// Compared as text, byte for byte: the checked-in file must be exactly what
/// the generator emits.
#[test]
fn generated_tokens_match_disk() {
    let generated = generate_css();
    let path = tokens_path();
    let on_disk =
        std::fs::read_to_string(&path).expect("web/src/styles/tokens.generated.css must exist");

    assert_eq!(
        on_disk.trim(),
        generated.trim(),
        "web/src/styles/tokens.generated.css is stale, so the web frontend and the native \
         shells now disagree about the palette. Regenerate:\n\n  \
         cargo run -p solarxy-core --example gen_tokens > web/src/styles/tokens.generated.css\n"
    );
}

/// The generated file is machine-owned. `tokens.css` must import it rather
/// than redefine the same custom properties, or the cascade decides which
/// palette wins by file order.
#[test]
fn tokens_css_imports_the_generated_file() {
    let path = workspace_root().join("web/src/styles/tokens.css");
    let css = std::fs::read_to_string(&path).expect("web/src/styles/tokens.css must exist");

    assert!(
        css.contains("tokens.generated.css"),
        "tokens.css must @import tokens.generated.css; without it the generated palette is \
         never loaded and every shell silently falls back to whatever tokens.css defines"
    );
}

/// Nothing the palette owns may be re-declared by the hand-authored
/// stylesheets.
///
/// This is a cascade trap, not a style nit: `main.tsx` imports `tokens.css`
/// BEFORE `styles.css`, and both target the same `:root` / `body.*-theme`
/// selectors, so a copy in `styles.css` has equal specificity and wins on
/// source order. The generated palette would then be silently dead. That is
/// exactly how `--cat-change` came to be green on desktop and error-red on
/// web while both claimed a shared source.
///
/// The legacy alias block is fine and deliberately not flagged: it points
/// AT the semantics (`--text-primary: var(--ink-primary)`) rather than
/// redefining them.
#[test]
fn hand_authored_css_does_not_redefine_generated_tokens() {
    let styles = std::fs::read_to_string(workspace_root().join("web/src/styles.css"))
        .expect("web/src/styles.css must exist");

    let palette = Palette::dark();
    let owned: Vec<&str> = palette
        .roles
        .entries()
        .into_iter()
        .map(|(name, _)| name)
        .chain(palette.review.entries().into_iter().map(|(name, _)| name))
        .collect();

    let offenders: Vec<&str> = owned
        .into_iter()
        .filter(|name| styles.contains(&format!("\n  --{name}:")))
        .collect();

    assert!(
        offenders.is_empty(),
        "styles.css redefines these generated tokens. tokens.css imports first, so these copies \
         win on source order and the generated palette is dead: {offenders:?}"
    );
}

/// The landing page keeps its own stylesheet, deliberately: it is a front
/// door mirroring koljam.com rather than app chrome, its vocabulary is MPW's
/// (`--paper`, `--ink`) rather than the app's semantic roles, and its dark
/// mode is the website's warm ground rather than the app's neutral grey.
///
/// But its LIGHT values are the same MPW palette the app's light theme now
/// uses. That overlap is the one place the two can silently disagree, so it
/// is pinned here rather than left to a comment nobody re-reads. If this
/// fails, the two are no longer the same cream and somebody must decide
/// which is right.
#[test]
fn landing_light_values_match_the_palette() {
    let css = std::fs::read_to_string(workspace_root().join("web/src/landing/landing.css"))
        .expect("web/src/landing/landing.css must exist");

    // Only the `:root` block; the dark override below it is expected to differ.
    let light = css
        .split("@media (prefers-color-scheme: dark)")
        .next()
        .expect("a :root block before the dark override");

    for (landing_var, primitive) in [
        ("--paper", prim::W_050),
        ("--paper-raised", prim::W_000),
        ("--paper-sunken", prim::W_150),
        ("--ink", prim::W_900),
        ("--ink-secondary", prim::W_700),
        ("--hairline", prim::W_200),
        ("--accent-ink", prim::CLAY_600),
    ] {
        let want = format!("{landing_var}: {};", primitive.css());
        assert!(
            light.contains(&want),
            "landing.css and the shared palette have drifted apart: expected `{want}`.\n\
             The landing's light theme and the app's light theme are the same MPW cream; \
             if this changed on purpose, update whichever side is now wrong."
        );
    }
}

/// The public pages' shared stylesheet is deliberately PARALLEL to the
/// landing's self-contained `landing.css` rather than extracted from it: the
/// landing shipped first and its bytes are the ones already published. Two
/// hand-maintained copies of one editorial vocabulary are a real drift risk,
/// closed here mechanically. `base.css`'s light values are pinned to the same
/// palette primitives `landing.css` is pinned to above, its two dark blocks
/// (the system preference and the explicit toggle) must stay identical to
/// each other, and every token both files define must resolve to the same
/// value in each theme. Change a colour in one and this names the other.
#[test]
fn landing_and_base_css_agree() {
    use std::collections::BTreeMap;

    fn decls(chunk: &str) -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        for line in strip_comments(chunk).lines() {
            let line = line.trim();
            let Some(rest) = line.strip_prefix("--") else {
                continue;
            };
            let Some((name, value)) = rest.split_once(':') else {
                continue;
            };
            let value = value
                .trim()
                .trim_end_matches(';')
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            out.insert(format!("--{}", name.trim()), value);
        }
        out
    }

    /// The body of the first `{ ... }` block following `marker`.
    fn block_after<'a>(css: &'a str, marker: &str) -> &'a str {
        let start = css
            .find(marker)
            .unwrap_or_else(|| panic!("`{marker}` not found"));
        let open = css[start..].find('{').expect("an opening brace") + start;
        let mut depth = 0usize;
        for (i, c) in css[open..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return &css[open + 1..open + i];
                    }
                }
                _ => {}
            }
        }
        panic!("unclosed block after `{marker}`");
    }

    const DARK: &str = "@media (prefers-color-scheme: dark)";
    let root = workspace_root();
    let landing = std::fs::read_to_string(root.join("web/src/landing/landing.css"))
        .expect("web/src/landing/landing.css must exist");
    let base = std::fs::read_to_string(root.join("web/src/public/base.css"))
        .expect("web/src/public/base.css must exist");

    let landing_light = decls(landing.split(DARK).next().expect("a light block"));
    let base_light = decls(base.split(DARK).next().expect("a light block"));

    // The same palette cream the landing is pinned to above.
    for (var, primitive) in [
        ("--paper", prim::W_050),
        ("--paper-raised", prim::W_000),
        ("--paper-sunken", prim::W_150),
        ("--ink", prim::W_900),
        ("--ink-secondary", prim::W_700),
        ("--hairline", prim::W_200),
        ("--accent-ink", prim::CLAY_600),
    ] {
        let want = primitive.css();
        assert_eq!(
            base_light.get(var),
            Some(&want),
            "base.css and the shared palette have drifted apart on `{var}`; the public pages \
             and the app's light theme must stay the same cream"
        );
    }

    // The two dark blocks exist so an explicit light choice can opt out of
    // the system preference; they must carry identical values.
    let base_dark_media = decls(block_after(&base, ":root:not([data-theme=\"light\"])"));
    let base_dark_explicit = decls(block_after(&base, ":root[data-theme=\"dark\"]"));
    assert_eq!(
        base_dark_media, base_dark_explicit,
        "base.css's system-preference dark block and its explicit dark block have diverged; \
         the theme toggle now produces different colours than the OS preference"
    );

    // Every token the two files share must agree, in both themes.
    let landing_dark = decls(block_after(&landing, DARK));
    for (theme, ours, theirs) in [
        ("light", &landing_light, &base_light),
        ("dark", &landing_dark, &base_dark_media),
    ] {
        let disagreements: Vec<String> = ours
            .iter()
            .filter_map(|(name, landing_value)| {
                theirs
                    .get(name)
                    .filter(|base_value| *base_value != landing_value)
                    .map(|base_value| {
                        format!(
                            "{name} ({theme}): landing `{landing_value}` vs base `{base_value}`"
                        )
                    })
            })
            .collect();
        assert!(
            disagreements.is_empty(),
            "landing.css and base.css disagree on shared tokens; the landing and the public \
             pages must read as one site:\n  {}",
            disagreements.join("\n  ")
        );
    }
}

/// Strip block comments: prose naming a token must not read as a use.
fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        match rest[start + 2..].find("*/") {
            Some(end) => rest = &rest[start + 2 + end + 2..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// Every `var(--token)` in the app's stylesheets must resolve to a token that
/// something actually defines.
///
/// This guards a bug class that shipped unnoticed for months, because a
/// missing custom property neither throws nor warns — it silently takes the
/// fallback, or drops the declaration entirely:
///
///   - `--hover-bg`, used in three places, defined nowhere: every hover rode a
///     hardcoded `rgba(128,128,128,0.12)` that ignored the theme.
///   - `--selection-bg`: a hardcoded blue, same story.
///   - `--radial-seg-*`: the whole radial menu was pinned to a slice of the
///     retired Ayu palette, which would have been a navy blob on the cream
///     light theme.
///   - `--bg-primary` / `--bg-secondary` / `--bg-elevated`: used with NO
///     fallback, so the WebGPU-unsupported page, the React error boundary and
///     the gizmo readout all rendered with a transparent background.
#[test]
fn every_css_var_resolves_to_a_defined_token() {
    use std::collections::{BTreeMap, BTreeSet};

    // The landing page is a separate bundle with its own vocabulary; it is
    // covered by `landing_light_values_match_the_palette` instead.
    let files = [
        "web/src/styles.css",
        "web/src/styles/tokens.css",
        "web/src/styles/tokens.generated.css",
    ];

    let mut defined: BTreeSet<String> = BTreeSet::new();
    let mut used: BTreeMap<String, &str> = BTreeMap::new();

    for rel in files {
        let css = strip_comments(
            &std::fs::read_to_string(workspace_root().join(rel))
                .unwrap_or_else(|e| panic!("{rel}: {e}")),
        );

        // A declaration follows `{` or `;` or a line start. Not line-anchored:
        // the per-category node fills are written inline
        // (`.flow-node.cat-import { --cat-fill: ...; }`).
        for (i, _) in css.match_indices("--") {
            let tail = &css[i..];
            // Underscores are legal in custom property names and appear in
            // the category fills (`--node-cat-tex_generate`, named after the
            // registry's snake_case category ids).
            let name_len = tail[2..]
                .find(|c: char| {
                    !c.is_ascii_lowercase() && !c.is_ascii_digit() && c != '-' && c != '_'
                })
                .unwrap_or(0);
            let name = &tail[..2 + name_len];
            let after = tail[2 + name_len..].trim_start();

            let before = css[..i].trim_end();
            let is_decl = after.starts_with(':')
                && (before.is_empty() || before.ends_with('{') || before.ends_with(';'));
            if is_decl {
                defined.insert(name.to_string());
            }
            if before.ends_with("var(") {
                used.insert(name.to_string(), rel);
            }
        }
    }

    let missing: Vec<String> = used
        .iter()
        // `--dv-*` are dockview's own; `--pane-tint` is set imperatively from
        // dock/panels.tsx per pane header.
        .filter(|(k, _)| {
            !defined.contains(*k) && !k.starts_with("--dv-") && k.as_str() != "--pane-tint"
        })
        .map(|(k, at)| format!("{k} (used in {at})"))
        .collect();

    assert!(
        missing.is_empty(),
        "these CSS custom properties are used but never defined, so they silently fall back and \
         ignore the theme:\n  {}\n\nDefine the colour in the Rust palette \
         (crates/solarxy-core/src/theme.rs) and regenerate, or point the call site at an \
         existing role.",
        missing.join("\n  ")
    );
}

/// No native `<select>` may return to the frontend.
///
/// They were replaced in 0.7.1 because there was no `select` styling at all:
/// they rendered as OS controls, so they ignored the theme entirely — system
/// grey against warm cream paper, with a platform-drawn popup list that no
/// token could reach. A new one would silently reintroduce that, since it
/// looks perfectly fine on whichever OS its author happens to use.
#[test]
fn the_frontend_has_no_native_select() {
    fn walk(dir: &std::path::Path, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            if path.is_dir() {
                // `wasm/pkg` is generated; `Select.tsx` documents what it replaced.
                if name != "wasm" && name != "node_modules" {
                    walk(&path, out);
                }
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("tsx") || name == "Select.tsx" {
                continue;
            }
            let Ok(src) = std::fs::read_to_string(&path) else {
                continue;
            };
            for (i, line) in src.lines().enumerate() {
                if line.contains("<select") {
                    out.push(format!("{name}:{}", i + 1));
                }
            }
        }
    }

    let mut offenders = Vec::new();
    walk(&workspace_root().join("web/src"), &mut offenders);
    assert!(
        offenders.is_empty(),
        "native <select> elements ignore the theme and draw an OS popup no token can reach. \
         Use `components/Select.tsx` instead: {offenders:?}"
    );
}

/// Every tour step's `target` selector must name a class that exists in
/// web/src.
///
/// A selector matching nothing is silently skipped at runtime. That is
/// correct for a panel the user docked away and wrong for a typo, and the
/// two are indistinguishable in the browser: the first draft shipped a
/// "viewport" step whose `.viewport-panel` matched nothing, so its fallback
/// spotlit the node canvas instead. Renames must fail loudly, here.
#[test]
fn tour_steps_point_at_real_classes() {
    let root = workspace_root();
    let steps = std::fs::read_to_string(root.join("web/src/components/tour/steps.ts"))
        .expect("tour steps must exist");

    // Collect every class name web/src mentions, from .tsx/.ts/.css alike.
    let mut haystack = String::new();
    fn walk(dir: &std::path::Path, out: &mut String) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            if path.is_dir() {
                if name != "wasm" && name != "node_modules" {
                    walk(&path, out);
                }
            } else if matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("ts" | "tsx" | "css")
            ) {
                // The script itself (and its test) must not be in the
                // haystack: a selector trivially "exists" in the very line
                // that declares it, which made the first version of this
                // test unable to fail at all.
                if name == "steps.ts" || name == "steps.test.ts" {
                    continue;
                }
                if let Ok(src) = std::fs::read_to_string(&path) {
                    out.push_str(&src);
                    out.push('\n');
                }
            }
        }
    }
    walk(&root.join("web/src"), &mut haystack);

    let mut missing = Vec::new();
    for line in steps.lines() {
        let Some(rest) = line.trim().strip_prefix("target: \"") else {
            continue;
        };
        let Some(selector) = rest.split('"').next() else {
            continue;
        };
        for sel in selector.split(',') {
            let class = sel.trim().trim_start_matches('.');
            if class.is_empty() {
                continue;
            }
            // A real class appears either quoted in TSX or as a .class rule.
            let quoted = format!("\"{class}");
            let rule = format!(".{class}");
            if !haystack.contains(&quoted) && !haystack.contains(&rule) {
                missing.push(sel.trim().to_string());
            }
        }
    }

    assert!(
        missing.is_empty(),
        "these tour targets match no class in web/src, so the step is silently skipped or a \
         fallback spotlights the wrong panel: {missing:?}"
    );
}

/// Every glyph key the registry declares must have dedicated art in the
/// frontend's `GLYPH_PATHS`.
///
/// The frontend deliberately falls back to a category icon for an unknown
/// key (the zero-frontend-change contract), which is correct behaviour and
/// a terrible default state: the 25 context-expansion node types shipped
/// with NO dedicated art and nobody noticed, because the fallback renders
/// something plausible. A new node must fail here until its glyph is drawn,
/// or consciously reuse an existing key.
#[test]
fn every_registry_glyph_key_has_dedicated_art() {
    let root = workspace_root();
    let registry: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("schemas/registry.json")).expect("registry.json"),
    )
    .expect("registry.json parses");

    let visual = std::fs::read_to_string(root.join("web/src/flow/nodeVisual.ts"))
        .expect("nodeVisual.ts must exist");
    // GLYPH_PATHS entries: `  key:` or `  key_name:` lines inside the map.
    let glyph_block = visual
        .split("GLYPH_PATHS")
        .nth(1)
        .and_then(|s| s.split("};").next())
        .expect("GLYPH_PATHS block");

    let mut missing = Vec::new();
    for node in registry["nodes"].as_array().expect("nodes array") {
        let glyph = node["glyph"].as_str().expect("glyph key");
        let type_id = node["typeId"].as_str().expect("typeId");
        let declared = format!("\n  {glyph}:");
        if !glyph_block.contains(&declared) {
            missing.push(format!("{type_id} (glyph key `{glyph}`)"));
        }
    }

    assert!(
        missing.is_empty(),
        "{} node type(s) declare a glyph key with no dedicated art in GLYPH_PATHS, so they \
         render the generic category icon on the canvas:\n  {}",
        missing.len(),
        missing.join("\n  ")
    );
}

/// The frontend's expression affordance must offer exactly the param types
/// the engine accepts an expression on.
///
/// Two failure modes, both silent without this gate. Offering the `=`
/// affordance on a type the engine refuses gives the user a control whose
/// every use is rejected. Omitting it from a type the engine accepts hides
/// a capability that works perfectly well, which is how a feature ships
/// half-wired and nobody notices.
///
/// Read as text on both sides rather than through a snapshot, because
/// `accepts_expression` is a predicate rather than serialized data: there
/// is nothing in `registry.json` to compare against. Mirrors the glyph gate
/// above, which is the house pattern for a Rust-to-TypeScript contract.
#[test]
fn expression_types_match_the_frontend() {
    let root = workspace_root();

    // Rust: the match arms of ParamType::accepts_expression.
    let param_spec =
        std::fs::read_to_string(root.join("crates/solarxy-graph/src/registry/param_spec.rs"))
            .expect("param_spec.rs must exist");
    // Bounded by the next item, not by brace matching: the following
    // method also lists every ParamType variant, so an unbounded slice
    // would silently claim the engine accepts all of them.
    let body = param_spec
        .split("pub fn accepts_expression")
        .nth(1)
        .and_then(|s| s.split("pub fn").next())
        .expect("accepts_expression body");
    let mut rust: Vec<String> = body
        .split("ParamType::")
        .skip(1)
        .filter_map(|s| {
            let name: String = s
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric())
                .collect();
            (!name.is_empty()).then(|| lower_camel(&name))
        })
        .collect();
    rust.sort();
    rust.dedup();

    // TypeScript: the EXPRESSION_TYPES array.
    let lane = std::fs::read_to_string(root.join("web/src/components/inputs/expressionLane.ts"))
        .expect("expressionLane.ts must exist");
    let block = lane
        .split("export const EXPRESSION_TYPES = [")
        .nth(1)
        .and_then(|s| s.split(']').next())
        .expect("EXPRESSION_TYPES block");
    let mut ts: Vec<String> = block
        .split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect();
    ts.sort();
    ts.dedup();

    assert!(
        !rust.is_empty(),
        "parsed no types out of accepts_expression"
    );
    assert_eq!(
        rust, ts,
        "ParamType::accepts_expression and EXPRESSION_TYPES disagree.\n\
         Rust: {rust:?}\nTypeScript: {ts:?}\n\
         Update web/src/components/inputs/expressionLane.ts to match."
    );
}

/// `Vec2` -> `vec2`, `Color` -> `color`: the wire spelling the frontend
/// uses for a param type.
fn lower_camel(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// The published player must not carry a UI framework it never renders with.
///
/// 0.8.1 shipped a player that pulled **431 KB of React and dockview** into
/// every exported scene bundle, because one line imported a zustand-backed
/// store for a single default number: zustand pulls React, and Vite has React
/// inside the `dock` chunk. Every gate was green through it. The CI wasm
/// budget is a hard gate but the JS figure is only a log line, and it sums all
/// pages together, so a player-only regression is invisible there.
///
/// This is a SOURCE-level rule, deliberately: it runs on every `cargo test`
/// with no build required, and it names the mistake rather than a byte count
/// that would drift. The player reads its defaults from the dependency-free
/// `store/displayDefaults.ts`.
#[test]
fn the_player_does_not_import_the_editors_ui_graph() {
    let root = workspace_root();
    let player_dir = root.join("web/src/player");
    assert!(
        player_dir.is_dir(),
        "web/src/player must exist; the published bundle is built from it"
    );

    // Value imports only. A `import type { ... }` is erased at build time and
    // costs a published bundle nothing, which is exactly how `engine/client.ts`
    // is allowed to reference the prefs types.
    let banned = [
        (
            "../store/prefs",
            "the preferences store is zustand-backed, and zustand pulls React",
        ),
        ("zustand", "pulls React into a page that renders no React"),
        ("react", "the player has no components"),
        ("dockview", "the player has no docking"),
        (
            "../dock/",
            "the dock module is the editor's layout, not the player's",
        ),
        // 0.8.1: CodeMirror is the editor's wrangle field. A published
        // scene shows no parameters at all, so a code editor reaching the
        // player would be the 431 KB React mistake a second time.
        (
            "@codemirror",
            "a published scene has no editable parameters",
        ),
        ("@lezer", "CodeMirror's parser layer, same reasoning"),
    ];

    let mut offenders = Vec::new();
    for entry in std::fs::read_dir(&player_dir)
        .expect("read web/src/player")
        .flatten()
    {
        let path = entry.path();
        if !path.extension().is_some_and(|e| e == "ts" || e == "tsx") {
            continue;
        }
        let Ok(src) = std::fs::read_to_string(&path) else {
            continue;
        };
        let file = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        for (i, line) in src.lines().enumerate() {
            let trimmed = line.trim_start();
            if !trimmed.starts_with("import ") || trimmed.starts_with("import type ") {
                continue;
            }
            for (needle, why) in banned {
                if line.contains(needle) {
                    offenders.push(format!("{file}:{} imports `{needle}` ({why})", i + 1));
                }
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "the published player would ship the editor's UI graph:\n  {}\n\n\
         Read a default from `web/src/store/displayDefaults.ts` (no imports) \
         rather than from the store, or use `import type` if you only need the \
         shape.",
        offenders.join("\n  ")
    );
}

/// The `file://` guard has to survive as an INLINE CLASSIC script.
///
/// Opening a bundle's index.html by double-clicking is the first thing most
/// people try, and a browser refuses to load an ES module from a `file://`
/// origin. The player's own error handling lives inside that module, so when
/// it is blocked nothing runs: the page sits on "Loading..." while the console
/// fills with CORS errors. An inline classic script still executes, which is
/// the whole reason this one is not in `main.ts`.
#[test]
fn the_player_page_warns_about_file_urls_without_needing_its_module() {
    let root = workspace_root();
    let html = std::fs::read_to_string(root.join("web/player.html")).expect("web/player.html");

    let guard = html
        .find("location.protocol")
        .expect("the file:// guard must be present in player.html");
    let status = html
        .find("id=\"player-status\"")
        .expect("the status element the guard writes to");
    assert!(
        status < guard,
        "the guard must come after the element it writes to, or getElementById returns null"
    );

    // It must not be a module: a module would fail for the same reason as the
    // thing it is reporting on.
    let open = html[..guard]
        .rfind("<script")
        .expect("an enclosing script tag");
    assert!(
        !html[open..guard].contains("type=\"module\""),
        "the file:// guard must be a CLASSIC script; a module cannot run when \
         module loading is what failed"
    );

    // And it must say the useful thing rather than merely detecting.
    for needle in ["served over HTTP", "http.server"] {
        assert!(
            html.contains(needle),
            "the guard should tell the reader how to fix it (missing: {needle:?})"
        );
    }
}

/// The comment content of one line, given whether a block comment was already
/// open, plus whether one is still open afterwards.
///
/// Deliberately crude about string literals: a `//` inside a quoted URL is
/// read as a comment. That costs nothing, because the only thing the caller
/// looks for is a planning code, and no URL carries one.
fn comment_text(line: &str, mut in_block: bool) -> (String, bool) {
    let mut out = String::new();
    let mut i = 0;
    while i < line.len() {
        if in_block {
            if let Some(end) = line[i..].find("*/") {
                out.push_str(&line[i..i + end]);
                out.push(' ');
                i += end + 2;
                in_block = false;
            } else {
                out.push_str(&line[i..]);
                break;
            }
        } else if line[i..].starts_with("//") {
            out.push_str(&line[i + 2..]);
            break;
        } else if line[i..].starts_with("/*") {
            in_block = true;
            i += 2;
        } else {
            i += line[i..].chars().next().map_or(1, char::len_utf8);
        }
    }
    (out, in_block)
}

/// Every milestone planning code in a stretch of comment text.
///
/// Three families, each matched only at a word boundary so an identifier or a
/// hyphenated word cannot trip it:
///
/// - work-item codes, `W` then a digit then an optional lowercase letter
///   (`W0a`, `W1e`, `W3`),
/// - decision codes, one of `M D R C P` then a hyphen then digits (`M-3`,
///   `D-24`),
/// - `stage` / `phase` / `milestone` followed by a number, with at most one
///   space or hyphen between (`Stage 8`, `phase-17`, `pre-Phase-10`).
///
/// Version references are deliberately NOT matched. "a blob persisted before
/// 0.8.1" is why a migration fallback exists, and a reader needs the version
/// to reason about the stored artifact.
fn planning_codes(text: &str) -> Vec<String> {
    fn is_word(c: char) -> bool {
        c.is_alphanumeric() || c == '_'
    }

    let chars: Vec<char> = text.chars().collect();
    let mut hits = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        // Every family below is anchored to the start of a word.
        if i > 0 && is_word(chars[i - 1]) {
            i += 1;
            continue;
        }

        // Work-item codes, in both shapes the planning documents have used:
        // `W0a` / `W3` and the hyphenated `W-C1`.
        if chars[i] == 'W' {
            let mut j = i + 1;
            // `W-C1`: a hyphen, an uppercase letter, then digits. Requiring
            // uppercase keeps the palette's `w-100` token names legal.
            if chars.get(j) == Some(&'-') && chars.get(j + 1).is_some_and(char::is_ascii_uppercase)
            {
                j += 2;
                let digits = j;
                while chars.get(j).is_some_and(char::is_ascii_digit) {
                    j += 1;
                }
                if j > digits && !chars.get(j).copied().is_some_and(is_word) {
                    hits.push(chars[i..j].iter().collect());
                    i = j;
                    continue;
                }
                j = i + 1;
            }
            if chars.get(j).is_some_and(char::is_ascii_digit) {
                j += 1;
                if chars.get(j).is_some_and(char::is_ascii_lowercase) {
                    j += 1;
                }
                if !chars.get(j).copied().is_some_and(is_word) {
                    hits.push(chars[i..j].iter().collect());
                    i = j;
                    continue;
                }
            }
        }

        // Decision codes.
        if matches!(chars[i], 'M' | 'D' | 'R' | 'C' | 'P') && chars.get(i + 1) == Some(&'-') {
            let mut j = i + 2;
            while chars.get(j).is_some_and(char::is_ascii_digit) {
                j += 1;
            }
            if j > i + 2 && !chars.get(j).copied().is_some_and(is_word) {
                hits.push(chars[i..j].iter().collect());
                i = j;
                continue;
            }
        }

        // Numbered stages and phases.
        let mut matched = None;
        for word in ["milestone", "stage", "phase"] {
            let end = i + word.chars().count();
            if end > chars.len() {
                continue;
            }
            let candidate: String = chars[i..end].iter().collect();
            if !candidate.eq_ignore_ascii_case(word) {
                continue;
            }
            let mut j = end;
            if chars.get(j).is_some_and(|c| *c == ' ' || *c == '-') {
                j += 1;
            }
            if chars.get(j).is_some_and(char::is_ascii_digit) {
                // Consume the whole number, so the report names `Phase-10`
                // rather than truncating it to `Phase-1`.
                while chars.get(j).is_some_and(char::is_ascii_digit) {
                    j += 1;
                }
                matched = Some(j);
                break;
            }
        }
        if let Some(end) = matched {
            hits.push(chars[i..end].iter().collect());
            i = end;
            continue;
        }

        i += 1;
    }
    hits
}

/// Milestone planning codes must not survive in code comments.
///
/// A work-item code (`W3b`), a numbered stage or phase (`Stage 8`,
/// `pre-Phase-10`) and a decision code (`M-3`) are all ephemeral process
/// artifacts from a planning document, and a comment carrying one is
/// unreadable without that document open beside the file. The decision codes
/// are worse than merely opaque: they are per-milestone, so `decision M-3`
/// names three different decisions in three different files depending on
/// which spec the reader guesses at.
///
/// The fix is never to delete the sentence. It is to say what the code stood
/// for, so the comment carries its own meaning: `pre-Phase-10 desk shape`
/// becomes `the legacy desk shape, as stored before docking gained per-group
/// layouts`.
///
/// A SOURCE-level rule, like the player import guard above: it runs on every
/// `cargo test` with no build, and it names the offending line rather than a
/// count that would drift. Planning codes remain correct and expected in
/// `Docs/`, which this rule does not scan.
#[test]
fn no_planning_codes_in_comments() {
    fn walk(dir: &std::path::Path, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
        paths.sort();
        for path in paths {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_string();
            if path.is_dir() {
                // `wasm` is wasm-bindgen output; `fixtures` is test data, not
                // prose anyone reads.
                if !matches!(
                    name.as_str(),
                    "wasm" | "node_modules" | "target" | "fixtures"
                ) {
                    walk(&path, out);
                }
                continue;
            }
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or_default();
            if !matches!(ext, "rs" | "wgsl" | "ts" | "tsx" | "css") {
                continue;
            }
            // This file names every banned pattern in order to ban it, exactly
            // as `Select.tsx` is exempt from the native-select rule.
            if name == "tokens_drift.rs" {
                continue;
            }
            let Ok(src) = std::fs::read_to_string(&path) else {
                continue;
            };
            let mut in_block = false;
            for (i, line) in src.lines().enumerate() {
                let (text, next) = comment_text(line, in_block);
                in_block = next;
                // A test title is not a comment but is read like one, in CI
                // output, so it is held to the same rule. `describe("viewport
                // tools (phase 11)")` is exactly as opaque as the comment
                // version and would otherwise be the loophole.
                let title = line.trim_start();
                let is_title = ["describe(", "it(", "test("]
                    .iter()
                    .any(|kw| title.starts_with(kw));
                for code in planning_codes(&text) {
                    out.push(format!("{}:{} `{code}`", path.display(), i + 1));
                }
                if is_title {
                    for code in planning_codes(line) {
                        out.push(format!(
                            "{}:{} `{code}` (in a test title)",
                            path.display(),
                            i + 1
                        ));
                    }
                }
            }
        }
    }

    let root = workspace_root();
    let mut offenders = Vec::new();
    for dir in ["crates", "src", "web/src"] {
        walk(&root.join(dir), &mut offenders);
    }

    assert!(
        offenders.is_empty(),
        "{} comment(s) carry a milestone planning code, which means nothing without the \
         planning document open beside the file:\n  {}\n\n\
         Say what the code stood for instead of naming it. A work-item code drops and \
         leaves the noun it labelled; a numbered stage or phase becomes a description of \
         the change; a decision code becomes the decision's substance. Version \
         references are allowed and should stay.",
        offenders.len(),
        offenders.join("\n  ")
    );
}

/// The rule above is proven to fire rather than assumed to.
///
/// A matcher that silently matched nothing would let the whole convention rot
/// while reporting green, which is the failure mode every guard in this file
/// exists because of.
#[test]
fn the_planning_code_matcher_fires_on_real_examples() {
    // Real lines this codebase carried before the sweep.
    for bad in [
        "/// The W2a contract: topology crosses into the renderer contract",
        "//! W0b: the cost of an expression dependency graph (0.8.1 milestone).",
        "/* Stage 8: the Tree panel (searchable scene outline) */",
        "/** The pre-Phase-10 desk shape, as stored by an existing user. */",
        "//! **Foundation only** (decision M-11). There is a clock,",
        "// The graph/list switch (D-24): a right-side icon command",
        "// absent here: a pre-Stage-8 payload must keep the historical ramp.",
        "/* ---- W-C1: pastel category fills ---- */",
    ] {
        let (text, _) = comment_text(bad, false);
        assert!(
            !planning_codes(&text).is_empty(),
            "the matcher missed a planning code in: {bad}"
        );
    }

    // Prose that must stay legal: version references, graphics vocabulary
    // that happens to contain these words, and hyphenated words.
    for good in [
        "/// Null for a scene saved before 0.8.1, which is why the fallback exists.",
        "// The vertex stage builds the varying as transpose(...).",
        "// A W3C-compliant colour string; the D-pad maps to the arrow keys.",
        "// Phases of the moon are not a concern here.",
        "/* The Tree panel (searchable scene outline) */",
        "// 0.7.1 collapsed three themes into two.",
        "// Cook-Torrance specular, sRGB-to-linear on import.",
        "// The palette's w-100 and w-950 tokens are not work-item codes.",
    ] {
        let (text, _) = comment_text(good, false);
        assert!(
            planning_codes(&text).is_empty(),
            "the matcher false-positived on: {good} -> {:?}",
            planning_codes(&text)
        );
    }

    // Code outside a comment is never inspected.
    let (text, _) = comment_text("let stage_8 = phase_17(W0a);", false);
    assert!(planning_codes(&text).is_empty(), "code is not a comment");

    // A test title is checked as a whole line, because the rule reads it
    // directly rather than through `comment_text`.
    assert!(
        !planning_codes("describe(\"viewport tools (phase 11)\", () => {").is_empty(),
        "a planning code in a test title must be caught"
    );
}
