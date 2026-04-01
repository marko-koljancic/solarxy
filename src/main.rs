use std::fs;
use std::io::{self, IsTerminal};

use clap::Parser;
use solarxy::run_viewer;

use crate::calc::analyze::ModelAnalyzer;
use crate::cli::parser::{Args, OperationMode, OutputFormat};
use crate::cli::tui::{PreferencesApp, TerminalApp};

mod calc;
mod cli;

fn main() -> io::Result<()> {
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

    let model_path = args.model_path.map(|p| {
        let canonical = fs::canonicalize(&p).expect("Failed to canonicalize the model path");
        canonical.to_string_lossy().to_string()
    });

    let preferences = solarxy::preferences::load();

    match args.mode {
        OperationMode::View => {
            if args.format == OutputFormat::Json {
                eprintln!("Error: --format json requires --mode analyze");
                std::process::exit(1);
            }
            run_viewer(model_path, preferences).unwrap();
            Ok(())
        }
        OperationMode::Analyze => {
            let model_path =
                model_path.expect("Model path is required for analyze mode (use -m <path>)");
            let analyzer =
                ModelAnalyzer::new(&model_path).expect("Failed to load model for analysis");
            let report = analyzer.generate_report();

            let output = match args.format {
                OutputFormat::Json => calc::json::report_to_json(&report),
                OutputFormat::Text => report.to_string(),
            };

            if let Some(ref output_path) = args.output {
                std::fs::write(output_path, &output).expect("Failed to write report file");
                eprintln!("Report written to {}", output_path.display());
                Ok(())
            } else if args.format == OutputFormat::Json && io::stdout().is_terminal() {
                let json_path = std::path::Path::new(&model_path).with_extension("json");
                std::fs::write(&json_path, &output).expect("Failed to write JSON report file");
                eprintln!("Report written to {}", json_path.display());
                Ok(())
            } else if args.format == OutputFormat::Json || !io::stdout().is_terminal() {
                print!("{output}");
                Ok(())
            } else {
                let mut terminal = ratatui::init();
                let app_result = TerminalApp::new(report, model_path).run(&mut terminal);
                ratatui::restore();
                app_result
            }
        }
        OperationMode::Preferences => {
            let mut terminal = ratatui::init();
            let app_result = PreferencesApp::new(preferences).run(&mut terminal);
            ratatui::restore();
            app_result
        }
    }
}
