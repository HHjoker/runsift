use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use crate::logs::stable_id;
use crate::model::{
    CorrelationContext, Diagnostic, DiagnosticKind, Event, EvidenceRef, StackFrame,
};
use crate::redact;

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
    redact_enabled: bool,
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
                summary: redact::text(&start.summary, redact_enabled),
                stack_frames: parse_stack_frames(block)
                    .into_iter()
                    .map(|mut frame| {
                        frame.raw = redact::text(&frame.raw, redact_enabled);
                        frame.function = frame
                            .function
                            .map(|value| redact::text(&value, redact_enabled));
                        frame.file = frame.file.map(|value| {
                            redact::text(&value.to_string_lossy(), redact_enabled).into()
                        });
                        frame
                    })
                    .collect(),
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
