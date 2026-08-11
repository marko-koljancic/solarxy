//! The two inputs, and that they arrive at the same place.
//!
//! No device is needed for any of this, deliberately: everything up to the
//! moment a backend ingests the delta is the engine's work, and the engine has
//! no GPU. That matters because the continuous-integration machine has no
//! adapter, so a test that needed one would be a test that never ran.

use std::path::{Path, PathBuf};

use solarxy_render::input;

fn repo_root() -> PathBuf {
    // The crate directory, then up twice: `crates/solarxy-render`.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate sits two levels below the workspace root")
        .to_path_buf()
}

/// A model file becomes a document that has cooked into real geometry.
///
/// This is the whole bare-model adapter in one assertion, and every step of it
/// is a place it could silently do nothing: staging the bytes, creating the
/// container, creating the import inside it, pointing the import at the staged
/// asset, and flagging it as what to display.
///
/// It also covers the cook loop, which is the part most likely to be written
/// wrong. An import does not parse during a cook: it queues a job, and the
/// geometry only exists after that job has been resolved and handed back. A
/// loop that cooked once, or that cooked several times without ever draining
/// jobs, would leave this document with no geometry at all and no error to say
/// so. The engine's own sample-scene tests do not catch it, because every
/// bundled sample is fully parametric and queues no jobs.
#[test]
fn a_model_file_cooks_into_geometry_through_the_synthesized_document() {
    let model = repo_root().join("res/models/armadillo.obj");
    assert!(model.exists(), "the fixture model is missing: {model:?}");

    let loaded = input::load(&model, None).expect("a model file loads");
    let displayed = loaded.engine.display_geometries();

    assert!(
        !displayed.is_empty(),
        "the synthesized document displays nothing, which is what an undrained \
         import job looks like from here"
    );
    let points: usize = displayed
        .iter()
        .map(|(_, set, _)| {
            set.meshes
                .iter()
                .map(|m| m.positions.len() / 3)
                .sum::<usize>()
        })
        .sum();
    assert!(
        points > 1000,
        "the displayed geometry has {points} points, which is not this model"
    );
}

/// Both inputs leave through one door.
///
/// The value here is structural rather than behavioural: it pins that the two
/// adapters return the same type, so a later change cannot quietly give one of
/// them its own render path. That was the failure the single-path decision
/// exists to prevent, and a type is the only thing that can hold it.
#[test]
fn both_inputs_produce_a_cooked_engine_of_the_same_kind() {
    let model = repo_root().join("res/models/armadillo.obj");
    let loaded = input::load(&model, None).expect("a model file loads");

    // The scene adapter is exercised by round-tripping what the model adapter
    // built, which is also the strongest available statement that the two are
    // interchangeable: the same document, through the other door.
    let bytes = loaded
        .engine
        .save_slxy(&solarxy_graph::engine::SceneSidecar::default())
        .expect("the synthesized document saves");

    let dir = std::env::temp_dir().join("solarxy-render-adapters");
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    let scene = dir.join("round-trip.slxy");
    std::fs::write(&scene, bytes).expect("the scene writes");

    let reopened = input::load(&scene, None).expect("the scene file loads");
    assert!(
        !reopened.engine.display_geometries().is_empty(),
        "the scene adapter produced no geometry from a document the model \
         adapter had just built"
    );
    let _ = std::fs::remove_file(&scene);
}

/// Something that is neither is refused by kind rather than by parse failure.
#[test]
fn an_unsupported_file_is_refused_before_anything_is_read_into_the_engine() {
    let dir = std::env::temp_dir().join("solarxy-render-adapters");
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    let odd = dir.join("notes.txt");
    std::fs::write(&odd, b"not a model").expect("the file writes");

    let Err(err) = input::load(&odd, None) else {
        panic!("a text file is not renderable");
    };
    assert!(
        matches!(err, solarxy_render::RenderError::InputUnsupported { .. }),
        "expected the unsupported-kind error, got {err}"
    );
    let _ = std::fs::remove_file(&odd);
}

/// A path that is not there says so, rather than failing later and vaguely.
#[test]
fn a_missing_input_is_its_own_failure() {
    let Err(err) = input::load(Path::new("does-not-exist.slxy"), None) else {
        panic!("nothing to load");
    };
    assert!(matches!(err, solarxy_render::RenderError::InputMissing(_)));
}
