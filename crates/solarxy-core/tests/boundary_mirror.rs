//! The WebAssembly boundary's Rust definitions and the browser's hand-written
//! TypeScript mirror carry the same variants and the same fields.
//!
//! `web/src/engine/types.ts` mirrors roughly eighty Rust types by hand. Rust
//! owns the schema; the TypeScript is a copy. Before this file existed, three
//! JSON spot-checks pinned six of `Command`'s thirty-five variants and one
//! source grep pinned two serde attributes, so a type added or changed in Rust
//! and forgotten in TypeScript compiled, linked, shipped, and failed only when
//! a user reached it.
//!
//! That was not hypothetical. Building this check found three live defects:
//! `ViewStateDto::pane_camera_locked` was mirrored as `paneCameraLock`, so the
//! pane camera-lock toggle read `undefined` and could never be cleared;
//! `DisplaySettings::point_size` reached the wire and no declaration; and
//! `IssueScope` renamed its variants without renaming its fields, which is the
//! documented trap that already cost one release on a neighbouring enum.
//!
//! **This is a source-level scan on both sides, and that is forced rather than
//! chosen.** `HostEvent` lives behind `cfg(target_arch = "wasm32")`, so no
//! native test can construct one, and the TypeScript is not Rust at all.
//! `tokens_drift.rs` already scans source for the same reason and calls the
//! technique the house pattern for a Rust-to-TypeScript contract.
//!
//! **It does not replace the three camelCase assertions** in
//! `solarxy-graph/src/engine/tests.rs`, and deleting them would reopen a
//! failure that has already shipped. This file compares two *declarations* and
//! proves they agree. Those tests serialize a real value and prove the *serde
//! attributes* are right. A declaration can look correct and serialize wrong,
//! which is exactly what `IssueScope` was doing. Two guards, two failures.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(Path::parent)
        .map(PathBuf::from)
        .expect("workspace root")
}

// ---------------------------------------------------------------------------
// Shared text helpers
// ---------------------------------------------------------------------------

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// The line with line comments and string literals blanked out, so brace and
/// bracket counting is not thrown off by a `{` inside a doc comment. Several
/// boundary types document their wire shape with braces in prose.
fn code_only(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '/' if chars.peek() == Some(&'/') => break,
            '"' => {
                // Consume the literal, honouring backslash escapes.
                while let Some(c) = chars.next() {
                    if c == '\\' {
                        chars.next();
                    } else if c == '"' {
                        break;
                    }
                }
            }
            _ => out.push(c),
        }
    }
    out
}

fn depth_delta(line: &str, open: char, close: char) -> i32 {
    let code = code_only(line);
    let o = i32::try_from(code.matches(open).count()).expect("brace count fits");
    let c = i32::try_from(code.matches(close).count()).expect("brace count fits");
    o - c
}

fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    paths.sort();
    for p in paths {
        if p.is_dir() {
            rs_files(&p, out);
        } else if p.extension().is_some_and(|e| e == "rs") {
            out.push(p);
        }
    }
}

