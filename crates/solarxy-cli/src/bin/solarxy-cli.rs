use std::fs;
use std::io::{self, IsTerminal};
use std::path::Path;
use std::process::ExitCode;

use anyhow::Context;
use clap::Parser;

use solarxy_cli::parser::{Args, OperationMode, OutputFormat};

#[cfg(feature = "analyzer")]
use solarxy_cli::calc::analyze::ModelAnalyzer;
#[cfg(feature = "analyzer")]
use solarxy_cli::tui_analysis::TerminalApp;
#[cfg(feature = "analyzer")]
use solarxy_validate::{
    self as validate, ConfigSource, Output as ValidateOutput,
    adapter::{AdapterFormat, AdapterName, FailOn},
};

fn main() -> anyhow::Result<ExitCode> {
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

        println!("Solarxy CLI {version}");
        println!("{description}");
        println!();
        println!("Repository   {repository}");
        println!("License      {license}");
        println!("Contact      https://koljam.com");
        return Ok(ExitCode::SUCCESS);
    }

    if args.list_tui_themes {
        solarxy_cli::print_theme_listing();
        return Ok(ExitCode::SUCCESS);
    }

    if args.update {
        return run_update().map(|()| ExitCode::SUCCESS);
    }

    let model_path = args
        .model_path
        .map(|p| -> anyhow::Result<String> {
            let canonical = fs::canonicalize(&p).context("Failed to canonicalize model path")?;
            Ok(canonical.to_string_lossy().to_string())
        })
        .transpose()?;

    match args.mode {
        OperationMode::View => Ok(exec_gui(model_path.as_deref())),
        OperationMode::Analyze => {
            if !args.paths.is_empty() {
                run_validate(
                    &args.paths,
                    args.config.as_deref(),
                    args.adapter,
                    args.adapter_format,
                    args.fail_on,
                    args.output.as_deref(),
                )
                .map(|code| u8::try_from(code).map_or(ExitCode::FAILURE, ExitCode::from))
            } else {
                run_analyze(
                    model_path,
                    &args.format,
                    args.output.as_deref(),
                    args.config.as_deref(),
                    args.tui_theme.as_deref(),
                )
                .map(|()| ExitCode::SUCCESS)
            }
        }
    }
}

fn exec_gui(model_path: Option<&str>) -> ExitCode {
    let gui_bin_name = if cfg!(target_os = "windows") {
        "solarxy.exe"
    } else {
        "solarxy"
    };

    let gui_path = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(gui_bin_name)))
        .filter(|p| p.exists());

    let mut cmd = match gui_path {
        Some(p) => std::process::Command::new(p),
        None => std::process::Command::new("solarxy"),
    };

    if let Some(m) = model_path {
        cmd.arg("--model").arg(m);
    }

    match cmd.status() {
        Ok(status) => status
            .code()
            .and_then(|c| u8::try_from(c).ok())
            .map_or(ExitCode::FAILURE, ExitCode::from),
        Err(e) => {
            eprintln!("Failed to launch solarxy GUI: {e}");
            eprintln!();
            eprintln!("The Solarxy GUI is distributed separately from the CLI:");
            eprintln!("  Linux:   flatpak install flathub dev.koljam.solarxy");
            eprintln!("  macOS:   brew install --cask koljam/solarxy/solarxy");
            eprintln!("  Windows: winget install Koljam.Solarxy");
            eprintln!();
            eprintln!("Or download from https://github.com/marko-koljancic/solarxy/releases");
            ExitCode::from(127)
        }
    }
}

