use std::fs::{self, File, Metadata};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use regex::Regex;
use sha2::{Digest, Sha256};

use crate::model::{
    CorrelationContext, Event, EvidenceRef, Severity, SourceSegment, SourceSummary,
};
use crate::profile::LogProfile;
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
static KEY_SIGNAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(error|failed?|failure|fatal|panic|exception|timeout|corrupt(?:ed|ion)?|invalid|mismatch|dropped|missing|lost|abort(?:ed)?|segmentation fault|sigsegv)\b",
    )
    .unwrap()
});

#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileIdentity {
    device: u64,
    inode: u64,
}

#[cfg(not(unix))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileIdentity;

#[derive(Debug)]
pub struct Snapshot {
    path: PathBuf,
    initial_size: u64,
    identity: Option<FileIdentity>,
}

#[derive(Debug)]
pub struct Delta {
    pub artifact: String,
    pub content: String,
    pub events: Vec<Event>,
    original_bytes: u64,
}

#[derive(Debug)]
pub struct CollectedSource {
    pub deltas: Vec<Delta>,
    pub summary: SourceSummary,
}

#[derive(Debug)]
pub struct ImportedLog {
    pub content: Vec<u8>,
    pub raw_content: String,
    pub events: Vec<Event>,
    pub summary: SourceSummary,
}

pub fn snapshot(path: PathBuf) -> Snapshot {
    let metadata = path.metadata().ok();
    Snapshot {
        initial_size: metadata.as_ref().map_or(0, |value| value.len()),
        identity: metadata.as_ref().and_then(file_identity),
        path,
    }
}

pub fn collect(
    snapshot: Snapshot,
    artifact_prefix: &str,
    observed_at: DateTime<Utc>,
    redact_enabled: bool,
    profile: Option<&LogProfile>,
    context: &CorrelationContext,
) -> Result<CollectedSource> {
    let current_metadata = snapshot.path.metadata().ok();
    let current_identity = current_metadata.as_ref().and_then(file_identity);
    let final_size = current_metadata.as_ref().map_or(0, |value| value.len());
    let reset_detected = snapshot.identity == current_identity
        && current_metadata
            .as_ref()
            .is_some_and(|metadata| metadata.len() < snapshot.initial_size);
    let rotation_detected = snapshot.identity.is_some() && snapshot.identity != current_identity;
    let mut requests = Vec::<(PathBuf, u64)>::new();
    let mut rotation_recovered = false;

    if rotation_detected {
        if let Some(rotated) = find_identity(&snapshot.path, snapshot.identity) {
            let start = rotated
                .metadata()
                .map(|metadata| snapshot.initial_size.min(metadata.len()))
                .unwrap_or(0);
            requests.push((rotated, start));
            rotation_recovered = true;
        }
        if current_metadata.is_some() {
            requests.push((snapshot.path.clone(), 0));
        }
    } else if current_metadata.is_some() {
        requests.push((
            snapshot.path.clone(),
            if reset_detected {
                0
            } else {
                snapshot.initial_size
            },
        ));
    }

    if requests.is_empty() {
        bail!(
            "log {} is unavailable after the command",
            snapshot.path.display()
        );
    }

    let multiple = requests.len() > 1;
    let mut deltas = Vec::with_capacity(requests.len());
    let mut segments = Vec::with_capacity(requests.len());
    let mut collected_bytes = 0_u64;
    for (index, (path, start)) in requests.into_iter().enumerate() {
        let artifact = segment_artifact(artifact_prefix, index, &path, multiple);
        let delta = read_delta(
            &path,
            artifact.clone(),
            start,
            observed_at,
            redact_enabled,
            profile,
            context,
        )?;
        let byte_end = start + delta.original_bytes;
        collected_bytes += delta.original_bytes;
        segments.push(SourceSegment {
            path,
            artifact,
            byte_start: start,
            byte_end,
        });
        deltas.push(delta);
    }

    Ok(CollectedSource {
        deltas,
        summary: SourceSummary {
            path: snapshot.path,
            initial_size: snapshot.initial_size,
            final_size,
            collected_bytes,
            reset_detected,
            rotation_detected,
            rotation_recovered,
            sha256: None,
            modified_at: current_metadata
                .and_then(|metadata| metadata.modified().ok())
                .map(DateTime::<Utc>::from),
            segments,
        },
    })
}

pub fn import_file(
    path: &Path,
    artifact: String,
    imported_at: DateTime<Utc>,
    redact_enabled: bool,
    profile: Option<&LogProfile>,
    context: &CorrelationContext,
) -> Result<ImportedLog> {
    let before = path
        .metadata()
        .with_context(|| format!("failed to inspect historical log {}", path.display()))?;
    let bytes = fs::read(path)
        .with_context(|| format!("failed to read historical log {}", path.display()))?;
    let after = path
        .metadata()
        .with_context(|| format!("failed to inspect historical log {}", path.display()))?;
    if before.len() != after.len() || before.modified().ok() != after.modified().ok() {
        bail!(
            "historical log {} changed during import; retry after the file is stable",
            path.display()
        );
    }
    let raw_content = String::from_utf8_lossy(&bytes).into_owned();
    let options = ParseOptions {
        observed_at: imported_at,
        redact_enabled,
        profile,
        context,
    };
    let events = parse_event_bytes(path, &artifact, 0, &bytes, &options);
    let sha256 = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let size = bytes.len() as u64;
    let content = if redact_enabled {
        redact::text(&raw_content, true).into_bytes()
    } else {
        bytes
    };

    Ok(ImportedLog {
        content,
        raw_content,
        events,
        summary: SourceSummary {
            path: path.to_path_buf(),
            initial_size: size,
            final_size: size,
            collected_bytes: size,
            reset_detected: false,
            rotation_detected: false,
            rotation_recovered: false,
            sha256: Some(sha256),
            modified_at: after.modified().ok().map(DateTime::<Utc>::from),
            segments: vec![SourceSegment {
                path: path.to_path_buf(),
                artifact,
                byte_start: 0,
                byte_end: size,
            }],
        },
    })
}

