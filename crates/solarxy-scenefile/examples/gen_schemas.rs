//! Regenerates `schemas/slxy-scene.v0.json` from the canonical
//! [`solarxy_scenefile::SceneJson`] type via `schemars`. Run from the
//! workspace root:
//!
//! ```text
//! cargo run -p solarxy-scenefile --features schemars-gen --example gen_schemas \
//!     > schemas/slxy-scene.v0.json
//! ```
//!
//! The companion test `tests/schema_drift.rs` asserts the checked-in file
//! matches this output, so CI catches any schema change that was not
//! regenerated. Both go through `solarxy_scenefile::schema_json`.

#[cfg(feature = "schemars-gen")]
fn main() {
    let json = solarxy_scenefile::schema_json().expect("generate JSON schema");
    println!("{json}");
}

#[cfg(not(feature = "schemars-gen"))]
fn main() {
    eprintln!("Rebuild with --features schemars-gen to regenerate the JSON schema.");
    std::process::exit(1);
}
