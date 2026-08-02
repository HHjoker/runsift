use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::Utc;

use crate::cli::ImportArgs;
use crate::crash;
use crate::diagnostics;
use crate::logs;
use crate::model::{CaptureMode, CorrelationContext, CrashEvidence, Manifest};
use crate::pattern;
use crate::profile::LogProfile;
use crate::report;
use crate::test_report;

pub fn run(args: ImportArgs) -> Result<i32> {
    let started_at = Utc::now();
    let case_id = match args.case_id.as_deref() {
        Some(value) => validate_id("case ID", value)?,
        None => format!(
            "case_{}_{}",
            started_at.format("%Y%m%dT%H%M%S%.3fZ"),
            std::process::id()
        ),
    };
    let files = discover(&args.inputs, args.recursive)?;
    if files.is_empty() {
        bail!("no regular files were found in the supplied historical log inputs");
    }

    let bundle_dir = args.output.join(&case_id);
    if bundle_dir.exists() {
        bail!(
            "historical evidence bundle {} already exists; choose a different case ID",
            bundle_dir.display()
        );
    }
    fs::create_dir_all(&args.output)
        .with_context(|| format!("failed to create {}", args.output.display()))?;
    let staging = args
        .output
        .join(format!(".{case_id}.tmp-{}", std::process::id()));
    if staging.exists() {
        bail!(
            "temporary import directory {} already exists",
            staging.display()
        );
    }
    fs::create_dir_all(staging.join("logs"))?;
    fs::create_dir_all(staging.join("tests"))?;
    fs::create_dir_all(staging.join("debugger"))?;

    let result = build_bundle(&args, &case_id, &files, &staging, started_at);
    if let Err(error) = result {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }
    if bundle_dir.exists() {
        let _ = fs::remove_dir_all(&staging);
        bail!(
            "historical evidence bundle {} appeared during import; choose a different case ID",
            bundle_dir.display()
        );
    }
    if let Err(error) = fs::rename(&staging, &bundle_dir) {
        let _ = fs::remove_dir_all(&staging);
        return Err(error).with_context(|| {
            format!(
                "failed to finalize historical evidence bundle {}",
                bundle_dir.display()
            )
        });
    }

    eprintln!("runsift: imported {} historical log file(s)", files.len());
    eprintln!(
        "runsift: historical evidence bundle: {}",
        bundle_dir.display()
    );
    eprintln!(
        "runsift: developer summary: {}",
        bundle_dir.join("summary.md").display()
    );
    eprintln!("runsift: next: runsift context {}", bundle_dir.display());
    Ok(0)
}

