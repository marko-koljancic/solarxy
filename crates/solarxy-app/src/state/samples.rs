//! The bundled sample scenes, embedded rather than fetched.
//!
//! The browser fetches these from its own origin because it has no other
//! way to carry a file; a native application with no network should still
//! open them, so they are compiled in. They are read from the same
//! directory the browser serves, `web/public/samples/`, which is committed
//! source data written by a Rust example (`gen_samples`) rather than a web
//! build artifact, and a test pins the embedded list to that directory and
//! to the browser's menu.
//!
//! A sample opens as a document with no file, so Save asks for a path
//! rather than writing over the bundled copy.

use super::State;

/// One bundled scene: the browser's label, the file it is served as, and
/// its bytes.
pub(crate) struct Sample {
    pub label: &'static str,
    pub file: &'static str,
    pub bytes: &'static [u8],
}

macro_rules! sample {
    ($label:literal, $file:literal) => {
        Sample {
            label: $label,
            file: $file,
            bytes: include_bytes!(concat!("../../../../web/public/samples/", $file)),
        }
    };
}

/// The nine samples, in the browser's menu order: the flagship second to
/// last, and the Cornell box last because it teaches the renderer rather
/// than the graph.
pub(crate) const SAMPLES: &[Sample] = &[
    sample!("Modeling Basics", "modeling-basics.slxy"),
    sample!("Copy & Scatter", "copy-and-scatter.slxy"),
    sample!("Attributes & Displace", "attributes-and-displace.slxy"),
    sample!("Texture to Material", "texture-to-material.slxy"),
    sample!("Lights, Camera, Review", "lights-camera-review.slxy"),
    sample!("Animated Field", "animated-field.slxy"),
    sample!("Procedural Look-dev", "procedural-lookdev.slxy"),
    sample!("The Orrery", "the-orrery.slxy"),
    sample!("Cornell Box", "cornell-box.slxy"),
];

impl State {
    /// Open a sample, asking first when the document has unsaved changes.
    pub fn open_sample(&mut self, index: usize) {
        if index >= SAMPLES.len() {
            return;
        }
        self.guard_discard(super::discard::DiscardAction::OpenSample(index));
    }

    /// The action itself, past the guard.
    pub(super) fn open_sample_now(&mut self, index: usize) {
        let Some(sample) = SAMPLES.get(index) else {
            return;
        };
        self.adopt_scene_bytes(sample.bytes, sample.file, "");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn repo_root() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    /// The embedded set is the directory the browser serves, no more and
    /// no less, so a sample added to one place cannot be missing from the
    /// other.
    #[test]
    fn the_embedded_samples_are_the_served_directory() {
        let dir = repo_root().join("web/public/samples");
        let mut on_disk: Vec<String> = std::fs::read_dir(&dir)
            .expect("the samples directory is beside this crate")
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| {
                Path::new(n)
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("slxy"))
            })
            .collect();
        on_disk.sort();
        let mut embedded: Vec<String> = SAMPLES.iter().map(|s| s.file.to_string()).collect();
        embedded.sort();
        assert_eq!(embedded, on_disk);
        for sample in SAMPLES {
            let disk = std::fs::read(dir.join(sample.file)).expect("read");
            assert_eq!(
                sample.bytes,
                disk.as_slice(),
                "{} differs from disk",
                sample.file
            );
        }
    }

    /// The labels and their order are the browser's menu, read from its
    /// source rather than restated.
    #[test]
    fn the_labels_and_order_are_the_browsers_menu() {
        let src = std::fs::read_to_string(repo_root().join("web/src/components/menu/MenuBar.tsx"))
            .expect("the browser's menu bar");
        let start = src
            .find("const SAMPLE_SCENES")
            .expect("the browser's sample list");
        let end = src[start..].find("];").expect("the list closes") + start;
        let list = &src[start..end];
        let browser: Vec<(String, String)> = list
            .lines()
            .filter_map(|line| {
                let label = line.split("label: \"").nth(1)?.split('"').next()?;
                let file = line.split("file: \"").nth(1)?.split('"').next()?;
                Some((label.to_string(), file.to_string()))
            })
            .collect();
        let embedded: Vec<(String, String)> = SAMPLES
            .iter()
            .map(|s| (s.label.to_string(), s.file.to_string()))
            .collect();
        assert_eq!(embedded, browser);
    }

    /// Every embedded sample opens into a bare engine with no warning, so
    /// the menu never offers a file that toasts.
    #[test]
    fn every_embedded_sample_opens_clean() {
        for sample in SAMPLES {
            let mut engine = solarxy_graph::engine::Engine::new().expect("engine");
            let loaded = engine
                .load_slxy(sample.bytes)
                .unwrap_or_else(|e| panic!("{} does not open: {e}", sample.file));
            assert!(
                loaded.warnings.is_empty(),
                "{}: {:?}",
                sample.file,
                loaded.warnings
            );
        }
    }
}
