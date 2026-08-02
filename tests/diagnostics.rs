use runsift::diagnostics;
use runsift::model::{CorrelationContext, DiagnosticKind};

fn context() -> CorrelationContext {
    CorrelationContext {
        run_id: "run_test".to_owned(),
        ..Default::default()
    }
}

#[test]
fn parses_asan_report_and_stack() {
    let input = "\
==42==ERROR: AddressSanitizer: heap-use-after-free on address 0x1234
    #0 0x1000 in parse_record /work/parser.cpp:42
    #1 0x2000 in main /work/main.cpp:10
SUMMARY: AddressSanitizer: heap-use-after-free
";
    let diagnostics = diagnostics::parse(
        input,
        "stderr.log".as_ref(),
        "stderr.log",
        &context(),
        &[],
        true,
    );
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].kind, DiagnosticKind::Address);
    assert_eq!(diagnostics[0].stack_frames.len(), 2);
    assert_eq!(diagnostics[0].stack_frames[0].line, Some(42));
}

#[test]
fn parses_ubsan_and_tsan_reports() {
    let input = "\
/work/parser.cpp:21:9: runtime error: signed integer overflow
WARNING: ThreadSanitizer: data race
  Write of size 4 at 0x1234 by thread T1:
    #0 update /work/state.cpp:18
";
    let diagnostics = diagnostics::parse(
        input,
        "stderr.log".as_ref(),
        "stderr.log",
        &context(),
        &[],
        true,
    );
    assert_eq!(diagnostics.len(), 2);
    assert_eq!(diagnostics[0].kind, DiagnosticKind::UndefinedBehavior);
    assert_eq!(diagnostics[1].kind, DiagnosticKind::Thread);
    assert_eq!(diagnostics[1].stack_frames[0].line, Some(18));
}
