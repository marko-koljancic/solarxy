use clap::Parser;
use crate::cli::parser::Args;
use solarxy::run_viewer;

mod cli;

fn main() {
    let args = Args::parse();
    println!("::: Solarxy starting :::");
    println!("Model path >>> {}", args.model_path.display());
    println!("Operation mode >>> {:?}", args.mode);

    if args.mode == cli::parser::OperationMode::View {
        println!("Analyze mode is not yet implemented.");
        return;
    }

    let model_path = args.model_path.to_string_lossy().to_string();
    run_viewer(model_path).unwrap();
}
