use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use regex::Regex;
use sha2::{Digest, Sha256};

use crate::model::{Event, EvidenceRef, Severity, SourceSummary};
use crate::redact;

static TIMESTAMP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^\[?(?P<value>\d{4}-\d{2}-\d{2}[ T]\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:?\d{2})?)",
    )
    .unwrap()
});
static SEVERITY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(?:^|[\s\[])(trace|debug|info|warn|warning|err|error|critical|fatal)(?:[\]\s:]|$)",
    )
    .unwrap()
});
static THREAD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\[(?:thread|tid)?[ =:]*(?P<value>[0-9a-fx]+)\]").unwrap());

#[derive(Debug)]
pub struct Snapshot {
    path: PathBuf,
    initial_size: u64,
}

impl Snapshot {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[derive(Debug)]
pub struct Delta {
    pub artifact: String,
    pub content: String,
    pub events: Vec<Event>,
    pub summary: SourceSummary,
}

pub fn snapshot(path: PathBuf) -> Snapshot {
    let initial_size = path.metadata().map(|metadata| metadata.len()).unwrap_or(0);
    Snapshot { path, initial_size }
}

pub fn collect_delta(
    snapshot: Snapshot,
    artifact: String,
    observed_at: DateTime<Utc>,
    redact_enabled: bool,
) -> Result<Delta> {
    let mut file = File::open(&snapshot.path)
        .with_context(|| format!("failed to open log {}", snapshot.path.display()))?;
    let final_size = file.metadata()?.len();
    let reset_detected = final_size < snapshot.initial_size;
    let start = if reset_detected {
        0
    } else {
        snapshot.initial_size
    };

    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::with_capacity((final_size.saturating_sub(start)) as usize);
    file.read_to_end(&mut bytes)?;

    let raw = String::from_utf8_lossy(&bytes);
    let content = redact::text(&raw, redact_enabled);
    let events = parse_event_bytes(
        &snapshot.path,
        &artifact,
        start,
        &bytes,
        observed_at,
        redact_enabled,
    );

    Ok(Delta {
        artifact,
        content,
        events,
        summary: SourceSummary {
            path: snapshot.path,
            initial_size: snapshot.initial_size,
            final_size,
            collected_bytes: bytes.len() as u64,
            reset_detected,
        },
    })
}

pub fn parse_events(
    source_path: &Path,
    artifact: &str,
    base_offset: u64,
    content: &str,
    observed_at: DateTime<Utc>,
) -> Vec<Event> {
    parse_event_bytes(
        source_path,
        artifact,
        base_offset,
        content.as_bytes(),
        observed_at,
        false,
    )
}

fn parse_event_bytes(
    source_path: &Path,
    artifact: &str,
    base_offset: u64,
    content: &[u8],
    observed_at: DateTime<Utc>,
    redact_enabled: bool,
) -> Vec<Event> {
    let mut records = Vec::<(usize, usize, String)>::new();
    let mut current: Option<(usize, usize, String)> = None;
    let mut offset = 0;

    for line in content.split_inclusive(|byte| *byte == b'\n') {
        let line_start = offset;
        offset += line.len();
        let line_end = offset;
        let line = String::from_utf8_lossy(line);
        let trimmed = line.trim_end_matches(['\r', '\n']);
        let starts_record = TIMESTAMP.is_match(trimmed)
            || SEVERITY.is_match(trimmed)
            || current.is_none()
            || !is_continuation(trimmed);

        if starts_record {
            if let Some(record) = current.take() {
                records.push(record);
            }
            current = Some((line_start, line_end, trimmed.to_owned()));
        } else if let Some((_, end, message)) = current.as_mut() {
            *end = line_end;
            message.push('\n');
            message.push_str(trimmed);
        }
    }

    if let Some(record) = current {
        records.push(record);
    }

    records
        .into_iter()
        .filter(|(_, _, message)| !message.trim().is_empty())
        .map(|(start, end, raw_message)| {
            let message = redact::text(&raw_message, redact_enabled);
            let timestamp = extract_timestamp(&message);
            let severity = extract_severity(&message);
            let thread_id = THREAD
                .captures(&message)
                .and_then(|captures| captures.name("value"))
                .map(|value| value.as_str().to_owned());
            let absolute_start = base_offset + start as u64;
            let absolute_end = base_offset + end as u64;
            let event_id = stable_id(
                "evt",
                &format!(
                    "{}:{absolute_start}:{absolute_end}:{message}",
                    source_path.display()
                ),
            );

            Event {
                event_id,
                observed_at,
                timestamp,
                severity,
                source: source_path.display().to_string(),
                thread_id,
                message,
                evidence: EvidenceRef {
                    artifact: artifact.to_owned(),
                    source_path: source_path.to_owned(),
                    byte_start: absolute_start,
                    byte_end: absolute_end,
                },
            }
        })
        .collect()
}

fn is_continuation(line: &str) -> bool {
    let trimmed = line.trim_start();
    line.starts_with([' ', '\t'])
        || trimmed.starts_with("at ")
        || trimmed.starts_with('#')
        || trimmed.starts_with("Caused by:")
        || trimmed.starts_with("Stack trace:")
        || trimmed.starts_with("...")
}

fn extract_timestamp(message: &str) -> Option<DateTime<Utc>> {
    let value = TIMESTAMP
        .captures(message)?
        .name("value")?
        .as_str()
        .replace(' ', "T");

    DateTime::parse_from_rfc3339(&value)
        .ok()
        .map(|timestamp| timestamp.with_timezone(&Utc))
}

fn extract_severity(message: &str) -> Severity {
    let Some(value) = SEVERITY
        .captures(message)
        .and_then(|captures| captures.get(1))
        .map(|value| value.as_str().to_ascii_lowercase())
    else {
        return Severity::Unknown;
    };

    match value.as_str() {
        "trace" => Severity::Trace,
        "debug" => Severity::Debug,
        "info" => Severity::Info,
        "warn" | "warning" => Severity::Warn,
        "err" | "error" => Severity::Error,
        "critical" | "fatal" => Severity::Critical,
        _ => Severity::Unknown,
    }
}

pub fn stable_id(prefix: &str, value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    let mut short = String::with_capacity(16);
    for byte in &digest[..8] {
        use std::fmt::Write;
        let _ = write!(short, "{byte:02x}");
    }
    format!("{prefix}_{short}")
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::{parse_event_bytes, parse_events};
    use crate::model::Severity;

    #[test]
    fn parses_spdlog_and_keeps_multiline_stack() {
        let input = "\
[2026-07-30T10:00:00+08:00] [error] [thread 17] parse failed
  at parser.cpp:42
[2026-07-30T10:00:01+08:00] [info] done
";
        let events = parse_events("app.log".as_ref(), "logs/app.log", 100, input, Utc::now());

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].severity, Severity::Error);
        assert_eq!(events[0].thread_id.as_deref(), Some("17"));
        assert!(events[0].message.contains("parser.cpp:42"));
        assert_eq!(events[0].evidence.byte_start, 100);
        assert_eq!(events[1].severity, Severity::Info);
    }

    #[test]
    fn keeps_plain_test_output_as_separate_events() {
        let events = parse_events(
            "stdout.log".as_ref(),
            "stdout.log",
            0,
            "test started\nexpected 10\nactual 8\n",
            Utc::now(),
        );
        assert_eq!(events.len(), 3);
    }

    #[test]
    fn redaction_does_not_change_original_byte_offsets() {
        let input = b"[error] password=secret-value\n[info] done\n";
        let events = parse_event_bytes(
            "app.log".as_ref(),
            "logs/app.log",
            500,
            input,
            Utc::now(),
            true,
        );

        assert_eq!(events[0].message, "[error] password=<redacted>");
        assert_eq!(events[1].evidence.byte_start, 500 + 30);
    }
}
