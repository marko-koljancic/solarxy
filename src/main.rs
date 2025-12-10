use clap::Parser;
use crate::cli::{
    parser::{Args, OperationMode},
    tui::TerminalApp,
};
use crate::calc::analyize::ModelAnalyzer;
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
            println!("Launching viewer for model at path: {}", model_path);
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
