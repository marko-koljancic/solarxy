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
