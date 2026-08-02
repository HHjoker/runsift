use std::process;

use anyhow::Result;
use clap::Parser;

use runsift::cli::{Cli, Command};
use runsift::{ai, capture};

fn main() {
    match try_main() {
        Ok(code) => process::exit(code),
        Err(error) => {
            eprintln!("runsift: {error:#}");
            process::exit(2);
        }
    }
}

fn try_main() -> Result<i32> {
    let cli = Cli::parse();

    match cli.command {
        Command::Run(args) => capture::run(args),
        Command::Context(args) => ai::context_command(args),
        Command::Analyze(args) => ai::analyze_command(args),
    }
}
