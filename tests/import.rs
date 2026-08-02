use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

mod support;

fn runsift(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_runsift"))
        .args(args)
        .output()
        .unwrap()
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

#[test]
fn imports_complete_historical_log_and_builds_ai_context() {
    let directory = support::temp_directory("historical-import");
    let input_dir = directory.join("input");
    let output_dir = directory.join("cases");
    fs::create_dir_all(&input_dir).unwrap();
    let log_path = input_dir.join("application.log");
    let original = "\
[2026-08-02T10:00:00+08:00] [info] [thread 17] parser started
[2026-08-02T10:00:01+08:00] [error] [thread 17] invalid record length 18 password=supersecret
==42==ERROR: AddressSanitizer: heap-use-after-free on address 0x1234
    #0 0x1000 in parse_record /work/parser.cpp:42
SUMMARY: AddressSanitizer: heap-use-after-free
";
    fs::write(&log_path, original).unwrap();

    let output = runsift(&[
        "import",
        "--case-id",
        "field_4821",
        "--output",
        output_dir.to_str().unwrap(),
        log_path.to_str().unwrap(),
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let message = String::from_utf8_lossy(&output.stderr);
    assert!(message.contains("imported 1 historical log file(s)"));
    assert!(message.contains("developer summary"));
    assert!(message.contains("next: runsift context"));

    let bundle = output_dir.join("field_4821");
    let manifest = read_json(&bundle.join("manifest.json"));
    assert_eq!(manifest["schema_version"], 3);
    assert_eq!(manifest["capture_mode"], "import");
    assert_eq!(manifest["case_id"], "field_4821");
    assert!(manifest.get("command").is_none());
    assert_eq!(manifest["sources"][0]["sha256"].as_str().unwrap().len(), 64);
    assert_eq!(manifest["diagnostic_count"], 1);
    assert_eq!(manifest["observed_started_at"], "2026-08-02T02:00:00Z");
    assert_eq!(manifest["observed_finished_at"], "2026-08-02T02:00:01Z");

    assert_eq!(fs::read_to_string(&log_path).unwrap(), original);
    let copied = fs::read_to_string(bundle.join("logs/000-application.log")).unwrap();
    assert!(!copied.contains("supersecret"));
    assert!(copied.contains("password=<redacted>"));

    let events = fs::read_to_string(bundle.join("events.jsonl")).unwrap();
    let parsed = events
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    let error = parsed
        .iter()
        .find(|event| event["severity"] == "error")
        .unwrap();
    assert_eq!(error["evidence"]["source_path"], log_path.to_str().unwrap());
    assert!(error["evidence"]["byte_start"].as_u64().unwrap() > 0);

    let summary = fs::read_to_string(bundle.join("summary.md")).unwrap();
    assert!(summary.contains("# RunSift 历史日志摘要"));
    assert!(summary.contains("## 关键信息概览"));
    assert!(summary.contains("invalid record length"));
    assert!(summary.contains("SHA-256"));
    assert!(summary.contains("ASan"));

    let context_output = runsift(&["context", bundle.to_str().unwrap()]);
    assert!(
        context_output.status.success(),
        "{}",
        String::from_utf8_lossy(&context_output.stderr)
    );
    let context = read_json(&bundle.join("ai/context.json"));
    assert_eq!(context["protocol_version"], 2);
    assert_eq!(context["source"]["capture_mode"], "import");
    assert_eq!(context["source"]["correlation_id"], "field_4821");
    assert!(context["source"]["run_id"].is_null());
    assert!(context["subject"]["command"].is_null());
    assert!(context["subject"]["success"].is_null());
    assert!(
        context["facts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|fact| fact["kind"] == "historical_case")
    );
    assert!(
        context["missing_information"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["missing_id"] == "missing_execution_context")
    );

    let _ = fs::remove_dir_all(directory);
}

#[test]
fn directory_import_is_recursive_only_when_requested() {
    let directory = support::temp_directory("recursive-import");
    let input = directory.join("input");
    let nested = input.join("nested");
    fs::create_dir_all(&nested).unwrap();
    fs::write(input.join("service.log"), "[error] direct failure\n").unwrap();
    fs::write(nested.join("worker.log"), "[warn] nested timeout\n").unwrap();

    let direct = runsift(&[
        "import",
        "--case-id",
        "direct_case",
        "--output",
        directory.join("cases").to_str().unwrap(),
        input.to_str().unwrap(),
    ]);
    assert!(direct.status.success());
    let direct_manifest = read_json(&directory.join("cases/direct_case/manifest.json"));
    assert_eq!(direct_manifest["sources"].as_array().unwrap().len(), 1);

    let recursive = runsift(&[
        "import",
        "--recursive",
        "--case-id",
        "recursive_case",
        "--output",
        directory.join("cases").to_str().unwrap(),
        input.to_str().unwrap(),
    ]);
    assert!(recursive.status.success());
    let recursive_manifest = read_json(&directory.join("cases/recursive_case/manifest.json"));
    assert_eq!(recursive_manifest["sources"].as_array().unwrap().len(), 2);

    let _ = fs::remove_dir_all(directory);
}

#[test]
fn failed_import_does_not_leave_a_partial_bundle() {
    let directory = support::temp_directory("failed-import");
    let output_dir = directory.join("cases");
    fs::create_dir_all(&directory).unwrap();
    let log = directory.join("application.log");
    fs::write(&log, "[error] should remain source-only\n").unwrap();
    let missing_profile = directory.join("missing-profile.json");
    let output = runsift(&[
        "import",
        "--case-id",
        "broken_case",
        "--output",
        output_dir.to_str().unwrap(),
        "--log-profile",
        missing_profile.to_str().unwrap(),
        log.to_str().unwrap(),
    ]);

    assert_eq!(output.status.code(), Some(2));
    assert!(!output_dir.join("broken_case").exists());
    assert_eq!(fs::read_dir(&output_dir).unwrap().count(), 0);
    assert_eq!(
        fs::read_to_string(log).unwrap(),
        "[error] should remain source-only\n"
    );

    let _ = fs::remove_dir_all(directory);
}

#[test]
fn no_redact_preserves_original_non_utf8_bytes_in_the_artifact() {
    let directory = support::temp_directory("binary-safe-import");
    fs::create_dir_all(&directory).unwrap();
    let log = directory.join("raw.log");
    let original = [b'[', b'e', b'r', b'r', b'o', b'r', b']', b' ', 0xff, b'\n'];
    fs::write(&log, original).unwrap();
    let output = runsift(&[
        "import",
        "--no-redact",
        "--case-id",
        "raw_case",
        "--output",
        directory.join("cases").to_str().unwrap(),
        log.to_str().unwrap(),
    ]);

    assert!(output.status.success());
    assert_eq!(
        fs::read(directory.join("cases/raw_case/logs/000-raw.log")).unwrap(),
        original
    );

    let _ = fs::remove_dir_all(directory);
}
