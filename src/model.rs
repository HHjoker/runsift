use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CorrelationContext {
    pub run_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub batch_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceRef {
    pub artifact: String,
    pub source_path: PathBuf,
    pub byte_start: u64,
    pub byte_end: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub event_id: String,
    pub context: CorrelationContext,
    pub observed_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<DateTime<Utc>>,
    pub severity: Severity,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logger: Option<String>,
    pub message: String,
    pub evidence: EvidenceRef,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pattern {
    pub pattern_id: String,
    pub severity: Severity,
    pub template: String,
    pub count: usize,
    pub first_observed_at: DateTime<Utc>,
    pub last_observed_at: DateTime<Utc>,
    pub representative_event_ids: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GitInfo {
    pub root: PathBuf,
    pub commit: Option<String>,
    pub branch: Option<String>,
    pub dirty: bool,
    pub changed_files: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SourceSummary {
    pub path: PathBuf,
    pub initial_size: u64,
    pub final_size: u64,
    pub collected_bytes: u64,
    pub reset_detected: bool,
    pub rotation_detected: bool,
    pub rotation_recovered: bool,
    pub segments: Vec<SourceSegment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceSegment {
    pub path: PathBuf,
    pub artifact: String,
    pub byte_start: u64,
    pub byte_end: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TestFramework {
    CTest,
    GoogleTest,
    JUnit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TestStatus {
    Passed,
    Failed,
    Error,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestCase {
    pub test_id: String,
    pub suite: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class_name: Option<String>,
    pub status: TestStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestReport {
    pub source_path: PathBuf,
    pub artifact: String,
    pub framework: TestFramework,
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub errors: usize,
    pub skipped: usize,
    pub tests: Vec<TestCase>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticKind {
    #[serde(rename = "address_sanitizer")]
    Address,
    #[serde(rename = "undefined_behavior_sanitizer")]
    UndefinedBehavior,
    #[serde(rename = "thread_sanitizer")]
    Thread,
}

impl DiagnosticKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Address => "ASan",
            Self::UndefinedBehavior => "UBSan",
            Self::Thread => "TSan",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StackFrame {
    pub index: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    pub raw: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    pub diagnostic_id: String,
    pub context: CorrelationContext,
    pub kind: DiagnosticKind,
    pub summary: String,
    pub stack_frames: Vec<StackFrame>,
    pub evidence: EvidenceRef,
    pub event_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreDump {
    pub core_id: String,
    pub path: PathBuf,
    pub size: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_at: Option<DateTime<Utc>>,
    pub format: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DebuggerKind {
    Gdb,
    Lldb,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebuggerReport {
    pub report_id: String,
    pub context: CorrelationContext,
    pub source_path: PathBuf,
    pub artifact: String,
    pub debugger: DebuggerKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal: Option<String>,
    pub stack_frames: Vec<StackFrame>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrashEvidence {
    pub core_dumps: Vec<CoreDump>,
    pub debugger_reports: Vec<DebuggerReport>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CommandResult {
    pub program: String,
    pub args: Vec<String>,
    pub exit_code: Option<i32>,
    pub success: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub schema_version: u32,
    pub run_id: String,
    pub context: CorrelationContext,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub working_directory: PathBuf,
    pub command: CommandResult,
    pub redacted: bool,
    pub git: Option<GitInfo>,
    pub sources: Vec<SourceSummary>,
    pub event_count: usize,
    pub pattern_count: usize,
    pub test_count: usize,
    pub failed_test_count: usize,
    pub diagnostic_count: usize,
    pub core_dump_count: usize,
    pub debugger_report_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_profile: Option<String>,
    pub artifacts: Vec<String>,
}
