use chrono::Utc;
use runsift::model::{CorrelationContext, Event, EvidenceRef, Severity};
use runsift::pattern;

#[test]
fn normalizes_dynamic_values() {
    assert_eq!(
        pattern::normalize("2026-07-30 10:20:30.123 error id=42 addr=0x7ffe ip=10.0.0.1"),
        "<timestamp> error id=<num> addr=<hex> ip=<ip>"
    );
}

#[test]
fn stacktrace_does_not_split_the_error_pattern() {
    let event = |id: &str, message: &str| Event {
        event_id: id.to_owned(),
        context: CorrelationContext {
            run_id: "run_test".to_owned(),
            ..Default::default()
        },
        observed_at: Utc::now(),
        timestamp: None,
        severity: Severity::Error,
        source: "app.log".to_owned(),
        thread_id: None,
        logger: None,
        message: message.to_owned(),
        evidence: EvidenceRef {
            artifact: "app.log".to_owned(),
            source_path: "app.log".into(),
            byte_start: 0,
            byte_end: 1,
        },
    };
    let patterns = pattern::aggregate(&[
        event("one", "[error] invalid length 18\n  at parser.cpp:42"),
        event("two", "[error] invalid length 21"),
    ]);

    assert_eq!(patterns.len(), 1);
    assert_eq!(patterns[0].count, 2);
}