pub fn parse_events(
    source_path: &Path,
    artifact: &str,
    base_offset: u64,
    content: &str,
    observed_at: DateTime<Utc>,
    profile: Option<&LogProfile>,
    context: &CorrelationContext,
) -> Vec<Event> {
    let options = ParseOptions {
        observed_at,
        redact_enabled: false,
        profile,
        context,
    };
    parse_event_bytes(
        source_path,
        artifact,
        base_offset,
        content.as_bytes(),
        &options,
    )
}

pub fn is_key_message(message: &str) -> bool {
    KEY_SIGNAL.is_match(message)
}

struct ParseOptions<'a> {
    observed_at: DateTime<Utc>,
    redact_enabled: bool,
    profile: Option<&'a LogProfile>,
    context: &'a CorrelationContext,
}

fn parse_event_bytes(
    source_path: &Path,
    artifact: &str,
    base_offset: u64,
    content: &[u8],
    options: &ParseOptions<'_>,
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
        let starts_record = options
            .profile
            .and_then(|value| value.captures(trimmed))
            .is_some()
            || TIMESTAMP.is_match(trimmed)
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
            let fields = options
                .profile
                .and_then(|value| profile_fields(value, &raw_message))
                .unwrap_or_else(|| default_fields(&raw_message));
            let message = redact::text(&fields.message, options.redact_enabled);
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
                context: options.context.clone(),
                observed_at: options.observed_at,
                timestamp: fields.timestamp,
                severity: fields.severity,
                source: source_path.display().to_string(),
                thread_id: fields.thread_id,
                logger: fields.logger,
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

struct EventFields {
    timestamp: Option<DateTime<Utc>>,
    severity: Severity,
    thread_id: Option<String>,
    logger: Option<String>,
    message: String,
}

fn profile_fields(profile: &LogProfile, raw_message: &str) -> Option<EventFields> {
    let first_line = raw_message.lines().next().unwrap_or(raw_message);
    let captures = profile.captures(first_line)?;
    let mut message = captures.name("message")?.as_str().to_owned();
    for continuation in raw_message.lines().skip(1) {
        message.push('\n');
        message.push_str(continuation);
    }
    Some(EventFields {
        timestamp: captures
            .name("timestamp")
            .and_then(|value| profile.timestamp(value.as_str())),
        severity: captures
            .name("level")
            .map(|value| parse_severity(value.as_str()))
            .unwrap_or(Severity::Unknown),
        thread_id: captures
            .name("thread")
            .map(|value| value.as_str().to_owned()),
        logger: captures
            .name("logger")
            .map(|value| value.as_str().to_owned()),
        message,
    })
}

fn default_fields(raw_message: &str) -> EventFields {
    EventFields {
        timestamp: extract_timestamp(raw_message),
        severity: extract_severity(raw_message),
        thread_id: THREAD
            .captures(raw_message)
            .and_then(|captures| captures.name("value"))
            .map(|value| value.as_str().to_owned()),
        logger: None,
        message: raw_message.to_owned(),
    }
}

fn read_delta(
    path: &Path,
    artifact: String,
    start: u64,
    observed_at: DateTime<Utc>,
    redact_enabled: bool,
    profile: Option<&LogProfile>,
    context: &CorrelationContext,
) -> Result<Delta> {
    let mut file =
        File::open(path).with_context(|| format!("failed to open log {}", path.display()))?;
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    let raw = String::from_utf8_lossy(&bytes);
    let content = redact::text(&raw, redact_enabled);
    let options = ParseOptions {
        observed_at,
        redact_enabled,
        profile,
        context,
    };
    let events = parse_event_bytes(path, &artifact, start, &bytes, &options);
    Ok(Delta {
        artifact,
        content,
        events,
        original_bytes: bytes.len() as u64,
    })
}

fn segment_artifact(prefix: &str, index: usize, path: &Path, multiple: bool) -> String {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("application.log")
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    if multiple {
        format!("{prefix}-{index:03}-{name}")
    } else {
        format!("{prefix}-{name}")
    }
}

#[cfg(unix)]
fn file_identity(metadata: &Metadata) -> Option<FileIdentity> {
    use std::os::unix::fs::MetadataExt;

    Some(FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

#[cfg(not(unix))]
fn file_identity(_: &Metadata) -> Option<FileIdentity> {
    None
}

fn find_identity(path: &Path, expected: Option<FileIdentity>) -> Option<PathBuf> {
    let expected = expected?;
    let parent = path.parent()?;
    fs::read_dir(parent)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|candidate| {
            candidate != path
                && candidate
                    .metadata()
                    .ok()
                    .and_then(|metadata| file_identity(&metadata))
                    == Some(expected)
        })
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
        .map(|value| value.as_str())
    else {
        return Severity::Unknown;
    };
    parse_severity(value)
}

fn parse_severity(value: &str) -> Severity {
    match value.to_ascii_lowercase().as_str() {
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
