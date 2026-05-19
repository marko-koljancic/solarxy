//! Regenerates `schemas/solarxy-config.v1.json` from the canonical
//! [`ProjectConfig`] type via `schemars`. Run from the workspace root:
//!
//! ```text
//! cargo run -p solarxy-core --features schemars-gen --example gen_schemas \
//!     > schemas/solarxy-config.v1.json
//! ```
//!
//! The companion test `tests/schema_drift.rs` asserts the file on disk is
//! byte-equal to the schema this example would emit — so CI catches any
//! schema change that wasn't regenerated.

#[cfg(feature = "schemars-gen")]
fn main() {
    use solarxy_core::project_config::ProjectConfig;
    let schema = schemars::schema_for!(ProjectConfig);
    let json = serde_json::to_string_pretty(&schema).expect("serialize schema");
    println!("{json}");
}

#[cfg(not(feature = "schemars-gen"))]
fn main() {
    eprintln!("Rebuild with --features schemars-gen to regenerate the JSON schema.");
    std::process::exit(1);
}
