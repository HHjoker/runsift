use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use crate::logs::stable_id;
use crate::model::{
    CorrelationContext, Diagnostic, DiagnosticKind, Event, EvidenceRef, StackFrame,
};

static ASAN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^(?:==\d+==)?ERROR: AddressSanitizer: (?P<summary>[^\r\n]+)").unwrap()
});
static TSAN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)^(?:WARNING: ThreadSanitizer:|ThreadSanitizer: data race)(?P<summary>[^\r\n]*)",
    )
    .unwrap()
});
static UBSAN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^[^\r\n]*runtime error: (?P<summary>[^\r\n]+)").unwrap());
static FRAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)^\s*#(?P<index>\d+)\s+(?:(?P<address>0x[0-9a-fA-F]+)\s+)?(?:in\s+)?(?P<body>[^\r\n]+)$",
    )
    .unwrap()
});
static LLDB_FRAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)^\s*frame #(?P<index>\d+):\s+(?:(?P<address>0x[0-9a-fA-F]+)\s+)?(?P<body>[^\r\n]+)$",
    )
    .unwrap()
});
static LOCATION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?P<file>(?:[A-Za-z]:)?[^()\s]+):(?P<line>\d+)(?::\d+)?").unwrap()
});

#[derive(Debug)]
struct Start {
    offset: usize,
    kind: DiagnosticKind,
    summary: String,
}

pub fn parse(
    content: &str,
    source_path: &Path,
    artifact: &str,
    context: &CorrelationContext,
    events: &[Event],
) -> Vec<Diagnostic> {
    let mut starts = Vec::new();
    collect_starts(&mut starts, &ASAN, DiagnosticKind::Address, content);
    collect_starts(&mut starts, &TSAN, DiagnosticKind::Thread, content);
    collect_starts(
        &mut starts,
        &UBSAN,
        DiagnosticKind::UndefinedBehavior,
        content,
    );
    starts.sort_by_key(|start| start.offset);
    starts.dedup_by_key(|start| start.offset);

    starts
        .iter()
        .enumerate()
        .map(|(index, start)| {
            let end = starts
                .get(index + 1)
                .map_or(content.len(), |next| next.offset);
            let block = &content[start.offset..end];
            let diagnostic_id = stable_id(
                "diag",
                &format!(
                    "{}:{}:{}:{:?}:{}",
                    source_path.display(),
                    start.offset,
                    end,
                    start.kind,
                    start.summary
                ),
            );
            let event_ids = events
                .iter()
                .filter(|event| {
                    event.evidence.artifact == artifact
                        && event.evidence.byte_start < end as u64
                        && event.evidence.byte_end > start.offset as u64
                })
                .map(|event| event.event_id.clone())
                .collect();

            Diagnostic {
                diagnostic_id,
                context: context.clone(),
                kind: start.kind,
                summary: start.summary.clone(),
                stack_frames: parse_stack_frames(block),
                evidence: EvidenceRef {
                    artifact: artifact.to_owned(),
                    source_path: source_path.to_path_buf(),
                    byte_start: start.offset as u64,
                    byte_end: end as u64,
                },
                event_ids,
            }
        })
        .collect()
}

fn collect_starts(output: &mut Vec<Start>, pattern: &Regex, kind: DiagnosticKind, content: &str) {
    output.extend(pattern.captures_iter(content).filter_map(|captures| {
        let whole = captures.get(0)?;
        let summary = captures
            .name("summary")
            .map(|value| value.as_str().trim())
            .filter(|value| !value.is_empty())
            .unwrap_or(match kind {
                DiagnosticKind::Thread => "data race",
                _ => "sanitizer finding",
            });
        Some(Start {
            offset: whole.start(),
            kind,
            summary: summary.to_owned(),
        })
    }));
}

pub fn parse_stack_frames(content: &str) -> Vec<StackFrame> {
    let mut frames = FRAME
        .captures_iter(content)
        .chain(LLDB_FRAME.captures_iter(content))
        .filter_map(|captures| {
            let raw = captures.get(0)?.as_str().trim().to_owned();
            let index = captures.name("index")?.as_str().parse().ok()?;
            let address = captures
                .name("address")
                .map(|value| value.as_str().to_owned());
            let body = captures.name("body")?.as_str().trim();
            let location = LOCATION.captures(body);
            let file = location
                .as_ref()
                .and_then(|value| value.name("file"))
                .map(|value| PathBuf::from(value.as_str()));
            let line = location
                .as_ref()
                .and_then(|value| value.name("line"))
                .and_then(|value| value.as_str().parse().ok());
            let function = location
                .as_ref()
                .and_then(|value| value.get(0))
                .map_or(body, |value| body[..value.start()].trim())
                .trim_end_matches(" at")
                .trim()
                .to_owned();

            Some(StackFrame {
                index,
                address,
                function: (!function.is_empty()).then_some(function),
                file,
                line,
                raw,
            })
        })
        .collect::<Vec<_>>();
    frames.sort_by_key(|frame| frame.index);
    frames.dedup_by(|left, right| left.index == right.index && left.raw == right.raw);
    frames
}

#[cfg(test)]
mod tests {
    use super::parse;
    use crate::model::{CorrelationContext, DiagnosticKind};

    #[test]
    fn parses_asan_report_and_stack() {
        let input = "\
==42==ERROR: AddressSanitizer: heap-use-after-free on address 0x1234
    #0 0x1000 in parse_record /work/parser.cpp:42
    #1 0x2000 in main /work/main.cpp:10
SUMMARY: AddressSanitizer: heap-use-after-free
";
        let diagnostics = parse(
            input,
            "stderr.log".as_ref(),
            "stderr.log",
            &CorrelationContext {
                run_id: "run_test".to_owned(),
                ..Default::default()
            },
            &[],
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
        let diagnostics = parse(
            input,
            "stderr.log".as_ref(),
            "stderr.log",
            &CorrelationContext {
                run_id: "run_test".to_owned(),
                ..Default::default()
            },
            &[],
        );
        assert_eq!(diagnostics.len(), 2);
        assert_eq!(diagnostics[0].kind, DiagnosticKind::UndefinedBehavior);
        assert_eq!(diagnostics[1].kind, DiagnosticKind::Thread);
        assert_eq!(diagnostics[1].stack_frames[0].line, Some(18));
    }
}
