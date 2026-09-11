//! What kind of thing a staged asset is, judged by its file name.
//!
//! The Assets panel on either shell groups its tiles by this, chooses a
//! thumbnail or a placeholder by it, and labels it. The rule is an
//! extension list, which is presentation rather than a fact about the
//! asset: the engine knows a MIME type, when the stager supplied one, and
//! this deliberately reads the name instead, because a file dropped from
//! disk arrives with a name and not always with a type.
//!
//! The browser's copy is `assetKind` in `web/src/components/AssetsPane.tsx`,
//! and a test here reads its three lists and its labels so the two shells
//! cannot sort one file into different bins.

/// The four bins an asset can fall into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetKind {
    /// A texture the panel can thumbnail.
    Image,
    /// A model the preview can orbit.
    Model,
    /// An environment map. Neither shell thumbnails one.
    Hdri,
    /// Anything else, shown as a file.
    Other,
}

impl AssetKind {
    /// The tile's caption for this bin.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Image => "Texture",
            Self::Model => "Model",
            Self::Hdri => "HDRI",
            Self::Other => "File",
        }
    }
}

/// Extensions the panel thumbnails.
pub const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp"];
/// Extensions the preview orbits.
pub const MODEL_EXTENSIONS: &[&str] = &["obj", "gltf", "glb", "stl", "ply"];
/// Extensions that light a scene.
pub const HDRI_EXTENSIONS: &[&str] = &["hdr", "exr"];

/// The bin for `name`, by its extension, compared without regard to case.
#[must_use]
pub fn asset_kind(name: &str) -> AssetKind {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    if IMAGE_EXTENSIONS.contains(&ext.as_str()) {
        AssetKind::Image
    } else if MODEL_EXTENSIONS.contains(&ext.as_str()) {
        AssetKind::Model
    } else if HDRI_EXTENSIONS.contains(&ext.as_str()) {
        AssetKind::Hdri
    } else {
        AssetKind::Other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bins_are_decided_by_extension_without_regard_to_case() {
        assert_eq!(asset_kind("wood.png"), AssetKind::Image);
        assert_eq!(asset_kind("WOOD.JPEG"), AssetKind::Image);
        assert_eq!(asset_kind("knot.obj"), AssetKind::Model);
        assert_eq!(asset_kind("scene.GLB"), AssetKind::Model);
        assert_eq!(asset_kind("studio.hdr"), AssetKind::Hdri);
        assert_eq!(asset_kind("notes.txt"), AssetKind::Other);
        assert_eq!(asset_kind("no-extension"), AssetKind::Other);
        assert_eq!(
            asset_kind("a.b.png"),
            AssetKind::Image,
            "the last extension decides"
        );
        assert_eq!(AssetKind::Image.label(), "Texture");
        assert_eq!(AssetKind::Other.label(), "File");
    }

    /// The three lists and the four labels are the browser's, read from its
    /// source rather than restated, so a file cannot be a texture on one
    /// shell and a file on the other.
    #[test]
    fn the_lists_and_labels_are_the_browsers() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../web/src/components/AssetsPane.tsx");
        let src =
            std::fs::read_to_string(&path).expect("the browser's assets pane is beside this crate");
        let list_after = |marker: &str| -> Vec<String> {
            let at = src
                .find(marker)
                .unwrap_or_else(|| panic!("{marker} in AssetsPane.tsx"));
            let open = src[at..].find('[').expect("a list opens") + at;
            let close = src[open..].find(']').expect("a list closes") + open;
            src[open + 1..close]
                .split(',')
                .map(|s| s.trim().trim_matches('"').to_string())
                .filter(|s| !s.is_empty())
                .collect()
        };
        let images = list_after("const ext = name.split");
        assert_eq!(images, IMAGE_EXTENSIONS);
        let models = list_after("return \"image\";");
        assert_eq!(models, MODEL_EXTENSIONS);
        let hdris = list_after("return \"model\";");
        assert_eq!(hdris, HDRI_EXTENSIONS);

        let labels = src
            .find("const KIND_LABEL")
            .map(|at| &src[at..])
            .expect("the browser's labels");
        for (key, kind) in [
            ("image", AssetKind::Image),
            ("model", AssetKind::Model),
            ("hdri", AssetKind::Hdri),
            ("other", AssetKind::Other),
        ] {
            assert!(
                labels.contains(&format!("{key}: \"{}\"", kind.label())),
                "the browser labels {key} differently from {:?}",
                kind.label()
            );
        }
    }
}
