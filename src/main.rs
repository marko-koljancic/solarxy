use clap::Parser;
use crate::cli::parser::Args;
// use solarxy::run_viewer;

mod cli;

fn main() {
    // println!("::: Solarxy starting :::");
    // run_viewer().unwrap();

    let args = Args::parse();
    println!("Model path: {}", args.model_path.display());
    println!("Operation mode: {:?}", args.mode);
}