/// `pane_camera_locked` -> `paneCameraLocked`.
fn to_camel(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut upper = false;
    for c in name.chars() {
        if c == '_' {
            upper = true;
        } else if upper {
            out.extend(c.to_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// `AddNode` -> `addNode`.
fn lower_first(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) => c.to_lowercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// `AddNode` -> `add_node`.
fn to_snake(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 4);
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// The wire spelling serde gives a name under a `rename_all` rule.
///
/// Panics on a rule nobody has modelled. That is deliberate: silently passing
/// a rule through unchanged would make the whole comparison vacuous for that
/// type, which is the failure this file exists to stop, one level up.
fn apply_case(name: &str, rule: Option<&str>, is_variant: bool) -> String {
    match rule {
        None => name.to_string(),
        Some("camelCase") => {
            if is_variant {
                lower_first(name)
            } else {
                to_camel(name)
            }
        }
        Some("snake_case") => {
            if is_variant {
                to_snake(name)
            } else {
                name.to_string()
            }
        }
        Some("PascalCase") => name.to_string(),
        Some("lowercase") => name.to_lowercase(),
        Some("UPPERCASE") => name.to_uppercase(),
        Some("SCREAMING_SNAKE_CASE") => to_snake(name).to_uppercase(),
        Some("kebab-case") => to_snake(name).replace('_', "-"),
        Some(other) => panic!(
            "boundary_mirror does not model `rename_all = \"{other}\"`. Teach \
             apply_case the rule rather than leaving the type unchecked."
        ),
    }
}

// ---------------------------------------------------------------------------
// The Rust side
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Enum,
    Struct,
    /// A newtype or tuple struct: it serializes as whatever it wraps, so it
    /// has no members of its own to compare.
    Newtype,
}

#[derive(Debug)]
struct RustType {
    kind: Kind,
    file: String,
    line: usize,
    rename_all: Option<String>,
    rename_all_fields: Option<String>,
    content: Option<String>,
    /// Struct fields, as wire names.
    fields: BTreeSet<String>,
    /// Rust identifiers of `#[serde(flatten)]` fields, whose own fields are
    /// spliced in by the caller once every type is known.
    flatten: Vec<String>,
    /// Enum variants, as wire names, each with its wire field names.
    variants: BTreeMap<String, BTreeSet<String>>,
    /// Every capitalized identifier in the body, which is how reachability is
    /// followed without resolving types.
    refs: BTreeSet<String>,
}

/// The value of a `key = "..."` serde option in an attribute block.
///
/// Matches the key only on an identifier boundary, so `rename_all` does not
/// match inside `rename_all_fields`. That distinction is the whole subject of
/// the trap this boundary has already sprung.
fn serde_opt(blob: &str, key: &str) -> Option<String> {
    let bytes = blob.as_bytes();
    let mut from = 0usize;
    while let Some(rel) = blob[from..].find(key) {
        let at = from + rel;
        let after = at + key.len();
        let before_ok = at == 0 || !is_ident_byte(bytes[at - 1]);
        let after_ok = after >= bytes.len() || !is_ident_byte(bytes[after]);
        if before_ok
            && after_ok
            && let Some(rest) = blob[after..].trim_start().strip_prefix('=')
            && let Some(rest) = rest.trim_start().strip_prefix('"')
            && let Some(end) = rest.find('"')
        {
            return Some(rest[..end].to_string());
        }
        from = at + key.len();
    }
    None
}

/// The item a line declares, if it declares one.
fn item_decl(line: &str) -> Option<(Kind, String)> {
    let mut t = line.trim_start();
    if let Some(rest) = t.strip_prefix("pub") {
        t = rest.trim_start();
        if t.starts_with('(') {
            {
                let i = t.find(')')?;
                t = t[i + 1..].trim_start()
            }
        }
    }
    for (word, kind) in [("enum", Kind::Enum), ("struct", Kind::Struct)] {
        let Some(rest) = t.strip_prefix(word) else {
            continue;
        };
        if !rest.starts_with(char::is_whitespace) {
            continue;
        }
        let rest = rest.trim_start();
        let name: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if name.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
            let kind = if kind == Kind::Struct && !line.contains('{') {
                Kind::Newtype
            } else {
                kind
            };
            return Some((kind, name));
        }
    }
    None
}

/// The attribute and doc block immediately above line `i`, taken whole.
///
/// Walks backwards tracking bracket depth, because `Command` and `EngineEvent`
/// both carry a multi-line `#[serde(..)]` block and the validation types carry
/// a multi-line `#[cfg_attr(..)]`. A line-by-line walk that stops at the first
/// line not starting with `#[` stops at the closing `)]` and finds no derive
/// at all, which silently drops the two largest types on the boundary.
fn attr_block(lines: &[&str], i: usize) -> String {
    let mut out: Vec<&str> = Vec::new();
    let mut depth = 0i32;
    let mut j = i;
    while j > 0 {
        j -= 1;
        let line = lines[j];
        depth += depth_delta(line, ']', '[') + depth_delta(line, ')', '(');
        let t = line.trim_start();
        let is_attr =
            t.starts_with("#[") || t.starts_with("//") || t.starts_with('*') || t.starts_with("/*");
        if depth > 0 || is_attr {
            out.push(line);
            depth = depth.max(0);
            continue;
        }
        break;
    }
    out.reverse();
    out.join("\n")
}

/// The name a body line declares as a field, if it declares one.
fn field_name(line: &str) -> Option<String> {
    let mut t = line.trim_start();
    if let Some(rest) = t.strip_prefix("pub") {
        t = rest.trim_start();
        if t.starts_with('(') {
            {
                let i = t.find(')')?;
                t = t[i + 1..].trim_start()
            }
        }
    }
    let name: String = t
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    if name.is_empty() {
        return None;
    }
    t[name.len()..]
        .trim_start()
        .starts_with(':')
        .then_some(name)
}

/// The name a body line declares as an enum variant, if it declares one.
///
/// Accepts the `Variant => "label"` form as well, because `preferences.rs`
/// declares fifty-two of its variants inside the `cycle_enum!` macro and those
/// enums reach the boundary through `PaneDisplaySettings`.
fn variant_name(line: &str) -> Option<String> {
    let t = line.trim_start();
    let name: String = t
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    if !name.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
        return None;
    }
    let rest = t[name.len()..].trim_start();
    let opens = rest.is_empty()
        || rest.starts_with('{')
        || rest.starts_with('(')
        || rest.starts_with(',')
        || rest.starts_with("=>");
    opens.then_some(name)
}

/// The type of a `#[serde(flatten)]` field: the first capitalized identifier
/// after the colon, which is the type whose fields get spliced into the wire.
fn flattened_type(line: &str) -> Option<String> {
    let after = line.split_once(':')?.1;
    after
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .find(|w| w.chars().next().is_some_and(|c| c.is_ascii_uppercase()))
        .map(str::to_string)
}

/// Fills members and referenced type names from an item's body.
fn parse_body(ty: &mut RustType, body: &[&str]) {
    let mut depth = 0i32;
    let mut pending = String::new();
    let mut current: Option<String> = None;
    for (idx, line) in body.iter().enumerate() {
        let code = code_only(line);
        for word in code.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
            if word.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
                ty.refs.insert(word.to_string());
            }
        }
        let t = line.trim_start();
        if idx == 0 {
            depth += depth_delta(line, '{', '}');
            continue;
        }
        if t.starts_with("#[") {
            pending.push_str(t);
            pending.push(' ');
            depth += depth_delta(line, '{', '}');
            continue;
        }
        if t.is_empty() || t.starts_with("//") || t.starts_with('*') || t.starts_with("/*") {
            depth += depth_delta(line, '{', '}');
            continue;
        }
        let attrs = std::mem::take(&mut pending);
        // `skip` drops a field from the wire entirely; `skip_serializing_if`
        // only makes it conditional, so the mirror still declares it.
        let skipped = attrs.contains("skip)")
            || attrs.contains("skip,")
            || attrs.contains("skip ")
                && !attrs.contains("skip_serializing_if")
                && !attrs.contains("skip_deserializing");
        let flattened = attrs.contains("flatten");
        let renamed = serde_opt(&attrs, "rename");
        let before = depth;

        if ty.kind == Kind::Struct && before == 1 {
            if let Some(name) = field_name(line)
                && !skipped
            {
                if flattened {
                    // Record the flattened field's TYPE, since what the
                    // wire carries is that type's own fields spliced in.
                    if let Some(ty_name) = flattened_type(line) {
                        ty.flatten.push(ty_name);
                    }
                } else {
                    let wire = renamed
                        .unwrap_or_else(|| apply_case(&name, ty.rename_all.as_deref(), false));
                    ty.fields.insert(wire);
                }
            }
        } else if ty.kind == Kind::Enum {
            if before == 1 {
                if let Some(name) = variant_name(line)
                    && !skipped
                {
                    let wire = renamed
                        .unwrap_or_else(|| apply_case(&name, ty.rename_all.as_deref(), true));
                    let mut fields = BTreeSet::new();
                    let rest = &t[name.len()..];
                    if let (Some(o), Some(c)) = (rest.find('{'), rest.rfind('}')) {
                        // A struct variant written on one line, such as
                        // `Ok { ms: f64 },`. The two-level walk below never
                        // sees its fields, because they never reach depth 2
                        // on a line of their own.
                        for part in rest[o + 1..c].split(',') {
                            if let Some(f) = field_name(part) {
                                fields.insert(apply_case(
                                    &f,
                                    ty.rename_all_fields.as_deref(),
                                    false,
                                ));
                            }
                        }
                    } else if rest.trim_start().starts_with('(') {
                        // Adjacent tagging gives a tuple variant's payload
                        // the `content` key; without a content key it has
                        // no named field at all.
                        if let Some(content) = &ty.content {
                            fields.insert(content.clone());
                        }
                    }
                    current = Some(wire.clone());
                    ty.variants.insert(wire, fields);
                }
            } else if before == 2
                && let (Some(name), Some(variant)) = (field_name(line), current.as_ref())
                && !skipped
            {
                let wire = renamed
                    .unwrap_or_else(|| apply_case(&name, ty.rename_all_fields.as_deref(), false));
                ty.variants.entry(variant.clone()).or_default().insert(wire);
            }
        }
        depth += depth_delta(line, '{', '}');
    }
}

