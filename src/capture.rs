use std::ffi::OsString;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;

use anyhow::{Context, Result, bail};
use chrono::Utc;

use crate::cli::RunArgs;
use crate::crash;
use crate::diagnostics;
use crate::git;
use crate::logs;
use crate::model::{CommandResult, CorrelationContext, CrashEvidence, Manifest};
use crate::pattern;
use crate::profile::LogProfile;
use crate::redact;
use crate::report;
use crate::test_report;

#[derive(Debug, Clone, Copy)]
enum Stream {
    Stdout,
    Stderr,
}

#[derive(Debug)]
struct Chunk {
    stream: Stream,
    bytes: Vec<u8>,
}

pub fn run(args: RunArgs) -> Result<i32> {
    let (program, command_args) = args
        .command
        .split_first()
        .context("a command is required after `--`")?;
    let cwd = std::env::current_dir().context("failed to determine working directory")?;
    let started_at = Utc::now();
    let run_id = match args.run_id.as_deref() {
        Some(value) => validate_id("run ID", value)?,
        None => format!(
            "run_{}_{}",
            started_at.format("%Y%m%dT%H%M%S%.3fZ"),
            std::process::id()
        ),
    };
    let context = CorrelationContext {
        run_id: run_id.clone(),
        batch_id: args
            .batch_id
            .as_deref()
            .map(|value| validate_id("batch ID", value))
            .transpose()?,
        test_id: args
            .test_id
            .as_deref()
            .map(|value| validate_id("test ID", value))
            .transpose()?,
    };
    let bundle_dir = args.output.join(&run_id);
    if bundle_dir.exists() {
        bail!(
            "evidence bundle {} already exists; choose a different run ID",
            bundle_dir.display()
        );
    }
    fs::create_dir_all(bundle_dir.join("logs"))
        .with_context(|| format!("failed to create {}", bundle_dir.display()))?;
    fs::create_dir_all(bundle_dir.join("debugger"))
        .with_context(|| format!("failed to create {}", bundle_dir.display()))?;
    fs::create_dir_all(bundle_dir.join("tests"))
        .with_context(|| format!("failed to create {}", bundle_dir.display()))?;
    let log_profile = args
        .log_profile
        .as_deref()
        .map(LogProfile::load)
        .transpose()?;

    let snapshots = args
        .logs
        .into_iter()
        .map(logs::snapshot)
        .collect::<Vec<_>>();
    let git_info = git::inspect(&cwd);
    let redact_enabled = !args.no_redact;

    let mut child = Command::new(program)
        .args(command_args)
        .current_dir(&cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to start {}", program.to_string_lossy()))?;

    let stdout = child.stdout.take().context("failed to capture stdout")?;
    let stderr = child.stderr.take().context("failed to capture stderr")?;
    let (sender, receiver) = mpsc::channel();
    let stdout_reader = spawn_reader(stdout, Stream::Stdout, sender.clone());
    let stderr_reader = spawn_reader(stderr, Stream::Stderr, sender);
    let mut captured_stdout = String::new();
    let mut captured_stderr = String::new();

    for chunk in receiver {
        match chunk.stream {
            Stream::Stdout => {
                std::io::stdout().write_all(&chunk.bytes)?;
                std::io::stdout().flush()?;
                captured_stdout.push_str(&redact::text(
                    &String::from_utf8_lossy(&chunk.bytes),
                    redact_enabled,
                ));
            }
            Stream::Stderr => {
                std::io::stderr().write_all(&chunk.bytes)?;
                std::io::stderr().flush()?;
                captured_stderr.push_str(&redact::text(
                    &String::from_utf8_lossy(&chunk.bytes),
                    redact_enabled,
                ));
            }
        }
    }

    join_reader(stdout_reader)?;
    join_reader(stderr_reader)?;
    let status = child.wait().context("failed to wait for child command")?;
    let finished_at = Utc::now();

    fs::write(bundle_dir.join("stdout.log"), &captured_stdout)?;
    fs::write(bundle_dir.join("stderr.log"), &captured_stderr)?;

    let mut events = Vec::new();
    events.extend(logs::parse_events(
        &bundle_dir.join("stdout.log"),
        "stdout.log",
        0,
        &captured_stdout,
        finished_at,
        log_profile.as_ref(),
        &context,
    ));
    events.extend(logs::parse_events(
        &bundle_dir.join("stderr.log"),
        "stderr.log",
        0,
        &captured_stderr,
        finished_at,
        log_profile.as_ref(),
        &context,
    ));

    let mut sources = Vec::new();
    let mut artifacts = vec![
        "manifest.json".to_owned(),
        "summary.md".to_owned(),
        "events.jsonl".to_owned(),
        "patterns.json".to_owned(),
        "tests.json".to_owned(),
        "diagnostics.json".to_owned(),
        "crash.json".to_owned(),
        "stdout.log".to_owned(),
        "stderr.log".to_owned(),
    ];

    for (index, snapshot) in snapshots.into_iter().enumerate() {
        match logs::collect(
            snapshot,
            &format!("logs/{index:03}"),
            finished_at,
            redact_enabled,
            log_profile.as_ref(),
            &context,
        ) {
            Ok(collected) => {
                for delta in collected.deltas {
                    let path = bundle_dir.join(&delta.artifact);
                    fs::write(&path, &delta.content)
                        .with_context(|| format!("failed to write {}", path.display()))?;
                    artifacts.push(delta.artifact);
                    events.extend(delta.events);
                }
                sources.push(collected.summary);
            }
            Err(error) => eprintln!("runsift: warning: {error:#}"),
        }
    }

    events.sort_by(|left, right| {
        left.timestamp
            .unwrap_or(left.observed_at)
            .cmp(&right.timestamp.unwrap_or(right.observed_at))
            .then_with(|| left.event_id.cmp(&right.event_id))
    });
    let patterns = pattern::aggregate(&events);
    let diagnostics = diagnostics::parse(
        &captured_stderr,
        &bundle_dir.join("stderr.log"),
        "stderr.log",
        &context,
        &events,
    );

    let mut test_reports = Vec::new();
    for (index, path) in args.test_reports.iter().enumerate() {
        let artifact = test_report::artifact(index, path);
        match test_report::import(path, artifact.clone(), redact_enabled) {
            Ok(imported) => {
                fs::write(bundle_dir.join(&artifact), imported.content)?;
                artifacts.push(artifact);
                test_reports.push(imported.report);
            }
            Err(error) => eprintln!(
                "runsift: warning: failed to parse test report {}: {error:#}",
                path.display()
            ),
        }
    }

    let mut core_dumps = Vec::new();
    for path in &args.core_dumps {
        match crash::inspect_core(path) {
            Ok(value) => core_dumps.push(value),
            Err(error) => eprintln!("runsift: warning: {error:#}"),
        }
    }
    let mut debugger_reports = Vec::new();
    for (index, path) in args.debugger_reports.iter().enumerate() {
        let artifact = crash::debugger_artifact(index, path);
        match crash::import_debugger_report(path, artifact.clone(), &context, redact_enabled) {
            Ok(imported) => {
                fs::write(bundle_dir.join(&artifact), imported.content)?;
                artifacts.push(artifact);
                debugger_reports.push(imported.report);
            }
            Err(error) => eprintln!("runsift: warning: {error:#}"),
        }
    }
    let crash_evidence = CrashEvidence {
        core_dumps,
        debugger_reports,
    };
    let test_count = test_reports.iter().map(|value| value.total).sum();
    let failed_test_count = test_reports
        .iter()
        .map(|value| value.failed + value.errors)
        .sum();

    let command_result = CommandResult {
        program: redact::text(&display(program), redact_enabled),
        args: command_args
            .iter()
            .map(|value| redact::text(&display(value), redact_enabled))
            .collect(),
        exit_code: status.code(),
        success: status.success(),
    };
    let manifest = Manifest {
        schema_version: 2,
        run_id: run_id.clone(),
        context,
        started_at,
        finished_at,
        working_directory: cwd,
        command: command_result,
        redacted: redact_enabled,
        git: git_info,
        sources,
        event_count: events.len(),
        pattern_count: patterns.len(),
        test_count,
        failed_test_count,
        diagnostic_count: diagnostics.len(),
        core_dump_count: crash_evidence.core_dumps.len(),
        debugger_report_count: crash_evidence.debugger_reports.len(),
        log_profile: log_profile.map(|value| value.name().to_owned()),
        artifacts,
    };
    report::write_bundle(
        &bundle_dir,
        &manifest,
        &events,
        &patterns,
        &test_reports,
        &diagnostics,
        &crash_evidence,
    )?;

    eprintln!("runsift: evidence bundle: {}", bundle_dir.display());
    Ok(status.code().unwrap_or(1))
}

fn validate_id(label: &str, value: &str) -> Result<String> {
    let valid = !value.is_empty()
        && value.len() <= 128
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'));
    if !valid {
        bail!("{label} must contain 1-128 ASCII letters, digits, '-' or '_'");
    }
    Ok(value.to_owned())
}

fn spawn_reader<R>(
    reader: R,
    stream: Stream,
    sender: mpsc::Sender<Chunk>,
) -> thread::JoinHandle<std::io::Result<()>>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut reader = BufReader::new(reader);
        let mut buffer = Vec::new();
        loop {
            buffer.clear();
            let count = reader.read_until(b'\n', &mut buffer)?;
            if count == 0 {
                break;
            }
            if sender
                .send(Chunk {
                    stream,
                    bytes: buffer.clone(),
                })
                .is_err()
            {
                break;
            }
        }
        Ok(())
    })
}

fn join_reader(handle: thread::JoinHandle<std::io::Result<()>>) -> Result<()> {
    match handle.join() {
        Ok(result) => result.context("failed to read child output"),
        Err(_) => bail!("child output reader panicked"),
    }
}

fn display(value: &OsString) -> String {
    value.to_string_lossy().into_owned()
}
