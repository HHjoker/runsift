use std::process;

use anyhow::Result;
use clap::Parser;

use runsift::capture;
use runsift::cli::{Cli, Command};

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
    }
}
