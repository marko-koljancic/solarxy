use clap::Parser;
use crate::cli::parser::{Args, OperationMode};
use crate::calc::analyize::ModelAnalyzer;
use solarxy::{run_viewer};
use std::fs;

mod calc;
mod cli;

fn main() {
    let args = Args::parse();
    println!("::: Solarxy starting :::");
    println!("Model path >>> {}", args.model_path.display());
    println!("Operation mode >>> {:?}", args.mode);

    let model_path_buff = fs::canonicalize(&args.model_path).expect("Failed to canonicalize the model path");
    let model_path = model_path_buff.to_string_lossy().to_string();

    match args.mode {
        OperationMode::View => {
            println!("Launching viewer for model at path: {}", model_path);
            run_viewer(model_path).unwrap();
        }
        OperationMode::Analyze => {
            println!("Analyzing model at path: {}", model_path);
            let analyzer = ModelAnalyzer::new(&model_path).expect("Failed to load model for analysis");
            analyzer.run_analysis();
        }
    }
}