fn build_bundle(
    args: &ImportArgs,
    case_id: &str,
    files: &[PathBuf],
    staging: &Path,
    started_at: chrono::DateTime<Utc>,
) -> Result<()> {
    let context = CorrelationContext {
        run_id: case_id.to_owned(),
        case_id: Some(case_id.to_owned()),
        batch_id: None,
        test_id: None,
    };
    let redact_enabled = !args.no_redact;
    let profile = args
        .log_profile
        .as_deref()
        .map(LogProfile::load)
        .transpose()?;
    let mut events = Vec::new();
    let mut sources = Vec::new();
    let mut all_diagnostics = Vec::new();
    let mut artifacts = vec![
        "manifest.json".to_owned(),
        "summary.md".to_owned(),
        "events.jsonl".to_owned(),
        "patterns.json".to_owned(),
        "tests.json".to_owned(),
        "diagnostics.json".to_owned(),
        "crash.json".to_owned(),
    ];

    for (index, path) in files.iter().enumerate() {
        let artifact = log_artifact(index, path);
        let imported = logs::import_file(
            path,
            artifact.clone(),
            started_at,
            redact_enabled,
            profile.as_ref(),
            &context,
        )?;
        all_diagnostics.extend(diagnostics::parse(
            &imported.raw_content,
            path,
            &artifact,
            &context,
            &imported.events,
            redact_enabled,
        ));
        fs::write(staging.join(&artifact), imported.content)?;
        artifacts.push(artifact);
        events.extend(imported.events);
        sources.push(imported.summary);
    }

    events.sort_by(|left, right| {
        left.timestamp
            .unwrap_or(left.observed_at)
            .cmp(&right.timestamp.unwrap_or(right.observed_at))
            .then_with(|| left.source.cmp(&right.source))
            .then_with(|| left.evidence.byte_start.cmp(&right.evidence.byte_start))
            .then_with(|| left.event_id.cmp(&right.event_id))
    });
    let patterns = pattern::aggregate(&events);

    let mut test_reports = Vec::new();
    for (index, path) in args.test_reports.iter().enumerate() {
        let artifact = test_report::artifact(index, path);
        let imported = test_report::import(path, artifact.clone(), redact_enabled)?;
        fs::write(staging.join(&artifact), imported.content)?;
        artifacts.push(artifact);
        test_reports.push(imported.report);
    }

    let mut core_dumps = Vec::new();
    for path in &args.core_dumps {
        core_dumps.push(crash::inspect_core(path)?);
    }
    let mut debugger_reports = Vec::new();
    for (index, path) in args.debugger_reports.iter().enumerate() {
        let artifact = crash::debugger_artifact(index, path);
        let imported =
            crash::import_debugger_report(path, artifact.clone(), &context, redact_enabled)?;
        fs::write(staging.join(&artifact), imported.content)?;
        artifacts.push(artifact);
        debugger_reports.push(imported.report);
    }
    let crash_evidence = CrashEvidence {
        core_dumps,
        debugger_reports,
    };
    let observed_started_at = events.iter().filter_map(|event| event.timestamp).min();
    let observed_finished_at = events.iter().filter_map(|event| event.timestamp).max();
    let test_count = test_reports.iter().map(|report| report.total).sum();
    let failed_test_count = test_reports
        .iter()
        .map(|report| report.failed + report.errors)
        .sum();
    let finished_at = Utc::now();
    let manifest = Manifest {
        schema_version: 3,
        capture_mode: CaptureMode::Import,
        run_id: case_id.to_owned(),
        case_id: Some(case_id.to_owned()),
        context,
        started_at,
        finished_at,
        working_directory: std::env::current_dir()?,
        command: None,
        observed_started_at,
        observed_finished_at,
        redacted: redact_enabled,
        git: None,
        sources,
        event_count: events.len(),
        pattern_count: patterns.len(),
        test_count,
        failed_test_count,
        diagnostic_count: all_diagnostics.len(),
        core_dump_count: crash_evidence.core_dumps.len(),
        debugger_report_count: crash_evidence.debugger_reports.len(),
        log_profile: profile.map(|value| value.name().to_owned()),
        artifacts,
    };
    report::write_bundle(
        staging,
        &manifest,
        &events,
        &patterns,
        &test_reports,
        &all_diagnostics,
        &crash_evidence,
    )
}

fn discover(inputs: &[PathBuf], recursive: bool) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for input in inputs {
        collect_path(input, recursive, &mut files)?;
    }
    files.sort();
    let mut seen = HashSet::new();
    files.retain(|path| {
        path.canonicalize()
            .ok()
            .is_some_and(|canonical| seen.insert(canonical))
    });
    Ok(files)
}

fn collect_path(path: &Path, recursive: bool, output: &mut Vec<PathBuf>) -> Result<()> {
    if path.is_file() {
        output.push(path.to_path_buf());
        return Ok(());
    }
    if !path.is_dir() {
        bail!(
            "historical log input {} does not exist or is not a regular file",
            path.display()
        );
    }
    let mut entries = fs::read_dir(path)
        .with_context(|| format!("failed to read directory {}", path.display()))?
        .collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let file_type = entry.file_type()?;
        if file_type.is_file() {
            output.push(entry.path());
        } else if recursive && file_type.is_dir() {
            collect_path(&entry.path(), true, output)?;
        }
    }
    Ok(())
}

fn log_artifact(index: usize, path: &Path) -> String {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("historical.log")
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    format!("logs/{index:03}-{name}")
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
