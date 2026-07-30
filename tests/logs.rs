use std::fs::{self, OpenOptions};
use std::io::Write;

use chrono::Utc;
use runsift::logs;
use runsift::model::{CorrelationContext, Severity};
use runsift::profile::LogProfile;

mod support;

fn context() -> CorrelationContext {
    CorrelationContext {
        run_id: "run_test".to_owned(),
        ..Default::default()
    }
}

#[test]
fn parses_spdlog_and_keeps_multiline_stack() {
    let input = "\
[2026-07-30T10:00:00+08:00] [error] [thread 17] parse failed
  at parser.cpp:42
[2026-07-30T10:00:01+08:00] [info] done
";
    let events = logs::parse_events(
        "app.log".as_ref(),
        "logs/app.log",
        100,
        input,
        Utc::now(),
        None,
        &context(),
    );

    assert_eq!(events.len(), 2);
    assert_eq!(events[0].severity, Severity::Error);
    assert_eq!(events[0].thread_id.as_deref(), Some("17"));
    assert!(events[0].message.contains("parser.cpp:42"));
    assert_eq!(events[0].evidence.byte_start, 100);
    assert_eq!(events[1].severity, Severity::Info);
}

#[test]
fn keeps_plain_test_output_as_separate_events() {
    let events = logs::parse_events(
        "stdout.log".as_ref(),
        "stdout.log",
        0,
        "test started\nexpected 10\nactual 8\n",
        Utc::now(),
        None,
        &context(),
    );
    assert_eq!(events.len(), 3);
}

#[test]
fn redaction_does_not_change_original_byte_offsets() {
    let directory = support::temp_directory("offsets");
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join("application.log");
    fs::write(&path, vec![b'x'; 500]).unwrap();
    let snapshot = logs::snapshot(path.clone());
    OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"[error] password=secret-value\n[info] done\n")
        .unwrap();

    let collected =
        logs::collect(snapshot, "logs/000", Utc::now(), true, None, &context()).unwrap();
    let events = &collected.deltas[0].events;
    assert_eq!(events[0].message, "[error] password=<redacted>");
    assert_eq!(events[1].evidence.byte_start, 500 + 30);
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn applies_custom_spdlog_profile() {
    let directory = support::temp_directory("profile");
    fs::create_dir_all(&directory).unwrap();
    let profile_path = directory.join("profile.json");
    fs::write(
        &profile_path,
        r#"{
  "schema_version": 1,
  "name": "pipe-format",
  "pattern": "^(?P<timestamp>[^|]+) \\| (?P<level>[^|]+) \\| (?P<thread>[^|]+) \\| (?P<logger>[^|]+) \\| (?P<message>.*)$",
  "timestamp_format": "%Y/%m/%d %H:%M:%S%.3f",
  "timezone": "+08:00"
}"#,
    )
    .unwrap();
    let profile = LogProfile::load(&profile_path).unwrap();
    let events = logs::parse_events(
        "app.log".as_ref(),
        "logs/app.log",
        0,
        "2026/07/30 10:01:02.123 | error | 17 | parser | invalid record\n",
        Utc::now(),
        Some(&profile),
        &context(),
    );
    let _ = fs::remove_dir_all(directory);

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].severity, Severity::Error);
    assert_eq!(events[0].thread_id.as_deref(), Some("17"));
    assert_eq!(events[0].logger.as_deref(), Some("parser"));
    assert_eq!(events[0].message, "invalid record");
    assert_eq!(
        events[0].timestamp.unwrap().to_rfc3339(),
        "2026-07-30T02:01:02.123+00:00"
    );
}

#[cfg(unix)]
#[test]
fn recovers_tail_from_renamed_rotation_file() {
    let directory = support::temp_directory("rotation");
    fs::create_dir_all(&directory).unwrap();
    let current = directory.join("application.log");
    let rotated = directory.join("application.log.1");
    fs::write(&current, "existing\n").unwrap();
    let snapshot = logs::snapshot(current.clone());
    OpenOptions::new()
        .append(true)
        .open(&current)
        .unwrap()
        .write_all(b"[error] before rotation\n")
        .unwrap();
    fs::rename(&current, &rotated).unwrap();
    fs::write(&current, "[info] after rotation\n").unwrap();

    let collected =
        logs::collect(snapshot, "logs/000", Utc::now(), true, None, &context()).unwrap();
    let messages = collected
        .deltas
        .iter()
        .flat_map(|delta| delta.events.iter())
        .map(|event| event.message.as_str())
        .collect::<Vec<_>>();
    let _ = fs::remove_dir_all(directory);

    assert!(collected.summary.rotation_detected);
    assert!(collected.summary.rotation_recovered);
    assert_eq!(collected.deltas.len(), 2);
    assert_eq!(
        messages,
        vec!["[error] before rotation", "[info] after rotation"]
    );
}
