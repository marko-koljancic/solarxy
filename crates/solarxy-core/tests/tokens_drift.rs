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
