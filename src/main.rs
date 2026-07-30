mod capture;
mod cli;
mod git;
mod logs;
mod model;
mod pattern;
mod redact;
mod report;

use std::process;

use anyhow::Result;
use clap::Parser;

use crate::cli::{Cli, Command};

fn main() {
    match try_main() {
        Ok(code) => process::exit(code),
        Err(error) => {
            eprintln!("loglens: {error:#}");
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
