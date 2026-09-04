//! Collecting the files a model names beside itself.
//!
//! A caller opens one path, but a model is often several files: a `.gltf`
//! points at an external `.bin` and its images, an `.obj` points at an `.mtl`
//! which points at its textures. Loading only the named file renders a glTF
//! not at all, claiming its own buffer was missing, and an OBJ untextured
//! with no complaint.
//!
//! # Collection, not staging
//!
//! This module reads the companions and hands them back as named blobs; what
//! a caller does with them is its own affair. The two native callers stage
//! them into an engine's asset table, where the import node resolves
//! companions by trailing file-name component, so handing over the siblings
//! is the whole fix. It lives here, beside the [`DirResolver`] that guards
//! the reads, rather than in either caller: the terminal's render command
//! and the desktop's still render must not each carry a walk that can drift
//! from the other. The browser reaches the same table by a different route
//! (its picker stages whole folders), and its preflight in
//! `web/src/engine/sidecars.ts` carries the same required-versus-optional
//! split, so the surfaces cannot disagree about what a format needs.
//!
//! # Named references, not the directory
//!
//! Only what the model asks for is read. Reading the containing directory
//! would pull in whatever else happened to be beside the file, hash it and
//! hold it in memory for a render that does not use it, and would make
//! behaviour depend on what a person keeps in a folder.
//!
//! # A URI is untrusted input
//!
//! Every reference is read through [`DirResolver`], which refuses a parent,
//! root or prefix component and confirms the canonicalized result is still
//! inside the model's directory, so a symlink cannot escape either. The rule
//! lives there and is not restated here. What this module adds is only the
//! *message*: a resolver that answers `None` cannot say whether the file
//! escaped or was simply absent, and a reader deserves to be told which.

use std::path::Path;

use crate::{AssetResolver, DirResolver};

/// One companion, ready to stage: the reference as written (percent-decoded,
/// relative to the model's own directory, subdirectories kept because they
/// are what read the file) and its bytes.
#[derive(Debug)]
pub struct CompanionAsset {
    pub name: String,
    pub bytes: Vec<u8>,
}

/// What a collection walk produced: the companions that were read, and a
/// line for each optional one that could not be.
#[derive(Debug, Default)]
pub struct Companions {
    pub assets: Vec<CompanionAsset>,
    pub warnings: Vec<String>,
}

/// A required companion that could not be read, which for a glTF means the
/// model cannot be parsed at all. The message names the file and, where the
/// reference carries a component the resolver refuses, says so.
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct RequiredCompanionError {
    pub message: String,
}

/// A file the model names.
struct Reference {
    /// The reference as written, percent-decoded, relative to the model's
    /// own directory. Kept whole rather than reduced to its file name,
    /// because it is what reads the file off the disk; a glTF may legally
    /// point into a subdirectory.
    rel: String,
    /// Whether the format fails without it. A missing glTF buffer is an
    /// error; a missing MTL or texture degrades to defaults, which is the
    /// split this crate documents on [`AssetResolver`] and the browser
    /// already implements.
    required: bool,
}

/// Collects every companion `bytes` names, with a warning for each optional
/// one that could not be read.
///
/// Self-contained formats name nothing extra and are handled first, so the
/// common case costs one match.
///
/// # Errors
/// A required companion that could not be read.
pub fn collect(path: &Path, ext: &str, bytes: &[u8]) -> Result<Companions, RequiredCompanionError> {
    // `.glb` embeds everything; `.stl` and `.ply` carry triangles and nothing
    // else. The registry's own node documentation says so, and writing the
    // no-op as the first arm is what makes it obvious.
    if matches!(ext, "glb" | "stl" | "ply") {
        return Ok(Companions::default());
    }

    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut resolver = DirResolver::new(dir);
    let mut out = Companions::default();

    let refs = match ext {
        "gltf" => gltf_references(bytes),
        "obj" => obj_references(bytes, &mut resolver, &mut out),
        _ => Vec::new(),
    };

    for r in refs {
        match resolver.read(&r.rel) {
            Some(blob) => out.assets.push(CompanionAsset {
                name: r.rel,
                bytes: blob,
            }),
            None if r.required => {
                return Err(RequiredCompanionError {
                    message: format!("{} {}", describe_failure(&r.rel), r.rel),
                });
            }
            None => out.warnings.push(format!(
                "{} {}, so it was not loaded",
                describe_failure(&r.rel),
                r.rel
            )),
        }
    }
    Ok(out)
}

