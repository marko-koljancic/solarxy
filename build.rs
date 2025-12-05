use anyhow::*;
use fs_extra::copy_items;
use fs_extra::dir::CopyOptions;
use std::env;

/// Build script to copy resource files to output directory for use at runtime.
/// This is only used for testign purposes; in a real application, resources would be
/// suplied at runtime or bundled differently.
/// This script copies the 'res' directory to the output directory specified by Cargo.
/// It ensures that any changes in the 'res' directory trigger a rebuild.
fn main() -> Result<()> {
    println!("cargo:rerun-if-changed=res/*");
    let out_dir = env::var("OUT_DIR")?;
    let mut copy_options = CopyOptions::new();
    copy_options.overwrite = true;
    let mut paths_to_copy = Vec::new();
    paths_to_copy.push("res/");
    copy_items(&paths_to_copy, out_dir, &copy_options)?;
    Ok(())
}
