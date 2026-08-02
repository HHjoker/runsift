use std::fs;
use std::process::{Command, Output};

mod support;

fn runsift(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_runsift"))
        .args(args)
        .output()
        .unwrap()
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn top_level_help_explains_purpose_output_and_first_command() {
    let output = runsift(&["help"]);
    let help = text(&output);

    assert!(output.status.success());
    assert!(help.contains("captures a program or test run into a compact local evidence bundle"));
    assert!(help.contains("QUICK START:"));
    assert!(help.contains("runsift run -- ./build/unit_tests"));
    assert!(help.contains(".runsift/runs/<run_id>/"));
    assert!(help.contains("summary.md"));
    assert!(help.contains("runsift context .runsift/runs/<run_id>"));
    assert!(help.contains("Only the explicit `analyze` command"));
}

#[test]
fn context_and_analyze_help_make_network_behavior_explicit() {
    let context = text(&runsift(&["context", "--help"]));
    let analyze = text(&runsift(&["analyze", "--help"]));

    assert!(context.contains("This command is local-only"));
    assert!(context.contains("ai/context.json"));
    assert!(analyze.contains("No model is contacted unless this command is explicitly run"));
    assert!(analyze.contains("local"));
    assert!(analyze.contains("openai"));
}

#[test]
fn run_without_command_shows_complete_usage_instead_of_a_weak_error() {
    let output = runsift(&["run"]);
    let help = text(&output);

    assert_eq!(output.status.code(), Some(2));
    assert!(help.contains("Usage: runsift run [OPTIONS] -- <COMMAND>..."));
    assert!(help.contains("EVIDENCE INPUTS:"));
    assert!(help.contains("CORRELATION:"));
    assert!(help.contains("EXAMPLES:"));
    assert!(help.contains("The `--` separator is required"));
    assert!(!help.contains("required arguments were not provided"));
}

#[test]
fn run_help_explains_exit_code_and_structured_test_workflow() {
    let output = runsift(&["run", "--help"]);
    let help = text(&output);

    assert!(output.status.success());
    assert!(help.contains("returns the command's original exit code"));
    assert!(help.contains("--test-report build/ctest.xml"));
    assert!(help.contains("--output-junit build/ctest.xml"));
}

#[test]
fn command_must_follow_the_separator() {
    let output = runsift(&["run", "./build/unit_tests"]);
    let message = text(&output);

    assert!(!output.status.success());
    assert!(message.contains("unexpected argument './build/unit_tests'"));
    assert!(message.contains("Usage: runsift run [OPTIONS] -- <COMMAND>..."));
}

#[test]
fn run_redacts_secrets_in_manifest_command_arguments() {
    let directory = support::temp_directory("command-redaction");
    let output = runsift(&[
        "run",
        "--output",
        directory.to_str().unwrap(),
        "--run-id",
        "run_redaction",
        "--",
        "sh",
        "-c",
        "exit 0",
        "api_key=supersecret",
    ]);

    assert!(output.status.success());
    let manifest = fs::read_to_string(directory.join("run_redaction/manifest.json")).unwrap();
    assert!(!manifest.contains("supersecret"));
    assert!(manifest.contains("api_key=<redacted>"));

    let _ = fs::remove_dir_all(directory);
}