/// Why a reference could not be read, as far as can honestly be told.
///
/// `DirResolver::read` collapses refusal, absence and an IO error into one
/// `None`. Rather than reimplement its rule, or widen its return type for a
/// message, the classification is redone here only for the wording: if the
/// reference carries a component the rule refuses, saying so is right whether
/// or not the file also happened to be missing.
fn describe_failure(rel: &str) -> &'static str {
    let escapes = Path::new(rel).components().any(|c| {
        matches!(
            c,
            std::path::Component::ParentDir
                | std::path::Component::RootDir
                | std::path::Component::Prefix(_)
        )
    });
    if escapes {
        "refused a reference that leaves the model's own directory,"
    } else {
        "could not read"
    }
}

/// The buffers and images a glTF names, skipping the ones it embeds.
///
/// A shallow scan rather than a typed glTF model: this needs two arrays of
/// strings and has no business owning the format. If it ever needs more than
/// that, it should grow beside the loader that consumes the same references.
fn gltf_references(bytes: &[u8]) -> Vec<Reference> {
    let Ok(json) = serde_json::from_slice::<serde_json::Value>(bytes) else {
        // Let the real parser produce the error. A preflight must never
        // refuse a file the loader would have accepted.
        return Vec::new();
    };
    let mut out = Vec::new();
    for (key, required) in [("buffers", true), ("images", false)] {
        let Some(entries) = json.get(key).and_then(|v| v.as_array()) else {
            continue;
        };
        for uri in entries
            .iter()
            .filter_map(|e| e.get("uri").and_then(|u| u.as_str()))
        {
            // A data URI carries its own bytes and needs no file.
            if uri.starts_with("data:") {
                continue;
            }
            push_unique(
                &mut out,
                Reference {
                    rel: percent_decode(uri),
                    required,
                },
            );
        }
    }
    out
}

/// The material libraries an OBJ names, and the maps those libraries name.
///
/// Transitive on purpose. OBJ textures are decoded at parse time through the
/// same resolver, so collecting the MTL alone still renders untextured, which
/// is the half-fix this walk exists to avoid. The browser's preflight stops at
/// the MTL because its picker stages whole folders anyway; the native callers
/// have no such backstop.
///
/// Each library is read here to be scanned and collected in one go, so a
/// library that cannot be read is reported once rather than twice.
fn obj_references(
    bytes: &[u8],
    resolver: &mut DirResolver,
    out: &mut Companions,
) -> Vec<Reference> {
    let mut refs = Vec::new();
    for lib in scan_directive(bytes, "mtllib") {
        let rel = percent_decode(&lib);
        let Some(blob) = resolver.read(&rel) else {
            out.warnings.push(format!(
                "{} the material library {rel}, so the model renders with default materials",
                describe_failure(&rel)
            ));
            continue;
        };
        for map in mtl_maps(&blob) {
            push_unique(
                &mut refs,
                Reference {
                    rel: map,
                    required: false,
                },
            );
        }
        out.assets.push(CompanionAsset {
            name: rel,
            bytes: blob,
        });
    }
    refs
}

