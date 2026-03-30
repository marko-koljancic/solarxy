use clap::Parser;
use crate::cli::{
    parser::{Args, OperationMode},
    tui::TerminalApp,
};
use crate::calc::analyze::ModelAnalyzer;
use solarxy::{run_viewer};
use std::fs;

use std::io;

mod calc;
mod cli;

fn main() -> io::Result<()> {
    let args = Args::parse();
    let model_path_buff = fs::canonicalize(&args.model_path).expect("Failed to canonicalize the model path");
    let model_path = model_path_buff.to_string_lossy().to_string();

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
            let analyzer = ModelAnalyzer::new(&model_path).expect("Failed to load model for analysis");
            let mut terminal = ratatui::init();
            let app_result = TerminalApp::new(analyzer.generate_report()).run(&mut terminal);
            ratatui::restore();
            app_result
        }
    }
}
