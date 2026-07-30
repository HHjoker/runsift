use std::ffi::OsString;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;

use anyhow::{Context, Result, bail};
use chrono::Utc;

use crate::cli::RunArgs;
use crate::git;
use crate::logs::{self, Snapshot};
use crate::model::{CommandResult, Manifest};
use crate::pattern;
use crate::redact;
use crate::report;

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
    let run_id = format!(
        "run_{}_{}",
        started_at.format("%Y%m%dT%H%M%S%.3fZ"),
        std::process::id()
    );
    let bundle_dir = args.output.join(&run_id);
    fs::create_dir_all(bundle_dir.join("logs"))
        .with_context(|| format!("failed to create {}", bundle_dir.display()))?;

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
    ));
    events.extend(logs::parse_events(
        &bundle_dir.join("stderr.log"),
        "stderr.log",
        0,
        &captured_stderr,
        finished_at,
    ));

    let mut sources = Vec::new();
    let mut artifacts = vec![
        "manifest.json".to_owned(),
        "summary.md".to_owned(),
        "events.jsonl".to_owned(),
        "patterns.json".to_owned(),
        "stdout.log".to_owned(),
        "stderr.log".to_owned(),
    ];

    for (index, snapshot) in snapshots.into_iter().enumerate() {
        match collect_log(snapshot, index, finished_at, redact_enabled) {
            Ok(delta) => {
                let path = bundle_dir.join(&delta.artifact);
                fs::write(&path, &delta.content)
                    .with_context(|| format!("failed to write {}", path.display()))?;
                artifacts.push(delta.artifact);
                events.extend(delta.events);
                sources.push(delta.summary);
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

    let command_result = CommandResult {
        program: display(program),
        args: command_args.iter().map(display).collect(),
        exit_code: status.code(),
        success: status.success(),
    };
    let manifest = Manifest {
        schema_version: 1,
        run_id,
        started_at,
        finished_at,
        working_directory: cwd,
        command: command_result,
        redacted: redact_enabled,
        git: git_info,
        sources,
        event_count: events.len(),
        pattern_count: patterns.len(),
        artifacts,
    };
    report::write_bundle(&bundle_dir, &manifest, &events, &patterns)?;

    eprintln!("runsift: evidence bundle: {}", bundle_dir.display());
    Ok(status.code().unwrap_or(1))
}

fn collect_log(
    snapshot: Snapshot,
    index: usize,
    observed_at: chrono::DateTime<Utc>,
    redact_enabled: bool,
) -> Result<logs::Delta> {
    let artifact = format!(
        "logs/{index:03}-{}",
        artifact_name(snapshot_path(&snapshot))
    );
    logs::collect_delta(snapshot, artifact, observed_at, redact_enabled)
}

fn snapshot_path(snapshot: &Snapshot) -> &Path {
    // Kept private to this module through the stable debug-independent accessor below.
    snapshot.path()
}

fn artifact_name(path: &Path) -> String {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("application.log");
    name.chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect()
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
