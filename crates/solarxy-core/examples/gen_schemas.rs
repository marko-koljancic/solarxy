//! Regenerates `schemas/solarxy-config.v1.json` from the canonical
//! [`ProjectConfig`] type via `schemars`. Run from the workspace root:
//!
//! ```text
//! cargo run -p solarxy-core --features schemars-gen --example gen_schemas \
//!     > schemas/solarxy-config.v1.json
//! ```
//!
//! The companion test `tests/schema_drift.rs` asserts the checked-in file
//! matches the schema this example emits — so CI catches any schema change
//! that wasn't regenerated. Both go through `project_config::schema_json`.

#[cfg(feature = "schemars-gen")]
fn main() {
    let json = solarxy_core::project_config::schema_json().expect("generate JSON schema");
    println!("{json}");
}

#[cfg(not(feature = "schemars-gen"))]
fn main() {
    eprintln!("Rebuild with --features schemars-gen to regenerate the JSON schema.");
    std::process::exit(1);
}
