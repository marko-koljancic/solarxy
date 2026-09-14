#![warn(clippy::pedantic)]
#![allow(
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::module_name_repetitions,
    clippy::must_use_candidate,
    clippy::uninlined_format_args
)]
#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

use std::fs;
use std::path::PathBuf;

use anyhow::Context;
use clap::Parser;
use tracing_subscriber::prelude::*;

#[derive(Parser, Debug)]
#[command(version, about = "Solarxy 3D model viewer", long_about = None)]
struct GuiArgs {
    // Keeps its name: `--model` is a published surface, and renaming it
    // would break existing invocations and shell aliases to describe the
    // same thing more precisely. The help text carries the widened scope.
    #[arg(
        short = 'm',
        long = "model",
        help = "Path to the scene, model, or environment file to open at launch"
    )]
    model: Option<PathBuf>,
    #[arg(
        long,
        help = "Enable verbose logging (equivalent to --log-level debug)"
    )]
    verbose: bool,
    #[arg(
        long = "log-level",
        help = "Logging filter directive (e.g. 'solarxy=debug')"
    )]
    log_level: Option<String>,
}

fn main() -> anyhow::Result<()> {
    let args = GuiArgs::parse();

    let stderr_filter =
        tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            let directive = args.log_level.clone().unwrap_or_else(|| {
                if args.verbose {
                    "solarxy=debug,wgpu_hal=warn,wgpu_core=warn".into()
                } else {
                    "solarxy=info,wgpu_hal=error,wgpu_core=error".into()
                }
            });
            directive.into()
        });

    // One layer, to standard error. The in-app log panel and the layer that
    // fed it were withdrawn in 0.10.0, so the terminal is the shell's whole
    // diagnostic surface, as it was before the panel existed.
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_filter(stderr_filter),
        )
        .init();

    let model_path = args
        .model
        .map(|p| -> anyhow::Result<String> {
            let canonical = fs::canonicalize(&p).context("Failed to canonicalize the file path")?;
            Ok(canonical.to_string_lossy().to_string())
        })
        .transpose()?;

    let preferences = solarxy_core::preferences::load();

    solarxy_app::run_viewer(model_path, preferences)
}
