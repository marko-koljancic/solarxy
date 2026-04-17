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
    #[arg(
        short = 'm',
        long = "model",
        help = "Path to the model file to open at launch"
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

    let console_buffer = solarxy::console::new_log_buffer();
    let offset = time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC);

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

    let console_layer = solarxy::console::ConsoleLayer::new(console_buffer.clone(), offset)
        .with_filter(
            tracing_subscriber::EnvFilter::try_from_env("SOLARXY_CONSOLE_LOG")
                .unwrap_or_else(|_| "solarxy=debug,wgpu_hal=warn,wgpu_core=warn".into()),
        );

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_filter(stderr_filter))
        .with(console_layer)
        .init();

    let model_path = args
        .model
        .map(|p| -> anyhow::Result<String> {
            let canonical = fs::canonicalize(&p).context("Failed to canonicalize model path")?;
            Ok(canonical.to_string_lossy().to_string())
        })
        .transpose()?;

    let preferences = solarxy_core::preferences::load();

    solarxy::run_viewer(model_path, preferences, console_buffer)
}