/// Every serde-deriving type declared under the given crate source roots.
///
/// Keyed on the derive attribute rather than on indentation, so the three
/// boundary DTOs declared inside function bodies (`CapsDto`, `BackendCapsDto`
/// and `PassesDto`) are found like any other.
fn scan_rust(roots: &[&str]) -> BTreeMap<String, RustType> {
    let root = workspace_root();
    let mut out: BTreeMap<String, RustType> = BTreeMap::new();
    let mut files = Vec::new();
    for r in roots {
        rs_files(&root.join(r), &mut files);
    }
    for path in files {
        let src = std::fs::read_to_string(&path).expect("read a source file");
        let lines: Vec<&str> = src.lines().collect();
        let rel = path
            .strip_prefix(&root)
            .unwrap_or(&path)
            .display()
            .to_string();
        for (i, line) in lines.iter().enumerate() {
            let Some((kind, name)) = item_decl(line) else {
                continue;
            };
            let blob = attr_block(&lines, i);
            if !blob.contains("Serialize") && !blob.contains("Deserialize") {
                continue;
            }
            let mut ty = RustType {
                kind,
                file: rel.clone(),
                line: i + 1,
                rename_all: serde_opt(&blob, "rename_all"),
                rename_all_fields: serde_opt(&blob, "rename_all_fields"),
                content: serde_opt(&blob, "content"),
                fields: BTreeSet::new(),
                flatten: Vec::new(),
                variants: BTreeMap::new(),
                refs: BTreeSet::new(),
            };
            if kind != Kind::Newtype {
                let mut depth = 0i32;
                let mut body = Vec::new();
                for (k, l) in lines.iter().enumerate().skip(i) {
                    depth += depth_delta(l, '{', '}');
                    body.push(*l);
                    if depth == 0 && k > i {
                        break;
                    }
                }
                parse_body(&mut ty, &body);
            }
            assert!(
                !out.contains_key(&name),
                "two serde types are both called `{name}`; the mirror is keyed \
                 by name, so one of them must be renamed"
            );
            out.insert(name, ty);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// The TypeScript side
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum TsKind {
    Interface,
    Alias,
}

#[derive(Debug, Default)]
struct TsMember {
    /// A bare string-literal member, such as `"root"`.
    lit: Option<String>,
    /// The properties of an object-literal member.
    props: BTreeMap<String, String>,
}

#[derive(Debug)]
struct TsType {
    kind: TsKind,
    props: BTreeMap<String, String>,
    members: Vec<TsMember>,
}

/// The mirror with its comments blanked and its string literals intact.
///
/// The literals are the discriminant values, so a strip that ate them would
/// leave every tagged union looking untagged. Twenty-eight percent of the file
/// is comment, and several comments contain braces.
fn strip_ts_comments(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = String::with_capacity(src.len());
    let mut i = 0usize;
    while i < b.len() {
        if b[i] == b'"' || b[i] == b'\'' {
            let quote = b[i];
            out.push(b[i] as char);
            i += 1;
            while i < b.len() {
                out.push(b[i] as char);
                if b[i] == b'\\' {
                    if i + 1 < b.len() {
                        out.push(b[i + 1] as char);
                    }
                    i += 2;
                    continue;
                }
                let done = b[i] == quote;
                i += 1;
                if done {
                    break;
                }
            }
        } else if b[i] == b'/' && i + 1 < b.len() && b[i + 1] == b'*' {
            while i < b.len() && !(b[i] == b'*' && i + 1 < b.len() && b[i + 1] == b'/') {
                out.push(if b[i] == b'\n' { '\n' } else { ' ' });
                i += 1;
            }
            out.push_str("  ");
            i += 2;
        } else if b[i] == b'/' && i + 1 < b.len() && b[i + 1] == b'/' {
            while i < b.len() && b[i] != b'\n' {
                out.push(' ');
                i += 1;
            }
        } else {
            out.push(b[i] as char);
            i += 1;
        }
    }
    out
}

/// Split on a separator that appears at bracket depth zero.
fn split_top(s: &str, sep: char) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut buf = String::new();
    for c in s.chars() {
        match c {
            '{' | '[' | '(' => depth += 1,
            '}' | ']' | ')' => depth -= 1,
            _ => {}
        }
        if c == sep && depth == 0 {
            parts.push(std::mem::take(&mut buf));
        } else {
            buf.push(c);
        }
    }
    parts.push(buf);
    parts
        .into_iter()
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

/// `name: type` pairs at the top level of an object body, `?` stripped.
fn props_at_depth1(body: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for chunk in split_top(&body.replace(';', ","), ',') {
        let Some(colon) = chunk.find(':') else {
            continue;
        };
        let name = chunk[..colon].trim().trim_end_matches('?').trim();
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        out.insert(name.to_string(), chunk[colon + 1..].trim().to_string());
    }
    out
}

/// The substring from `at` to the brace or bracket that closes the one at `at`.
fn balanced(src: &str, at: usize, open: char, close: char) -> &str {
    let mut depth = 0i32;
    for (i, c) in src[at..].char_indices() {
        if c == open {
            depth += 1;
        } else if c == close {
            depth -= 1;
            if depth == 0 {
                return &src[at + 1..at + i];
            }
        }
    }
    panic!("unbalanced `{open}` in the mirror at byte {at}");
}

fn scan_ts() -> BTreeMap<String, TsType> {
    let path = workspace_root().join("web/src/engine/types.ts");
    let raw = std::fs::read_to_string(&path).expect("read web/src/engine/types.ts");
    let src = strip_ts_comments(&raw);
    let mut out = BTreeMap::new();

    for (marker, kind) in [
        ("export interface ", TsKind::Interface),
        ("export type ", TsKind::Alias),
    ] {
        let mut from = 0usize;
        while let Some(rel) = src[from..].find(marker) {
            let at = from + rel + marker.len();
            let name: String = src[at..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            from = at + name.len();
            if name.is_empty() {
                continue;
            }
            if kind == TsKind::Interface {
                let open = src[from..].find('{').expect("interface body") + from;
                let body = balanced(&src, open, '{', '}');
                out.insert(
                    name,
                    TsType {
                        kind: TsKind::Interface,
                        props: props_at_depth1(body),
                        members: Vec::new(),
                    },
                );
            } else {
                let eq = src[from..].find('=').expect("alias body") + from;
                // The alias runs to the first `;` at depth zero.
                let mut depth = 0i32;
                let mut end = src.len();
                for (i, c) in src[eq + 1..].char_indices() {
                    match c {
                        '{' | '[' | '(' => depth += 1,
                        '}' | ']' | ')' => depth -= 1,
                        ';' if depth == 0 => {
                            end = eq + 1 + i;
                            break;
                        }
                        _ => {}
                    }
                }
                let body = &src[eq + 1..end];
                let mut members = Vec::new();
                for part in split_top(body, '|') {
                    let mut p = part.trim();
                    while p.starts_with('(') && p.ends_with(')') {
                        p = p[1..p.len() - 1].trim();
                    }
                    let mut member = TsMember::default();
                    // An intersection member contributes every object half it
                    // has: `({ kind: "literal" } & ParamValue)`.
                    for half in split_top(p, '&') {
                        let h = half.trim();
                        if h.starts_with('{') {
                            let inner = balanced(h, 0, '{', '}');
                            member.props.extend(props_at_depth1(inner));
                        } else if h.starts_with('"') && h.ends_with('"') && h.len() >= 2 {
                            member.lit = Some(h[1..h.len() - 1].to_string());
                        }
                    }
                    members.push(member);
                }
                out.insert(
                    name,
                    TsType {
                        kind: TsKind::Alias,
                        props: BTreeMap::new(),
                        members,
                    },
                );
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// What crosses, and what each thing corresponds to
// ---------------------------------------------------------------------------

/// Crates whose serde types can reach the boundary.
const SCAN_ROOTS: &[&str] = &[
    "crates/solarxy-graph/src",
    "crates/solarxy-core/src",
    "crates/solarxy-web/src",
    "crates/solarxy-host/src",
    "crates/solarxy-renderer/src",
];

/// The two modules that *are* the boundary. Everything else is included only
/// by being referenced from something here, which is what makes membership
/// derived rather than listed: a type added to either module is checked with
/// no other edit, and so is a type a new field points at.
const SEED_DIRS: &[&str] = &[
    "crates/solarxy-web/src/app/",
    "crates/solarxy-graph/src/engine/",
];

/// Rust type to mirror type, where the mechanical rule (identity, then strip a
/// `Dto` or `Snapshot` suffix) gets it wrong or finds nothing.
const NAMED: &[(&str, &str)] = &[
    ("CapsDto", "BackendCaps"),
    ("BackendCapsDto", "BackendCapsSet"),
    ("PassesDto", "StillPasses"),
    ("RectDto", "PaneRectDto"),
    ("ViewportDto", "CanvasViewport"),
    ("MetaDto", "SceneMeta"),
    ("LoadResultDto", "SlxyLoadResult"),
    ("EnvironmentDto", "EnvironmentState"),
    ("DisplaySettings", "DisplaySettingsDto"),
    ("Severity", "ValidationSeverity"),
    ("IssueScope", "ValidationScope"),
    ("ShowIfSnapshot", "ShowIfClause"),
];

/// Rust type to the mirror property that spells it inline, rather than to a
/// named mirror type. The mirror writes most small enums as a union on the
/// property that carries them.
const INLINE: &[(&str, &str, &str)] = &[
    ("ViewMode", "PaneDisplaySettings", "viewMode"),
    ("UvMode", "PaneDisplaySettings", "uvMode"),
    ("BoundsMode", "PaneDisplaySettings", "boundsMode"),
    ("LineWeight", "PaneDisplaySettings", "lineWeight"),
    ("InspectionMode", "PaneDisplaySettings", "inspectionMode"),
    (
        "MaterialOverride",
        "PaneDisplaySettings",
        "materialOverride",
    ),
    ("PaneMode", "PaneDisplaySettings", "paneMode"),
    ("PaneEngine", "PaneDisplaySettings", "paneEngine"),
    ("NormalsMode", "PaneDisplaySettings", "normalsMode"),
    ("ToneMode", "PaneLook", "toneMode"),
    ("UnitSnapshot", "ParamSnapshot", "unit"),
    ("NodePathAcceptSnapshot", "ParamSnapshot", "nodePath"),
    ("ScreenshotOverlaysDto", "ScreenshotOpts", "overlays"),
    ("Category", "NodeTypeSnapshot", "category"),
    ("AttrColorMode", "AttrVizState", "colorMode"),
];

/// Newtypes: they serialize as the scalar they wrap, so there is no shape to
/// compare and the mirror declares them as primitive aliases.
const SCALAR: &[&str] = &["NodeId", "EdgeId", "AnnotationId", "AssetId"];

/// Crosses the boundary, but the mirror deliberately does not describe its
/// shape. Each reason is the thing a future reader needs, because every one of
/// these looks at first like a gap.
const OPAQUE: &[(&str, &str)] = &[
    (
        "Annotation",
        "flattened into AnnotationSnapshot, which is what the mirror's Annotation mirrors",
    ),
    (
        "BackgroundMode",
        "the only untagged type on the boundary; the mirror types it unknown",
    ),
    (
        "BuiltinBg",
        "reached only through BackgroundMode, which the mirror types unknown",
    ),
    (
        "UvMapBackground",
        "the mirror types PaneDisplaySettings.uvBg as a plain string",
    ),
    (
        "IssueKind",
        "the mirror types ValidationIssue.kind as a free string",
    ),
    (
        "CameraCommandDto",
        "Rust models it as one struct of options, the mirror as a kind-tagged union",
    ),
    (
        "ResolvedParamDto",
        "Rust omits absent fields, the mirror models the same wire shape as an ok-tagged union",
    ),
    (
        "GraphFragment",
        "the clipboard payload, which the mirror types unknown and never destructures",
    ),
    ("SubflowFragment", "reached only through GraphFragment"),
    (
        "ValidationReport",
        "rides inside the validate worker's JSON payload, which is Rust at both ends",
    ),
];

/// Referenced from a boundary module but does not itself cross.
const INTERNAL: &[(&str, &str)] = &[
    (
        "DocumentFile",
        "the save and load file format, not the wasm boundary",
    ),
    ("DocumentData", "reached only through DocumentFile"),
    ("GraphData", "reached only through DocumentFile"),
    ("Edge", "document interior; the mirror carries EdgeMirror"),
    (
        "NodeData",
        "document interior; the mirror carries NodeMirror",
    ),
    (
        "GizmoTarget",
        "an engine return value read in Rust, never serialized to JS",
    ),
];

/// Every serde type reachable from a boundary module by following the type
/// names its fields mention.
fn reachable(all: &BTreeMap<String, RustType>) -> BTreeSet<String> {
    let mut reach: BTreeSet<String> = all
        .iter()
        .filter(|(_, t)| SEED_DIRS.iter().any(|d| t.file.starts_with(d)))
        .map(|(n, _)| n.clone())
        .collect();
    let mut frontier: Vec<String> = reach.iter().cloned().collect();
    while let Some(n) = frontier.pop() {
        for r in &all[&n].refs {
            if all.contains_key(r) && reach.insert(r.clone()) {
                frontier.push(r.clone());
            }
        }
    }
    reach
}

/// The mirror type a Rust type corresponds to under the mechanical rule.
fn mechanical<'a>(name: &str, ts: &'a BTreeMap<String, TsType>) -> Option<&'a str> {
    for cand in [
        Some(name),
        name.strip_suffix("Dto"),
        name.strip_suffix("Snapshot"),
    ]
    .into_iter()
    .flatten()
    {
        if let Some((k, _)) = ts.get_key_value(cand) {
            return Some(k.as_str());
        }
    }
    None
}

/// Variant wire name to field wire names, read off a mirror union.
fn ts_variants(t: &TsType) -> BTreeMap<String, BTreeSet<String>> {
    let mut out = BTreeMap::new();
    for m in &t.members {
        if let Some(lit) = &m.lit {
            out.insert(lit.clone(), BTreeSet::new());
            continue;
        }
        // An internally tagged member: one property whose value is a string
        // literal is the tag, and the rest are the variant's fields.
        let tag = m.props.iter().find(|(_, v)| {
            let v = v.trim();
            v.starts_with('"') && v.ends_with('"') && !v[1..].trim_end_matches('"').contains('"')
        });
        if let Some((tag_key, tag_val)) = tag {
            let name = tag_val.trim().trim_matches('"').to_string();
            let fields = m
                .props
                .keys()
                .filter(|k| *k != tag_key)
                .cloned()
                .collect::<BTreeSet<_>>();
            out.insert(name, fields);
        } else if m.props.len() == 1 {
            // Externally tagged: the single property name is the variant, and
            // an object payload carries the variant's fields.
            let (k, v) = m.props.iter().next().expect("one property");
            let v = v.trim();
            let fields = if v.starts_with('{') {
                props_at_depth1(balanced(v, 0, '{', '}'))
                    .into_keys()
                    .collect()
            } else {
                BTreeSet::new()
            };
            out.insert(k.clone(), fields);
        } else {
            for k in m.props.keys() {
                out.insert(k.clone(), BTreeSet::new());
            }
        }
    }
    out
}

/// The string literals a mirror property's type spells out.
fn literals(ty: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut rest = ty;
    while let Some(i) = rest.find('"') {
        let after = &rest[i + 1..];
        match after.find('"') {
            Some(j) => {
                out.insert(after[..j].to_string());
                rest = &after[j + 1..];
            }
            None => break,
        }
    }
    out
}

// ---------------------------------------------------------------------------
// The checks
// ---------------------------------------------------------------------------

/// The scan reads what it claims to read.
///
/// A source scanner's worst failure is not being wrong but being empty: a
/// regex that stops matching returns no findings and every comparison passes.
/// This release already saw three guards that passed both before and after the
/// change they were meant to gate, so the scanner is pinned against figures
/// taken by hand.
#[test]
fn the_boundary_scan_is_not_vacuous() {
    let rust = scan_rust(SCAN_ROOTS);
    let ts = scan_ts();

    for (name, want) in [("Command", 35usize), ("EngineEvent", 21), ("HostEvent", 10)] {
        let r = rust
            .get(name)
            .unwrap_or_else(|| panic!("the scan no longer finds `{name}`"));
        assert_eq!(
            r.variants.len(),
            want,
            "`{name}` scanned {} variants, expected {want}. If the enum really \
             changed, update this figure; if it did not, the scanner broke.",
            r.variants.len()
        );
        let t = ts
            .get(name)
            .unwrap_or_else(|| panic!("the mirror no longer declares `{name}`"));
        assert_eq!(ts_variants(t).len(), want, "the mirror's `{name}` union");
    }

    // The three DTOs declared inside function bodies. A scan anchored on
    // top-level items misses them and says nothing about it.
    for name in ["CapsDto", "BackendCapsDto", "PassesDto"] {
        assert!(
            rust.contains_key(name),
            "`{name}` is declared inside a function body and the scan must \
             still find it, because it crosses the boundary like any other DTO"
        );
    }

    // The macro-declared preference enums, which use `Variant => \"label\"`.
    assert_eq!(
        rust["LineWeight"].variants.len(),
        3,
        "`LineWeight` is declared by the cycle_enum! macro; the scanner must \
         read that form or fifty-two variants go unchecked"
    );

    assert!(
        rust.len() > 100 && ts.len() > 60,
        "scanned {} Rust types and {} mirror types, which is too few to be \
         a real read of either side",
        rust.len(),
        ts.len()
    );
}

/// Every type that crosses the boundary is accounted for.
///
/// This is the rule the whole file rests on. A check that knows what to look
/// at because someone typed a list fails in exactly the way it exists to
/// prevent, one level up. So membership is derived: the seeds are two module
/// paths and everything else arrives by being referenced. What stays written
/// down is only the *exceptions*, and a type that matches none of them fails
/// here rather than being skipped.
#[test]
fn every_boundary_type_is_accounted_for() {
    let rust = scan_rust(SCAN_ROOTS);
    let ts = scan_ts();
    let reach = reachable(&rust);

    let mut unaccounted = Vec::new();
    for name in &reach {
        let known = SCALAR.contains(&name.as_str())
            || OPAQUE.iter().any(|(n, _)| n == name)
            || INTERNAL.iter().any(|(n, _)| n == name)
            || INLINE.iter().any(|(n, _, _)| n == name)
            || NAMED.iter().any(|(n, _)| n == name)
            || mechanical(name, &ts).is_some();
        if !known {
            let t = &rust[name];
            unaccounted.push(format!("{name} ({}:{})", t.file, t.line));
        }
    }
    assert!(
        unaccounted.is_empty(),
        "these types reach the WebAssembly boundary and the mirror declares \
         nothing for them:\n  {}\n\nEither add the counterpart to \
         web/src/engine/types.ts, or record in this file why it does not need \
         one. Being unlisted is not an option, because that is how a variant \
         ships with no mirror.",
        unaccounted.join("\n  ")
    );

    // The exception tables are held honest in the other direction too: an
    // entry naming a type that no longer reaches the boundary is a stale
    // excuse, and stale excuses are how a real gap gets waved through.
    let mut stale = Vec::new();
    for n in SCALAR.iter().copied() {
        if !reach.contains(n) {
            stale.push(format!("SCALAR {n}"));
        }
    }
    for (n, _) in OPAQUE.iter().chain(INTERNAL.iter()) {
        if !reach.contains(*n) {
            stale.push(format!("OPAQUE/INTERNAL {n}"));
        }
    }
    for (n, _, _) in INLINE {
        if !reach.contains(*n) {
            stale.push(format!("INLINE {n}"));
        }
    }
    for (n, _) in NAMED {
        if !reach.contains(*n) {
            stale.push(format!("NAMED {n}"));
        }
    }
    assert!(
        stale.is_empty(),
        "these table entries name types that no longer reach the boundary:\n  {}",
        stale.join("\n  ")
    );
}

/// The mirror carries exactly the boundary's variants and fields.
#[test]
fn the_mirror_carries_exactly_the_boundarys_members() {
    let rust = scan_rust(SCAN_ROOTS);
    let ts = scan_ts();
    let reach = reachable(&rust);
    let mut problems: Vec<String> = Vec::new();

    let diff = |what: &str, mine: &BTreeSet<String>, theirs: &BTreeSet<String>| -> Option<String> {
        if mine == theirs {
            return None;
        }
        let missing: Vec<&String> = mine.difference(theirs).collect();
        let extra: Vec<&String> = theirs.difference(mine).collect();
        let mut s = format!("{what}:");
        if !missing.is_empty() {
            s.push_str(&format!(
                "\n      in Rust, absent from the mirror: {missing:?}"
            ));
        }
        if !extra.is_empty() {
            s.push_str(&format!(
                "\n      in the mirror, absent from Rust:  {extra:?}"
            ));
        }
        Some(s)
    };

    for name in &reach {
        if SCALAR.contains(&name.as_str())
            || OPAQUE.iter().any(|(n, _)| n == name)
            || INTERNAL.iter().any(|(n, _)| n == name)
        {
            continue;
        }
        let t = &rust[name];

        if let Some((_, owner, prop)) = INLINE.iter().find(|(n, _, _)| n == name) {
            let Some(owner_ty) = ts.get(*owner) else {
                problems.push(format!("{name}: the mirror has no `{owner}`"));
                continue;
            };
            let Some(raw) = owner_ty.props.get(*prop) else {
                problems.push(format!("{name}: the mirror's `{owner}` has no `{prop}`"));
                continue;
            };
            let theirs = if t.kind == Kind::Struct {
                props_at_depth1(balanced(raw.trim(), 0, '{', '}'))
                    .into_keys()
                    .collect()
            } else {
                literals(raw)
            };
            let mine: BTreeSet<String> = if t.kind == Kind::Struct {
                t.fields.clone()
            } else {
                t.variants.keys().cloned().collect()
            };
            if let Some(p) = diff(&format!("{name} against {owner}.{prop}"), &mine, &theirs) {
                problems.push(p);
            }
            continue;
        }

        let ts_name = NAMED
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, m)| *m)
            .or_else(|| mechanical(name, &ts))
            .expect("accounted for by the sibling test");
        let mirror = &ts[ts_name];

        match t.kind {
            Kind::Newtype => {}
            Kind::Struct => {
                if mirror.kind != TsKind::Interface {
                    problems.push(format!(
                        "{name} is a struct but the mirror's `{ts_name}` is a union"
                    ));
                    continue;
                }
                let mut mine = t.fields.clone();
                for flat in &t.flatten {
                    match rust.get(flat) {
                        Some(inner) => mine.extend(inner.fields.iter().cloned()),
                        None => problems.push(format!(
                            "{name} flattens `{flat}`, which the scan did not find"
                        )),
                    }
                }
                let theirs: BTreeSet<String> = mirror.props.keys().cloned().collect();
                if let Some(p) = diff(&format!("{name} against {ts_name}"), &mine, &theirs) {
                    problems.push(p);
                }
            }
            Kind::Enum => {
                if mirror.kind != TsKind::Alias {
                    problems.push(format!(
                        "{name} is an enum but the mirror's `{ts_name}` is an interface"
                    ));
                    continue;
                }
                let theirs = ts_variants(mirror);
                let mine_names: BTreeSet<String> = t.variants.keys().cloned().collect();
                let their_names: BTreeSet<String> = theirs.keys().cloned().collect();
                if let Some(p) = diff(
                    &format!("{name} variants against {ts_name}"),
                    &mine_names,
                    &their_names,
                ) {
                    problems.push(p);
                    continue;
                }
                for (v, mine) in &t.variants {
                    if let Some(p) = diff(
                        &format!("{name}::{v} fields against {ts_name}"),
                        mine,
                        &theirs[v],
                    ) {
                        problems.push(p);
                    }
                }
            }
        }
    }

    assert!(
        problems.is_empty(),
        "the browser's mirror disagrees with the Rust boundary:\n\n  {}\n\n\
         Rust owns the schema, so the mirror is normally the side that moves. \
         web/src/engine/types.ts is the file to edit.",
        problems.join("\n  ")
    );
}