/// The texture references in an MTL.
///
/// Every `map_*` directive plus the bump aliases that do not carry the prefix.
/// A map line may carry options before the filename (`-bm 0.2`, `-o 1 1 1`,
/// `-s 2 2 2`), so the filename is taken as the last whitespace-separated token
/// rather than the whole remainder. That differs from `mtllib`, where the
/// remainder is one path and spaces belong to it, which is how the loader reads
/// it too.
fn mtl_maps(bytes: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    for line in String::from_utf8_lossy(bytes).lines() {
        let line = line.trim();
        let Some((head, rest)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        let head = head.to_ascii_lowercase();
        if !(head.starts_with("map_") || head == "bump" || head == "disp" || head == "decal") {
            continue;
        }
        if let Some(file) = rest.split_whitespace().next_back() {
            let decoded = percent_decode(file);
            if !decoded.is_empty() && !out.contains(&decoded) {
                out.push(decoded);
            }
        }
    }
    out
}

/// Every argument of a one-word OBJ directive, with the line's whole remainder
/// taken as the value.
///
/// `mtllib` may legally name several libraries on one line, but a path may also
/// contain spaces, and the two cannot be told apart. The loader treats the
/// remainder as one path, so this does too rather than disagreeing with it.
fn scan_directive(bytes: &[u8], directive: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in String::from_utf8_lossy(bytes).lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix(directive) else {
            continue;
        };
        if !rest.starts_with(char::is_whitespace) {
            continue;
        }
        let value = rest.trim();
        if !value.is_empty() && !out.contains(&value.to_string()) {
            out.push(value.to_string());
        }
    }
    out
}

/// Percent-decoding, because glTF URIs are URI-encoded and a texture called
/// `red brick.png` arrives as `red%20brick.png`.
///
/// Written here rather than taken as a dependency: it is a dozen lines, the
/// crate graph is a supply chain, and the browser's mirror of this logic hand
/// -writes it for the same reason. An invalid escape is left as written, which
/// is what lets a literal `%` in a file name survive.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3])
                .ok()
                .and_then(|h| u8::from_str_radix(h, 16).ok());
            if let Some(b) = hex {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Push unless an equal relative path is already listed.
///
/// A glTF naming one image from several materials is ordinary, and reading it
/// twice would be harmless but wasteful; more to the point a duplicate would
/// warn twice about one missing file.
fn push_unique(out: &mut Vec<Reference>, r: Reference) {
    if !out.iter().any(|e| e.rel == r.rel) {
        out.push(r);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_gltf_names_its_buffer_as_required_and_its_image_as_optional() {
        let json = br#"{
            "buffers": [{"uri": "model.bin"}],
            "images":  [{"uri": "textures/albedo.png"}]
        }"#;
        let refs = gltf_references(json);
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].rel, "model.bin");
        assert!(refs[0].required, "an external buffer is not optional");
        assert_eq!(
            refs[1].rel, "textures/albedo.png",
            "a subdirectory survives, because it is what reads the file"
        );
        assert!(
            !refs[1].required,
            "a missing image degrades to an empty slot"
        );
    }

    #[test]
    fn an_embedded_gltf_names_nothing() {
        let json = br#"{"buffers": [{"uri": "data:application/octet-stream;base64,AAAA"}]}"#;
        assert!(gltf_references(json).is_empty());
    }

    /// A preflight that refused a file the real parser would accept would be
    /// worse than no preflight at all.
    #[test]
    fn unparseable_json_names_nothing_rather_than_failing() {
        assert!(gltf_references(b"this is not json").is_empty());
    }

    #[test]
    fn a_repeated_uri_is_named_once() {
        let json = br#"{"images": [{"uri": "t.png"}, {"uri": "t.png"}]}"#;
        assert_eq!(gltf_references(json).len(), 1);
    }

    #[test]
    fn percent_escapes_decode_and_a_bare_percent_survives() {
        assert_eq!(percent_decode("red%20brick.png"), "red brick.png");
        assert_eq!(percent_decode("100%.png"), "100%.png");
        assert_eq!(percent_decode("plain.png"), "plain.png");
    }

    #[test]
    fn an_obj_names_its_material_library_with_spaces_intact() {
        assert_eq!(
            scan_directive(b"v 0 0 0\nmtllib my materials.mtl\nf 1 1 1\n", "mtllib"),
            vec!["my materials.mtl".to_string()]
        );
    }

    /// `mtllib_extra` must not match `mtllib`, which is why the directive has
    /// to be followed by whitespace rather than merely be a prefix.
    #[test]
    fn a_longer_directive_is_not_mistaken_for_this_one() {
        assert!(scan_directive(b"mtllibrary foo.mtl\n", "mtllib").is_empty());
    }

    /// The trap this guards: a map line may carry options before the filename,
    /// and taking the whole remainder would name a file called `-bm 0.2 b.png`.
    #[test]
    fn map_options_are_skipped_and_the_filename_is_taken() {
        let mtl = b"newmtl m\nmap_Kd albedo.png\nmap_Bump -bm 0.2 normal.png\nbump -bm 1 b.png\n";
        assert_eq!(
            mtl_maps(mtl),
            vec![
                "albedo.png".to_string(),
                "normal.png".to_string(),
                "b.png".to_string()
            ]
        );
    }

    #[test]
    fn a_line_that_is_not_a_map_is_ignored() {
        let mtl = b"newmtl m\nKd 0.8 0.8 0.8\nNs 10\n";
        assert!(mtl_maps(mtl).is_empty());
    }

    #[test]
    fn a_traversing_reference_is_described_as_refused() {
        assert!(describe_failure("../../etc/passwd").contains("leaves the model's own directory"));
        assert!(describe_failure("/etc/passwd").contains("leaves the model's own directory"));
        assert_eq!(describe_failure("beside.png"), "could not read");
    }
}

