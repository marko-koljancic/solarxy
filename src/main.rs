use std::fs;
use std::io::{self, IsTerminal};

use clap::Parser;
use solarxy::run_viewer;

use crate::calc::analyze::ModelAnalyzer;
use crate::cli::parser::{Args, OperationMode};
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
            println!();
            println!("  SolarXY Viewer Controls");
            println!();
            println!("  View");
            println!("    W          Cycle modes (Shaded / Shaded+Wire / Wireframe)");
            println!("    S          Shaded mode");
            println!("    X          Toggle Ghosted");
            println!("    N          Cycle normals (Off / Face / Vertex / Face+Vertex)");
            println!();
            println!("  Camera");
            println!("    H          Frame (reset view)");
            println!("    T / F      Top / Front");
            println!("    L / R      Left / Right");
            println!("    P / O      Perspective / Orthographic");
            println!("    Left drag  Orbit");
            println!("    Mid drag   Pan");
            println!("    Scroll     Zoom");
            println!();
            println!("  Other");
            println!("    ?          Toggle shortcut hints");
            println!("    Esc        Exit");
            println!();
            run_viewer(model_path).unwrap();
            Ok(())
        }
        OperationMode::Analyze => {
            let model_path = model_path.expect("Model path is required for analyze mode (use -m <path>)");
            let analyzer = ModelAnalyzer::new(&model_path).expect("Failed to load model for analysis");
            let report = analyzer.generate_report();

            if let Some(ref output_path) = args.output {
                std::fs::write(output_path, report.to_string()).expect("Failed to write report file");
                eprintln!("Report written to {}", output_path.display());
                Ok(())
            } else if !io::stdout().is_terminal() {
                print!("{}", report);
                Ok(())
            } else {
                let mut terminal = ratatui::init();
                let app_result = TerminalApp::new(report).run(&mut terminal);
                ratatui::restore();
                app_result
            }
        }
    }
}
