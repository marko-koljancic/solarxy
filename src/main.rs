use clap::Parser;
use crate::cli::parser::{Args, OperationMode};
use solarxy::run_viewer;
use std::fs;

mod cli;

fn main() {
    let args = Args::parse();
    println!("::: Solarxy starting :::");
    println!("Model path >>> {}", args.model_path.display());
    println!("Operation mode >>> {:?}", args.mode);

    if args.mode == cli::parser::OperationMode::Analyze {
        todo!("Implement analyze mode");
    }

    match args.mode {
        OperationMode::View => {
            let model_path_buff = fs::canonicalize(&args.model_path).expect("Failed to canonicalize the model path");
            let model_path = model_path_buff.to_string_lossy().to_string();

            run_viewer(model_path).unwrap();
        }
        OperationMode::Analyze => todo!("Implement analyze mode"),
    }
}