/// Collection against a real directory. Fixtures are generated here rather
/// than committed: a binary asset in a public repository for content this
/// trivial is not worth it, and the hostile case could not be committed
/// comfortably at all.
#[cfg(test)]
mod collecting {
    use super::*;

    /// A scratch directory named for the case, emptied on entry so a rerun
    /// after a failure does not inherit the previous attempt's files.
    fn scratch(case: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir()
            .join("solarxy-formats-companions")
            .join(case);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch directory");
        dir
    }

    fn write(dir: &Path, rel: &str, bytes: &[u8]) -> std::path::PathBuf {
        let full = dir.join(rel);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).expect("parent directory");
        }
        std::fs::write(&full, bytes).expect("fixture");
        full
    }

    fn names(companions: &Companions) -> Vec<String> {
        let mut names: Vec<String> = companions.assets.iter().map(|a| a.name.clone()).collect();
        names.sort();
        names
    }

    #[test]
    fn a_gltf_collects_its_buffer_and_an_image_from_a_subdirectory() {
        let dir = scratch("gltf-subdir");
        let json = br#"{"buffers":[{"uri":"model.bin"}],"images":[{"uri":"textures/albedo.png"}]}"#;
        let model = write(&dir, "model.gltf", json);
        write(&dir, "model.bin", b"\x00\x01\x02\x03");
        write(&dir, "textures/albedo.png", b"not really a png");

        let companions = collect(&model, "gltf", json).expect("collection");
        assert_eq!(
            names(&companions),
            vec!["model.bin".to_string(), "textures/albedo.png".to_string()],
            "the subdirectory is kept, because it is what read the file"
        );
        assert!(
            companions.warnings.is_empty(),
            "nothing was missing: {:?}",
            companions.warnings
        );
    }

    #[test]
    fn a_missing_gltf_buffer_is_an_error_naming_the_file() {
        let dir = scratch("gltf-missing-buffer");
        let json = br#"{"buffers":[{"uri":"absent.bin"}]}"#;
        let model = write(&dir, "model.gltf", json);

        let err = collect(&model, "gltf", json)
            .expect_err("a missing external buffer cannot be recovered from");
        assert!(err.message.contains("absent.bin"), "{}", err.message);
    }

    /// An image is not a hard failure: the loader records the URI and leaves
    /// the slot empty, so the render proceeds untextured and says so.
    #[test]
    fn a_missing_gltf_image_warns_and_carries_on() {
        let dir = scratch("gltf-missing-image");
        let json = br#"{"buffers":[{"uri":"model.bin"}],"images":[{"uri":"gone.png"}]}"#;
        let model = write(&dir, "model.gltf", json);
        write(&dir, "model.bin", b"\x00");

        let companions = collect(&model, "gltf", json).expect("collection");
        assert_eq!(companions.warnings.len(), 1, "{:?}", companions.warnings);
        assert!(
            companions.warnings[0].contains("gone.png"),
            "{}",
            companions.warnings[0]
        );
        assert_eq!(names(&companions), vec!["model.bin".to_string()]);
    }

    /// The security case. The file the URI points at exists and is readable,
    /// which is what makes this worth asserting: the refusal has to come from
    /// the rule, not from the file happening to be absent.
    #[test]
    fn a_reference_that_leaves_the_directory_is_refused_and_says_so() {
        let dir = scratch("gltf-traversal");
        write(&dir, "secret.txt", b"not yours");
        let inner = dir.join("model");
        std::fs::create_dir_all(&inner).expect("inner directory");
        let json = br#"{"buffers":[{"uri":"../secret.txt"}]}"#;
        let model = write(&inner, "model.gltf", json);

        let err =
            collect(&model, "gltf", json).expect_err("a traversing reference must be refused");
        assert!(
            err.message.contains("leaves the model's own directory"),
            "the refusal must say why: {}",
            err.message
        );
    }

    #[test]
    fn an_obj_collects_its_library_and_the_textures_that_library_names() {
        let dir = scratch("obj-transitive");
        let obj = b"mtllib m.mtl\nv 0 0 0\n";
        let model = write(&dir, "m.obj", obj);
        write(
            &dir,
            "m.mtl",
            b"newmtl a\nmap_Kd albedo.png\nmap_Bump -bm 0.2 normal.png\n",
        );
        write(&dir, "albedo.png", b"x");
        write(&dir, "normal.png", b"y");

        let companions = collect(&model, "obj", obj).expect("collection");
        assert_eq!(
            names(&companions),
            vec![
                "albedo.png".to_string(),
                "m.mtl".to_string(),
                "normal.png".to_string()
            ],
            "the walk is transitive: the library alone renders untextured"
        );
        assert!(companions.warnings.is_empty(), "{:?}", companions.warnings);
    }

    #[test]
    fn a_missing_material_library_degrades_to_default_materials() {
        let dir = scratch("obj-missing-mtl");
        let obj = b"mtllib absent.mtl\nv 0 0 0\n";
        let model = write(&dir, "m.obj", obj);

        let companions = collect(&model, "obj", obj).expect("an MTL is not required");
        assert_eq!(companions.warnings.len(), 1, "{:?}", companions.warnings);
        assert!(
            companions.warnings[0].contains("absent.mtl"),
            "{}",
            companions.warnings[0]
        );
    }

    /// The formats that carry everything themselves must name nothing extra,
    /// and the check should be obviously a no-op rather than incidentally one.
    #[test]
    fn a_self_contained_format_collects_nothing_beyond_the_file_itself() {
        let dir = scratch("self-contained");
        for ext in ["glb", "stl", "ply"] {
            let name = format!("m.{ext}");
            // Content that would name a companion if it were ever scanned.
            let body = b"mtllib m.mtl\n";
            let model = write(&dir, &name, body);
            let companions = collect(&model, ext, body).expect("collection");
            assert!(
                companions.assets.is_empty(),
                "{ext} is self-contained and must name nothing extra"
            );
            assert!(
                companions.warnings.is_empty(),
                "{ext}: {:?}",
                companions.warnings
            );
        }
    }
}
