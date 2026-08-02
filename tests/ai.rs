use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::mpsc;
use std::thread;

use chrono::{DateTime, Utc};
use runsift::ai::{
    ANALYSIS_PROTOCOL, AnalysisFinding, AnalysisResult, AnalysisSeverity, EvidenceBundle,
    build_context, validate_analysis,
};
use runsift::model::{
    CommandResult, CorrelationContext, CrashEvidence, Event, EvidenceRef, Manifest, Pattern,
    Severity,
};
use serde_json::{Value, json};

mod support;

fn timestamp() -> DateTime<Utc> {
    "2026-08-02T10:00:00Z".parse().unwrap()
}

fn write_json(path: &Path, value: &impl serde::Serialize) {
    fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

fn create_bundle(label: &str) -> PathBuf {
    let directory = support::temp_directory(label);
    fs::create_dir_all(&directory).unwrap();
    let correlation = CorrelationContext {
        run_id: "run_demo".to_owned(),
        batch_id: Some("batch_42".to_owned()),
        test_id: Some("parser_suite".to_owned()),
    };
    let event = Event {
        event_id: "evt_bad_length".to_owned(),
        context: correlation.clone(),
        observed_at: timestamp(),
        timestamp: Some(timestamp()),
        severity: Severity::Error,
        source: "application.log".to_owned(),
        thread_id: Some("17".to_owned()),
        logger: Some("parser".to_owned()),
        message: "invalid record length 18 at offset 8192".to_owned(),
        evidence: EvidenceRef {
            artifact: "logs/000-application.log".to_owned(),
            source_path: "/var/log/application.log".into(),
            byte_start: 420,
            byte_end: 527,
        },
    };
    let pattern = Pattern {
        pattern_id: "pat_bad_length".to_owned(),
        severity: Severity::Error,
        template: "invalid record length <num> at offset <num>".to_owned(),
        count: 3,
        first_observed_at: timestamp(),
        last_observed_at: timestamp(),
        representative_event_ids: vec![event.event_id.clone()],
    };
    let manifest = Manifest {
        schema_version: 2,
        run_id: correlation.run_id.clone(),
        context: correlation,
        started_at: timestamp(),
        finished_at: timestamp(),
        working_directory: "/work/project".into(),
        command: CommandResult {
            program: "./build/parser_tests".to_owned(),
            args: vec![
                "--gtest_filter=Parser.*".to_owned(),
                "--api_key=supersecret".to_owned(),
            ],
            exit_code: Some(1),
            success: false,
        },
        redacted: true,
        git: None,
        sources: Vec::new(),
        event_count: 1,
        pattern_count: 1,
        test_count: 0,
        failed_test_count: 0,
        diagnostic_count: 0,
        core_dump_count: 0,
        debugger_report_count: 0,
        log_profile: None,
        artifacts: vec!["events.jsonl".to_owned(), "patterns.json".to_owned()],
    };
    write_json(&directory.join("manifest.json"), &manifest);
    fs::write(
        directory.join("events.jsonl"),
        format!("{}\n", serde_json::to_string(&event).unwrap()),
    )
    .unwrap();
    write_json(&directory.join("patterns.json"), &vec![pattern]);
    write_json(&directory.join("tests.json"), &Vec::<Value>::new());
    write_json(&directory.join("diagnostics.json"), &Vec::<Value>::new());
    write_json(
        &directory.join("crash.json"),
        &CrashEvidence {
            core_dumps: Vec::new(),
            debugger_reports: Vec::new(),
        },
    );
    directory
}

fn runsift(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_runsift"))
        .args(args)
        .output()
        .unwrap()
}

fn valid_analysis() -> Value {
    json!({
        "schema_version": 1,
        "summary": "The parser rejected a record length during the failed run.",
        "findings": [{
            "title": "Record length validation failed",
            "explanation": "The captured error identifies the rejected length and offset.",
            "severity": "error",
            "evidence_ids": ["evt_bad_length"]
        }],
        "hypotheses": [],
        "missing_information": ["The originating input bytes are not present."]
    })
}

#[test]
fn builds_budgeted_context_with_facts_and_explicit_gaps() {
    let directory = create_bundle("ai-context");
    let bundle = EvidenceBundle::load(&directory).unwrap();
    let context = build_context(&bundle, 8_000).unwrap();

    assert!(context.budget.estimated_tokens <= context.budget.max_tokens);
    assert!(context.facts.iter().any(|fact| fact.kind == "run"));
    assert!(context.hypotheses.is_empty());
    assert!(
        context
            .missing_information
            .iter()
            .any(|item| item.missing_id == "missing_tests")
    );
    assert!(
        context
            .response_contract
            .allowed_evidence_ids
            .contains(&"evt_bad_length".to_owned())
    );
    assert_eq!(context.evidence[0].evidence_id, "run_demo");
    assert!(
        !serde_json::to_string(&context)
            .unwrap()
            .contains("supersecret")
    );

    let _ = fs::remove_dir_all(directory);
}

#[test]
fn omits_lower_priority_evidence_when_the_budget_is_full() {
    let directory = create_bundle("ai-budget");
    let mut bundle = EvidenceBundle::load(&directory).unwrap();
    for index in 0..80 {
        bundle.events.push(Event {
            event_id: format!("evt_warning_{index:03}"),
            context: CorrelationContext {
                run_id: "run_demo".to_owned(),
                ..Default::default()
            },
            observed_at: timestamp(),
            timestamp: Some(timestamp()),
            severity: Severity::Warn,
            source: "application.log".to_owned(),
            thread_id: None,
            logger: None,
            message: format!(
                "warning {index}: {}",
                "lower-priority diagnostic detail ".repeat(12)
            ),
            evidence: EvidenceRef {
                artifact: "logs/000-application.log".to_owned(),
                source_path: "/var/log/application.log".into(),
                byte_start: index * 100,
                byte_end: index * 100 + 99,
            },
        });
    }

    let context = build_context(&bundle, 4_000).unwrap();
    assert!(context.budget.estimated_tokens <= 4_000);
    assert!(context.budget.omitted_count > 0);
    assert_eq!(context.evidence[0].evidence_id, "run_demo");
    assert!(
        context
            .missing_information
            .iter()
            .any(|item| item.missing_id == "missing_budget")
    );

    let _ = fs::remove_dir_all(directory);
}

#[test]
fn rejects_analysis_that_cites_evidence_outside_the_context() {
    let directory = create_bundle("ai-citations");
    let context = build_context(&EvidenceBundle::load(&directory).unwrap(), 8_000).unwrap();
    let analysis = AnalysisResult {
        schema_version: 1,
        summary: "A failure occurred.".to_owned(),
        findings: vec![AnalysisFinding {
            title: "Unsupported conclusion".to_owned(),
            explanation: "The citation was not supplied by RunSift.".to_owned(),
            severity: AnalysisSeverity::Error,
            evidence_ids: vec!["evt_invented".to_owned()],
        }],
        hypotheses: Vec::new(),
        missing_information: Vec::new(),
    };

    let error = validate_analysis(&analysis, &context).unwrap_err();
    assert!(error.to_string().contains("unavailable evidence ID"));

    let _ = fs::remove_dir_all(directory);
}

#[test]
fn context_cli_writes_context_and_prompt_without_a_model() {
    let directory = create_bundle("ai-command");
    let output = runsift(&["context", directory.to_str().unwrap()]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let context: Value =
        serde_json::from_slice(&fs::read(directory.join("ai/context.json")).unwrap()).unwrap();
    let prompt = fs::read_to_string(directory.join("ai/prompt.md")).unwrap();
    assert_eq!(context["protocol"], "runsift.diagnostic-context");
    assert!(prompt.contains("Preserve business intent"));

    let _ = fs::remove_dir_all(directory);
}

#[test]
fn local_adapter_writes_only_a_citation_validated_analysis() {
    let directory = create_bundle("ai-local");
    let analysis = valid_analysis().to_string();
    let output = runsift(&[
        "analyze",
        directory.to_str().unwrap(),
        "local",
        "--",
        "sh",
        "-c",
        &format!("printf '%s' '{}'", analysis),
    ]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: Value =
        serde_json::from_slice(&fs::read(directory.join("ai/analysis.json")).unwrap()).unwrap();
    assert_eq!(envelope["protocol"], ANALYSIS_PROTOCOL);
    assert_eq!(envelope["adapter"], "local:sh");
    assert_eq!(
        envelope["analysis"]["findings"][0]["evidence_ids"][0],
        "evt_bad_length"
    );

    let _ = fs::remove_dir_all(directory);
}

#[test]
fn existing_analysis_is_rejected_before_an_adapter_is_started() {
    let directory = create_bundle("ai-preflight");
    fs::create_dir_all(directory.join("ai")).unwrap();
    fs::write(directory.join("ai/analysis.json"), "existing").unwrap();
    let marker = directory.join("adapter-started");
    let output = runsift(&[
        "analyze",
        directory.to_str().unwrap(),
        "local",
        "--",
        "/usr/bin/touch",
        marker.to_str().unwrap(),
    ]);

    assert_eq!(output.status.code(), Some(2));
    assert!(!marker.exists());
    assert!(String::from_utf8_lossy(&output.stderr).contains("already exists"));

    let _ = fs::remove_dir_all(directory);
}

#[test]
fn openai_adapter_uses_responses_endpoint_and_validates_output() {
    let directory = create_bundle("ai-openai");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (sender, receiver) = mpsc::channel();
    let model_output = valid_analysis().to_string();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let count = stream.read(&mut buffer).unwrap();
            if count == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..count]);
            if let Some(header_end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&request[..header_end]);
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(str::trim)
                            .and_then(|value| value.parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                if request.len() >= header_end + 4 + length {
                    break;
                }
            }
        }
        sender.send(String::from_utf8(request).unwrap()).unwrap();
        let body = json!({
            "output": [{
                "type": "message",
                "content": [{ "type": "output_text", "text": model_output }]
            }]
        })
        .to_string();
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
    });

    let base_url = format!("http://{address}");
    let output = runsift(&[
        "analyze",
        directory.to_str().unwrap(),
        "openai",
        "--base-url",
        &base_url,
        "--model",
        "test-model",
        "--timeout",
        "5",
    ]);
    server.join().unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let request = receiver.recv().unwrap();
    assert!(request.starts_with("POST /responses HTTP/1.1"));
    let request_body = request.split("\r\n\r\n").nth(1).unwrap();
    let body: Value = serde_json::from_str(request_body).unwrap();
    assert_eq!(body["model"], "test-model");
    assert_eq!(body["store"], false);
    assert_eq!(body["text"]["format"]["type"], "json_schema");

    let _ = fs::remove_dir_all(directory);
}