#[cfg(feature = "analyzer")]
fn run_analyze(
    model_path: Option<String>,
    format: &OutputFormat,
    output: Option<&Path>,
    config: Option<&Path>,
    tui_theme: Option<&str>,
) -> anyhow::Result<()> {
    let model_path =
        model_path.ok_or_else(|| anyhow::anyhow!("Model path required for analyze mode"))?;
    let analyzer =
        ModelAnalyzer::new_with_config(&model_path, config).context("Failed to load model")?;
    let report = analyzer.generate_report();

    let rendered = match format {
        OutputFormat::Json => solarxy_core::json::report_to_json(&report)?,
        OutputFormat::Text => report.to_string(),
    };

    if let Some(output_path) = output {
        std::fs::write(output_path, &rendered).context("Failed to write report")?;
        tracing::info!("Report written to {}", output_path.display());
        Ok(())
    } else if *format == OutputFormat::Json && io::stdout().is_terminal() {
        let json_path = std::path::Path::new(&model_path).with_extension("json");
        std::fs::write(&json_path, &rendered).context("Failed to write JSON report")?;
        tracing::info!("Report written to {}", json_path.display());
        Ok(())
    } else if *format == OutputFormat::Json || !io::stdout().is_terminal() {
        print!("{rendered}");
        Ok(())
    } else if let Some(reason) = terminal_too_small() {
        // Decided before the screen is taken, so the notice lands on the
        // normal terminal rather than flashing on an alternate one that is
        // about to be torn down.
        eprintln!("{reason}");
        print!("{rendered}");
        Ok(())
    } else if solarxy_cli::tiled_analyze_requested() {
        solarxy_cli::run_tiled_analyze(&report, tui_theme)?;
        Ok(())
    } else {
        TerminalApp::new(report, model_path, tui_theme).run()?;
        Ok(())
    }
}

/// Whether this terminal is below the size the analyze surface needs.
///
/// A terminal that will not report its own size is taken at its word rather
/// than refused: the surface is the better experience when it fits, and
/// guessing "too small" would deny it to anyone whose terminal is merely
/// reticent.
#[cfg(feature = "analyzer")]
fn terminal_too_small() -> Option<String> {
    let (width, height) = crossterm::terminal::size().ok()?;
    solarxy_cli::terminal_floor_notice(width, height)
}

#[cfg(not(feature = "analyzer"))]
fn run_analyze(
    _model_path: Option<String>,
    _format: &OutputFormat,
    _output: Option<&Path>,
    _config: Option<&Path>,
    _tui_theme: Option<&str>,
) -> anyhow::Result<()> {
    anyhow::bail!("Analyzer not available: rebuild solarxy-cli with the 'analyzer' feature")
}

#[cfg(feature = "analyzer")]
fn run_validate(
    paths: &[String],
    config_path: Option<&Path>,
    adapter_name: AdapterName,
    adapter_format: Option<AdapterFormat>,
    fail_on: FailOn,
    output: Option<&Path>,
) -> anyhow::Result<i32> {
    let source = match config_path {
        Some(p) => ConfigSource::load(p)?,
        None => ConfigSource::discover(Path::new("."))?,
    };
    let adapter = validate::resolve_adapter(adapter_name);
    let format = adapter_format.unwrap_or_else(|| adapter.default_format());
    let out = ValidateOutput::from_path(output);
    validate::run_validation(paths, source, adapter.as_ref(), format, fail_on, &out)
        .map_err(anyhow::Error::from)
}

#[cfg(not(feature = "analyzer"))]
fn run_validate(
    _paths: &[String],
    _config_path: Option<&Path>,
    _adapter_name: AdapterName,
    _adapter_format: Option<AdapterFormat>,
    _fail_on: FailOn,
    _output: Option<&Path>,
) -> anyhow::Result<i32> {
    anyhow::bail!("Validator not available: rebuild solarxy-cli with the 'analyzer' feature")
}

#[cfg(feature = "updater")]
fn run_update() -> anyhow::Result<()> {
    use axoupdater::AxoUpdater;
    use solarxy_core::install_source::{InstallSource, detect};

    match detect() {
        InstallSource::HomebrewFormula => {
            eprintln!("This Solarxy CLI was installed via Homebrew. Update with:");
            eprintln!("  brew upgrade solarxy-cli");
            return Ok(());
        }
        InstallSource::Flatpak => {
            eprintln!("This Solarxy is running inside Flatpak — update via Flathub:");
            eprintln!("  flatpak update dev.koljam.solarxy");
            return Ok(());
        }
        _ => {}
    }

    let mut updater = AxoUpdater::new_for("solarxy-cli");
    updater.load_receipt()?;
    if updater.run_sync()?.is_some() {
        println!("solarxy-cli has been updated successfully.");
    } else {
        println!("solarxy-cli is already up to date.");
    }
    Ok(())
}

#[cfg(not(feature = "updater"))]
fn run_update() -> anyhow::Result<()> {
    anyhow::bail!("Updater not available: rebuild solarxy-cli with the 'updater' feature")
}
