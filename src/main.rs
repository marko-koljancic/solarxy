use std::fs;
use std::io::{self, IsTerminal};

use clap::Parser;
use solarxy::run_viewer;

use crate::calc::analyze::ModelAnalyzer;
use crate::cli::parser::{Args, OperationMode, OutputFormat};
use crate::cli::tui::TerminalApp;

mod calc;
mod cli;

fn main() -> io::Result<()> {
    let args = Args::parse();

    let model_path = args.model_path.map(|p| {
        let canonical = fs::canonicalize(&p).expect("Failed to canonicalize the model path");
        canonical.to_string_lossy().to_string()
    });

    match args.mode {
        OperationMode::View => {
            if args.format == OutputFormat::Json {
                eprintln!("Error: --format json requires --mode analyze");
                std::process::exit(1);
            }
            run_viewer(model_path).unwrap();
            Ok(())
        }
        OperationMode::Analyze => {
            let model_path = model_path.expect("Model path is required for analyze mode (use -m <path>)");
            let analyzer = ModelAnalyzer::new(&model_path).expect("Failed to load model for analysis");
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
    }
}
