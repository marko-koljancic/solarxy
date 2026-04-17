use std::fs;
use std::io::{self, IsTerminal};

use anyhow::Context;
use clap::Parser;

use solarxy_cli::parser::{Args, OperationMode, OutputFormat};
use solarxy_cli::tui_docs::DocsApp;
use solarxy_cli::tui_preferences::PreferencesApp;

#[cfg(feature = "analyzer")]
use solarxy_cli::calc::analyze::ModelAnalyzer;
#[cfg(feature = "analyzer")]
use solarxy_cli::tui_analysis::TerminalApp;

const APP_INFO: solarxy_cli::help::AppInfo = solarxy_cli::help::AppInfo {
    version: env!("CARGO_PKG_VERSION"),
    description: env!("CARGO_PKG_DESCRIPTION"),
    repository: env!("CARGO_PKG_REPOSITORY"),
    license: env!("CARGO_PKG_LICENSE"),
};

fn main() -> anyhow::Result<()> {
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
        return Ok(());
    }

    if args.update {
        return run_update();
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
        OperationMode::View => exec_gui(model_path.as_deref()),
        OperationMode::Analyze => run_analyze(model_path, args.format, args.output),
        OperationMode::Preferences => {
            PreferencesApp::new(preferences).run()?;
            Ok(())
        }
        OperationMode::Docs => {
            DocsApp::new(APP_INFO).run()?;
            Ok(())
        }
    }
}

fn exec_gui(model_path: Option<&str>) -> anyhow::Result<()> {
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
        Ok(status) if status.success() => Ok(()),
        Ok(status) => {
            std::process::exit(status.code().unwrap_or(1));
        }
        Err(e) => {
            eprintln!("Failed to launch solarxy GUI: {e}");
            eprintln!();
            eprintln!("The Solarxy GUI is distributed separately from the CLI:");
            eprintln!("  Linux:   flatpak install flathub dev.koljam.solarxy");
            eprintln!("  macOS:   brew install --cask koljam/solarxy/solarxy");
            eprintln!("  Windows: winget install Koljam.Solarxy");
            eprintln!();
            eprintln!("Or download from https://github.com/marko-koljancic/solarxy/releases");
            std::process::exit(127);
        }
    }
}

#[cfg(feature = "analyzer")]
fn run_analyze(
    model_path: Option<String>,
    format: OutputFormat,
    output: Option<std::path::PathBuf>,
) -> anyhow::Result<()> {
    let model_path =
        model_path.ok_or_else(|| anyhow::anyhow!("Model path required for analyze mode"))?;
    let analyzer = ModelAnalyzer::new(&model_path).context("Failed to load model")?;
    let report = analyzer.generate_report();

    let rendered = match format {
        OutputFormat::Json => solarxy_core::json::report_to_json(&report)?,
        OutputFormat::Text => report.to_string(),
    };

    if let Some(ref output_path) = output {
        std::fs::write(output_path, &rendered).context("Failed to write report")?;
        tracing::info!("Report written to {}", output_path.display());
        Ok(())
    } else if format == OutputFormat::Json && io::stdout().is_terminal() {
        let json_path = std::path::Path::new(&model_path).with_extension("json");
        std::fs::write(&json_path, &rendered).context("Failed to write JSON report")?;
        tracing::info!("Report written to {}", json_path.display());
        Ok(())
    } else if format == OutputFormat::Json || !io::stdout().is_terminal() {
        print!("{rendered}");
        Ok(())
    } else {
        TerminalApp::new(report, model_path).run()?;
        Ok(())
    }
}

#[cfg(not(feature = "analyzer"))]
fn run_analyze(
    _model_path: Option<String>,
    _format: OutputFormat,
    _output: Option<std::path::PathBuf>,
) -> anyhow::Result<()> {
    anyhow::bail!("Analyzer not available: rebuild solarxy-cli with the 'analyzer' feature")
}

#[cfg(feature = "updater")]
fn run_update() -> anyhow::Result<()> {
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

    use axoupdater::AxoUpdater;
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
