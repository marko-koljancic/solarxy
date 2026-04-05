use std::fs;
#[cfg(feature = "analyzer")]
use std::io::{self, IsTerminal};

use anyhow::Context;
use clap::Parser;

#[cfg(feature = "analyzer")]
use crate::calc::analyze::ModelAnalyzer;
use crate::cli::parser::{Args, OperationMode};
#[cfg(any(feature = "analyzer", feature = "viewer"))]
use crate::cli::parser::OutputFormat;
#[cfg(all(feature = "tui", feature = "analyzer"))]
use crate::cli::tui_analysis::TerminalApp;
#[cfg(feature = "tui")]
use crate::cli::tui_docs::DocsApp;
#[cfg(feature = "tui")]
use crate::cli::tui_preferences::PreferencesApp;

#[cfg(feature = "analyzer")]
mod calc;
mod cli;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
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

    let model_path = args
        .model_path
        .map(|p| -> anyhow::Result<String> {
            let canonical = fs::canonicalize(&p).context("Failed to canonicalize model path")?;
            Ok(canonical.to_string_lossy().to_string())
        })
        .transpose()?;

    let preferences = solarxy::preferences::load();

    match args.mode {
        OperationMode::View => {
            #[cfg(feature = "viewer")]
            {
                if args.format == OutputFormat::Json {
                    tracing::error!("--format json requires --mode analyze");
                    std::process::exit(1);
                }
                solarxy::run_viewer(model_path, preferences)?;
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
                    OutputFormat::Json => calc::json::report_to_json(&report),
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
                        let mut terminal = ratatui::init();
                        let app_result = TerminalApp::new(report, model_path).run(&mut terminal);
                        ratatui::restore();
                        Ok(app_result?)
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
                let mut terminal = ratatui::init();
                let app_result = PreferencesApp::new(preferences).run(&mut terminal);
                ratatui::restore();
                Ok(app_result?)
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
                let mut terminal = ratatui::init();
                let app_result = DocsApp::new().run(&mut terminal);
                ratatui::restore();
                Ok(app_result?)
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
