use std::fs;
#[cfg(feature = "analyzer")]
use std::io::{self, IsTerminal};

use anyhow::Context;
use clap::Parser;

#[cfg(feature = "analyzer")]
use crate::calc::analyze::ModelAnalyzer;
use solarxy_cli::parser::{Args, OperationMode};
#[cfg(any(feature = "analyzer", feature = "viewer"))]
use solarxy_cli::parser::OutputFormat;
#[cfg(all(feature = "tui", feature = "analyzer"))]
use solarxy_cli::tui_analysis::TerminalApp;
#[cfg(feature = "tui")]
use solarxy_cli::tui_docs::DocsApp;
#[cfg(feature = "tui")]
use solarxy_cli::tui_preferences::PreferencesApp;

#[cfg(feature = "analyzer")]
mod calc;

#[cfg(feature = "tui")]
const APP_INFO: solarxy_cli::help::AppInfo = solarxy_cli::help::AppInfo {
    version: env!("CARGO_PKG_VERSION"),
    description: env!("CARGO_PKG_DESCRIPTION"),
    repository: env!("CARGO_PKG_REPOSITORY"),
    license: env!("CARGO_PKG_LICENSE"),
};

fn main() -> anyhow::Result<()> {
    #[cfg(feature = "viewer")]
    let console_buffer = solarxy::console::new_log_buffer();

    #[cfg(feature = "viewer")]
    {
        use tracing_subscriber::prelude::*;
        let offset = time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC);
        let console_layer = solarxy::console::ConsoleLayer::new(console_buffer.clone(), offset)
            .with_filter(
                tracing_subscriber::EnvFilter::try_from_env("SOLARXY_CONSOLE_LOG")
                    .unwrap_or_else(|_| "solarxy=debug,wgpu_hal=warn,wgpu_core=warn".into()),
            );
        tracing_subscriber::registry()
            .with(
                tracing_subscriber::fmt::layer().with_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| "solarxy=info,wgpu_hal=error,wgpu_core=error".into()),
                ),
            )
            .with(console_layer)
            .init();
    }
    #[cfg(not(feature = "viewer"))]
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "solarxy=info,wgpu_hal=error,wgpu_core=error".into()),
        )
        .init();

    let args = Args::parse();

    if args.about {
        let version = env!("CARGO_PKG_VERSION");
        let description = env!("CARGO_PKG_DESCRIPTION");
        let repository = env!("CARGO_PKG_REPOSITORY");
        let license = env!("CARGO_PKG_LICENSE");

        println!("Solarxy {version}");
        println!("{description}");
        println!();
        println!("Repository   {repository}");
        println!("License      {license}");
        println!("Contact      https://koljam.com");
        return Ok(());
    }

    if args.update {
        #[cfg(feature = "updater")]
        {
            use axoupdater::AxoUpdater;
            let mut updater = AxoUpdater::new_for("solarxy");
            updater.load_receipt()?;
            if updater.run_sync()?.is_some() {
                println!("solarxy has been updated successfully.");
            } else {
                println!("solarxy is already up to date.");
            }
            return Ok(());
        }
        #[cfg(not(feature = "updater"))]
        anyhow::bail!("Updater not available: compile with the 'updater' feature to use --update");
    }

    let model_path = args
        .model_path
        .map(|p| -> anyhow::Result<String> {
            let canonical = fs::canonicalize(&p).context("Failed to canonicalize model path")?;
            Ok(canonical.to_string_lossy().to_string())
        })
        .transpose()?;

    let preferences = solarxy_core::preferences::load();

    match args.mode {
        OperationMode::View => {
            #[cfg(feature = "viewer")]
            {
                if args.format == OutputFormat::Json {
                    tracing::error!("--format json requires --mode analyze");
                    std::process::exit(1);
                }
                solarxy::run_viewer(model_path, preferences, console_buffer)?;
                Ok(())
            }
            #[cfg(not(feature = "viewer"))]
            {
                let _ = (model_path, preferences);
                anyhow::bail!(
                    "Viewer not available: compile with the 'viewer' feature to use --mode view"
                );
            }
        }
        OperationMode::Analyze => {
            #[cfg(feature = "analyzer")]
            {
                let model_path = model_path
                    .ok_or_else(|| anyhow::anyhow!("Model path required for analyze mode"))?;
                let analyzer = ModelAnalyzer::new(&model_path).context("Failed to load model")?;
                let report = analyzer.generate_report();

                let output = match args.format {
                    OutputFormat::Json => solarxy_core::json::report_to_json(&report)?,
                    OutputFormat::Text => report.to_string(),
                };

                if let Some(ref output_path) = args.output {
                    std::fs::write(output_path, &output).context("Failed to write report")?;
                    tracing::info!("Report written to {}", output_path.display());
                    Ok(())
                } else if args.format == OutputFormat::Json && io::stdout().is_terminal() {
                    let json_path = std::path::Path::new(&model_path).with_extension("json");
                    std::fs::write(&json_path, &output).context("Failed to write JSON report")?;
                    tracing::info!("Report written to {}", json_path.display());
                    Ok(())
                } else if args.format == OutputFormat::Json || !io::stdout().is_terminal() {
                    print!("{output}");
                    Ok(())
                } else {
                    #[cfg(feature = "tui")]
                    {
                        TerminalApp::new(report, model_path).run()?;
                        Ok(())
                    }
                    #[cfg(not(feature = "tui"))]
                    {
                        print!("{output}");
                        Ok(())
                    }
                }
            }
            #[cfg(not(feature = "analyzer"))]
            {
                let _ = model_path;
                anyhow::bail!(
                    "Analyzer not available: compile with the 'analyzer' feature to use --mode analyze"
                );
            }
        }
        OperationMode::Preferences => {
            #[cfg(feature = "tui")]
            {
                PreferencesApp::new(preferences).run()?;
                Ok(())
            }
            #[cfg(not(feature = "tui"))]
            {
                let _ = preferences;
                anyhow::bail!(
                    "TUI not available: compile with the 'tui' feature to use --mode preferences"
                );
            }
        }
        OperationMode::Docs => {
            #[cfg(feature = "tui")]
            {
                DocsApp::new(APP_INFO).run()?;
                Ok(())
            }
            #[cfg(not(feature = "tui"))]
            {
                anyhow::bail!(
                    "TUI not available: compile with the 'tui' feature to use --mode docs"
                );
            }
        }
    }
}
