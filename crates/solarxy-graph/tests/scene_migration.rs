//! Real scene files written by an earlier release, opened by this one.
//!
//! Every other migration test in this workspace builds its input with the
//! *current* writer, which proves the migration is self-consistent and proves
//! nothing about reading a file some earlier build actually produced. The
//! architecture set says so in as many words and files it as an open item.
//! These fixtures close it: they are byte-for-byte copies of shipped sample
//! scenes, taken before the vocabulary moved, so they carry the container type
//! ids, the sub-graph kinds and the parameter shapes of the release that wrote
//! them.
//!
//! They are deliberately NOT regenerated with the samples. A regenerated
//! sample is written straight at the current version by the sample generator
//! and never passes through a migration at all, so `sample_scenes.rs` gates
//! the *rename* and this file gates the *migration*. Replacing these bytes
//! would silently delete the only coverage the migration path has.

use solarxy_graph::engine::{Engine, EngineEvent};

/// Both fixtures, with the containers each was captured holding.
///
/// The names are asserted because an unnamed container resolves to its type's
/// display name, so a rename can silently change what an expression path
/// refers to. These carry explicit names and must keep them exactly.
const FIXTURES: &[(&str, &[&str])] = &[
    (
        "v1-the-orrery.slxy",
        &["bands", "relief", "materials", "orrery"],
    ),
    (
        "v1-texture-to-material.slxy",
        &["maps", "materials", "shaded"],
    ),
];

fn fixture(name: &str) -> Vec<u8> {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/scenes")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[test]
fn a_scene_from_an_earlier_release_opens_migrates_and_cooks_clean() {
    for (name, containers) in FIXTURES {
        let mut engine = Engine::new().expect("builtin registry");
        let loaded = engine
            .load_slxy(&fixture(name))
            .unwrap_or_else(|e| panic!("{name}: failed to load: {e}"));
        assert!(
            loaded.warnings.is_empty(),
            "{name}: loaded with warnings, which is what a partial migration looks like: {:?}",
            loaded.warnings
        );

        let mut errors: Vec<String> = Vec::new();
        for _ in 0..8 {
            let events = engine.cook(&mut || true);
            if events.is_empty() {
                break;
            }
            for ev in events {
                if let EngineEvent::CookStatus { node, status } = ev
                    && let solarxy_graph::cook::state::CookStatus::Error { message } = status
                {
                    errors.push(format!("{name}: node {node:?}: {message}"));
                }
            }
        }
        assert!(errors.is_empty(), "cook errors:\n{}", errors.join("\n"));
        assert!(
            !engine.display_geometries().is_empty(),
            "{name}: nothing displayed after cooking"
        );

        // Every container the file was captured holding is still there, still
        // answering to the name an expression would address it by.
        let root = engine
            .document()
            .graph(solarxy_graph::document::GraphContext::Root)
            .expect("the root graph");
        let names: Vec<String> = root
            .nodes()
            .filter(|n| engine.registry().opens(&n.type_id).is_some())
            .map(|n| solarxy_graph::naming::node_name(n, engine.registry()))
            .collect();
        for want in *containers {
            assert!(
                names.iter().any(|n| n == want),
                "{name}: the container '{want}' lost its name; resolved names were {names:?}"
            );
        }
    }
}

/// A migrated version-1 file IS its regenerated version-2 twin.
///
/// The weaker claim, that the old file opens and cooks without complaint, is
/// the test above and it would pass a migration that quietly dropped a
/// parameter. This one pins the strong claim the milestone actually makes:
/// each fixture was produced by the sample generator, the same generator now
/// emits the version-2 sample beside it, so migrating the old bytes must
/// arrive at exactly the document the new bytes describe. Anything the
/// migration forgot to carry, rename or back-fill shows up here as a
/// difference rather than as a scene that merely loads.
///
/// Compared through the document's own serialisation rather than the archive
/// bytes, because the archive also carries a generator string and an asset
/// table that have nothing to do with the migration.
#[test]
fn a_migrated_fixture_matches_its_regenerated_twin() {
    for (name, _) in FIXTURES {
        let current = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../web/public/samples")
            .join(name.trim_start_matches("v1-"));
        if !current.is_file() {
            eprintln!(
                "skipping {name}: no current sample at {}",
                current.display()
            );
            continue;
        }

        let mut old_engine = Engine::new().expect("builtin registry");
        old_engine
            .load_slxy(&fixture(name))
            .unwrap_or_else(|e| panic!("{name}: {e}"));

        let mut new_engine = Engine::new().expect("builtin registry");
        let bytes = std::fs::read(&current).expect("current sample readable");
        new_engine
            .load_slxy(&bytes)
            .unwrap_or_else(|e| panic!("{}: {e}", current.display()));

        let migrated = serde_json::to_value(old_engine.save_document()).expect("serialize");
        let regenerated = serde_json::to_value(new_engine.save_document()).expect("serialize");
        assert_eq!(
            migrated, regenerated,
            "{name} did not migrate to the document its regenerated twin describes"
        );
    }
}

/// The fixtures must stay stamped at the version they were captured at.
///
/// A well-meaning regeneration would rewrite them to whatever this build
/// writes and leave the test above asserting that the current writer can read
/// its own output, which is the exact vacuity these fixtures exist to remove.
/// The failure would be invisible, because a regenerated fixture passes every
/// other assertion in this file.
///
/// The archive stores `scene.json` uncompressed, so the stamp is legible in
/// the bytes without unzipping and this needs no dependency to read it.
#[test]
fn the_fixtures_are_still_stamped_at_version_one() {
    for (name, _) in FIXTURES {
        let bytes = fixture(name);
        let text = String::from_utf8_lossy(&bytes);
        let needle = "\"schema_version\":";
        let at = text
            .find(needle)
            .unwrap_or_else(|| panic!("{name}: no schema_version in the archive bytes"));
        let stamped: u32 = text[at + needle.len()..]
            .trim_start()
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse()
            .unwrap_or_else(|e| panic!("{name}: unreadable schema_version: {e}"));
        assert_eq!(
            stamped, 1,
            "{name} is no longer a version-1 file, so it has been regenerated and \
             the migration path is covered by nothing"
        );
    }
}
