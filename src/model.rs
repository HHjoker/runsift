use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
    Critical,
    Unknown,
}

impl Severity {
    pub fn priority(self) -> u8 {
        match self {
            Self::Critical => 6,
            Self::Error => 5,
            Self::Warn => 4,
            Self::Info => 3,
            Self::Debug => 2,
            Self::Trace => 1,
            Self::Unknown => 0,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct EvidenceRef {
    pub artifact: String,
    pub source_path: PathBuf,
    pub byte_start: u64,
    pub byte_end: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Event {
    pub event_id: String,
    pub observed_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<DateTime<Utc>>,
    pub severity: Severity,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    pub message: String,
    pub evidence: EvidenceRef,
}

#[derive(Debug, Clone, Serialize)]
pub struct Pattern {
    pub pattern_id: String,
    pub severity: Severity,
    pub template: String,
    pub count: usize,
    pub first_observed_at: DateTime<Utc>,
    pub last_observed_at: DateTime<Utc>,
    pub representative_event_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct GitInfo {
    pub root: PathBuf,
    pub commit: Option<String>,
    pub branch: Option<String>,
    pub dirty: bool,
    pub changed_files: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct SourceSummary {
    pub path: PathBuf,
    pub initial_size: u64,
    pub final_size: u64,
    pub collected_bytes: u64,
    pub reset_detected: bool,
}

#[derive(Debug, Serialize)]
pub struct CommandResult {
    pub program: String,
    pub args: Vec<String>,
    pub exit_code: Option<i32>,
    pub success: bool,
}

#[derive(Debug, Serialize)]
pub struct Manifest {
    pub schema_version: u32,
    pub run_id: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub working_directory: PathBuf,
    pub command: CommandResult,
    pub redacted: bool,
    pub git: Option<GitInfo>,
    pub sources: Vec<SourceSummary>,
    pub event_count: usize,
    pub pattern_count: usize,
    pub artifacts: Vec<String>,
}
