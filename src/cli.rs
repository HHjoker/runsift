use std::ffi::OsString;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run a command and package its failure evidence.
    Run(RunArgs),
}

#[derive(Debug, Args)]
pub struct RunArgs {
    /// Log file to snapshot before the command and collect after it exits.
    #[arg(long = "log", value_name = "PATH")]
    pub logs: Vec<PathBuf>,

    /// Parent directory for generated run bundles.
    #[arg(short, long, default_value = ".runsift/runs")]
    pub output: PathBuf,

    /// Keep obvious secrets in the generated bundle.
    #[arg(long, default_value_t = false)]
    pub no_redact: bool,

    /// Command and arguments to execute. Place these after `--`.
    #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
    pub command: Vec<OsString>,
}
