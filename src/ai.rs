use std::collections::HashSet;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::cli::{AnalyzeAdapter, AnalyzeArgs, ContextArgs, OpenAiAdapterArgs, OpenAiApi};
use crate::logs::stable_id;
use crate::model::{
    CrashEvidence, Diagnostic, Event, Manifest, Pattern, Severity, TestReport, TestStatus,
};
use crate::redact;

pub const CONTEXT_PROTOCOL: &str = "runsift.diagnostic-context";
pub const CONTEXT_PROTOCOL_VERSION: u32 = 1;
pub const ANALYSIS_PROTOCOL: &str = "runsift.analysis";
pub const ANALYSIS_PROTOCOL_VERSION: u32 = 1;
const MIN_TOKEN_BUDGET: usize = 600;

#[derive(Debug)]
pub struct EvidenceBundle {
    pub directory: PathBuf,
    pub manifest: Manifest,
    pub events: Vec<Event>,
    pub patterns: Vec<Pattern>,
    pub tests: Vec<TestReport>,
    pub diagnostics: Vec<Diagnostic>,
    pub crash: CrashEvidence,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticContext {
    pub protocol: String,
    pub protocol_version: u32,
    pub generated_at: DateTime<Utc>,
    pub source: ContextSource,
    pub budget: ContextBudget,
    pub run: ContextRun,
    pub facts: Vec<ContextFact>,
    pub hypotheses: Vec<ContextHypothesis>,
    pub missing_information: Vec<MissingInformation>,
    pub evidence: Vec<SelectedEvidence>,
    pub response_contract: ResponseContract,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextSource {
    pub bundle: PathBuf,
    pub evidence_schema_version: u32,
    pub run_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextBudget {
    pub max_tokens: usize,
    pub estimated_tokens: usize,
    pub estimator: String,
    pub candidate_count: usize,
    pub selected_count: usize,
    pub omitted_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextRun {
    pub command: String,
    pub success: bool,
    pub exit_code: Option<i32>,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub git_commit: Option<String>,
    pub git_branch: Option<String>,
    pub git_dirty: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextFact {
    pub fact_id: String,
    pub kind: String,
    pub statement: String,
    pub evidence_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextHypothesis {
    pub hypothesis_id: String,
    pub statement: String,
    pub evidence_ids: Vec<String>,
    pub verification_needed: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissingInformation {
    pub missing_id: String,
    pub description: String,
    pub impact: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectedEvidence {
    pub evidence_id: String,
    pub kind: String,
    pub priority: u16,
    pub content: String,
    pub source: String,
    pub related_evidence_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseContract {
    pub format: String,
    pub require_evidence_citations: bool,
    pub allowed_evidence_ids: Vec<String>,
    pub schema: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisResult {
    pub schema_version: u32,
    pub summary: String,
    pub findings: Vec<AnalysisFinding>,
    pub hypotheses: Vec<AnalysisHypothesis>,
    pub missing_information: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisFinding {
    pub title: String,
    pub explanation: String,
    pub severity: AnalysisSeverity,
    pub evidence_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisHypothesis {
    pub statement: String,
    pub confidence: AnalysisConfidence,
    pub evidence_ids: Vec<String>,
    pub verification_step: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnalysisSeverity {
    Info,
    Warning,
    Error,
    Critical,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnalysisConfidence {
    Low,
    Medium,
    High,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AnalysisEnvelope {
    pub protocol: String,
    pub protocol_version: u32,
    pub generated_at: DateTime<Utc>,
    pub source_run_id: String,
    pub adapter: String,
    pub analysis: AnalysisResult,
}

#[derive(Debug)]
struct Candidate {
    evidence: SelectedEvidence,
    fact: ContextFact,
}

pub fn context_command(args: ContextArgs) -> Result<i32> {
    let bundle = EvidenceBundle::load(&args.bundle)?;
    let context = build_context(&bundle, args.token_budget)?;
    if args.stdout {
        serde_json::to_writer_pretty(std::io::stdout().lock(), &context)?;
        println!();
        return Ok(0);
    }

    let directory = args.bundle.join("ai");
    let context_path = args
        .output
        .unwrap_or_else(|| directory.join("context.json"));
    let prompt_path = args
        .prompt_output
        .unwrap_or_else(|| directory.join("prompt.md"));
    ensure_outputs_available(&[&context_path, &prompt_path], args.force)?;
    write_json(&context_path, &context, args.force)?;
    write_text(&prompt_path, &prompt(&context)?, args.force)?;
    eprintln!("runsift: AI context: {}", context_path.display());
    eprintln!("runsift: AI prompt: {}", prompt_path.display());
    Ok(0)
}

pub fn analyze_command(args: AnalyzeArgs) -> Result<i32> {
    let bundle = EvidenceBundle::load(&args.bundle)?;
    let context = build_context(&bundle, args.token_budget)?;
    let prompt = prompt(&context)?;
    let output = args
        .output
        .clone()
        .unwrap_or_else(|| args.bundle.join("ai/analysis.json"));
    ensure_outputs_available(&[&output], args.force)?;
    let (adapter, raw) = match args.adapter {
        AnalyzeAdapter::Local(local) => {
            let label = local
                .command
                .first()
                .map(|value| format!("local:{}", value.to_string_lossy()))
                .unwrap_or_else(|| "local".to_owned());
            (label, run_local(&local.command, &prompt)?)
        }
        AnalyzeAdapter::Openai(openai) => {
            let label = format!("openai:{}", openai.model);
            (label, run_openai(&openai, &prompt)?)
        }
    };
    let analysis = parse_analysis(&raw)?;
    validate_analysis(&analysis, &context)?;
    let envelope = AnalysisEnvelope {
        protocol: ANALYSIS_PROTOCOL.to_owned(),
        protocol_version: ANALYSIS_PROTOCOL_VERSION,
        generated_at: Utc::now(),
        source_run_id: context.source.run_id,
        adapter,
        analysis,
    };
    write_json(&output, &envelope, args.force)?;
    eprintln!("runsift: validated AI analysis: {}", output.display());
    Ok(0)
}

impl EvidenceBundle {
    pub fn load(directory: &Path) -> Result<Self> {
        let manifest: Manifest = read_json(&directory.join("manifest.json"))?;
        if manifest.schema_version != 2 {
            bail!(
                "unsupported evidence schema {}, expected 2",
                manifest.schema_version
            );
        }
        let events = read_jsonl(&directory.join("events.jsonl"))?;
        let patterns = read_json(&directory.join("patterns.json"))?;
        let tests = read_json(&directory.join("tests.json"))?;
        let diagnostics = read_json(&directory.join("diagnostics.json"))?;
        let crash = read_json(&directory.join("crash.json"))?;
        Ok(Self {
            directory: directory.to_path_buf(),
            manifest,
            events,
            patterns,
            tests,
            diagnostics,
            crash,
        })
    }
}

pub fn build_context(bundle: &EvidenceBundle, token_budget: usize) -> Result<DiagnosticContext> {
    if token_budget < MIN_TOKEN_BUDGET {
        bail!("token budget must be at least {MIN_TOKEN_BUDGET}");
    }
    let candidates = candidates(bundle);
    let candidate_count = candidates.len();
    let mut selected = Vec::new();
    let mut facts = Vec::new();
    let missing = base_missing_information(bundle);
    let mut context = empty_context(bundle, token_budget, candidate_count, missing);
    let base_tokens = estimate_tokens(&prompt(&context)?);
    if base_tokens >= token_budget {
        bail!(
            "token budget {token_budget} is too small for protocol overhead (about {base_tokens} tokens)"
        );
    }
    let mut remaining = token_budget - base_tokens;

    for candidate in candidates {
        let cost = estimate_tokens(&serde_json::to_string(&candidate.evidence)?)
            + estimate_tokens(&serde_json::to_string(&candidate.fact)?);
        if cost <= remaining {
            remaining -= cost;
            selected.push(candidate.evidence);
            facts.push(candidate.fact);
        }
    }

    context.evidence = selected;
    context.facts = facts;
    finish_context(&mut context, candidate_count)?;
    while context.budget.estimated_tokens > token_budget && !context.evidence.is_empty() {
        context.evidence.pop();
        context.facts.pop();
        finish_context(&mut context, candidate_count)?;
    }
    if context.evidence.is_empty() && candidate_count > 0 {
        bail!("token budget {token_budget} leaves no room for evidence; increase --token-budget");
    }
    Ok(context)
}

fn empty_context(
    bundle: &EvidenceBundle,
    token_budget: usize,
    candidate_count: usize,
    missing_information: Vec<MissingInformation>,
) -> DiagnosticContext {
    let command = std::iter::once(&bundle.manifest.command.program)
        .chain(bundle.manifest.command.args.iter())
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");
    let command = redact::text(&command, bundle.manifest.redacted);
    let git = bundle.manifest.git.as_ref();
    DiagnosticContext {
        protocol: CONTEXT_PROTOCOL.to_owned(),
        protocol_version: CONTEXT_PROTOCOL_VERSION,
        generated_at: Utc::now(),
        source: ContextSource {
            bundle: bundle.directory.clone(),
            evidence_schema_version: bundle.manifest.schema_version,
            run_id: bundle.manifest.run_id.clone(),
        },
        budget: ContextBudget {
            max_tokens: token_budget,
            estimated_tokens: 0,
            estimator: "runsift-char-v1 (approximate, provider-independent)".to_owned(),
            candidate_count,
            selected_count: 0,
            omitted_count: candidate_count,
        },
        run: ContextRun {
            command,
            success: bundle.manifest.command.success,
            exit_code: bundle.manifest.command.exit_code,
            started_at: bundle.manifest.started_at,
            finished_at: bundle.manifest.finished_at,
            git_commit: git.and_then(|value| value.commit.clone()),
            git_branch: git.and_then(|value| value.branch.clone()),
            git_dirty: git.map(|value| value.dirty),
        },
        facts: Vec::new(),
        hypotheses: Vec::new(),
        missing_information,
        evidence: Vec::new(),
        response_contract: ResponseContract {
            format: "json".to_owned(),
            require_evidence_citations: true,
            allowed_evidence_ids: Vec::new(),
            schema: analysis_schema(),
        },
    }
}

fn finish_context(context: &mut DiagnosticContext, candidate_count: usize) -> Result<()> {
    let mut allowed = context
        .evidence
        .iter()
        .map(|evidence| evidence.evidence_id.clone())
        .collect::<Vec<_>>();
    allowed.sort();
    context.response_contract.allowed_evidence_ids = allowed;
    context.budget.selected_count = context.evidence.len();
    context.budget.omitted_count = candidate_count.saturating_sub(context.evidence.len());
    context
        .missing_information
        .retain(|value| value.missing_id != "missing_budget");
    if context.budget.omitted_count > 0 {
        context.missing_information.push(MissingInformation {
            missing_id: "missing_budget".to_owned(),
            description: format!(
                "{} lower-priority evidence candidates were omitted by the token budget",
                context.budget.omitted_count
            ),
            impact: "The model must not assume omitted evidence supports or disproves a conclusion"
                .to_owned(),
        });
    }
    context.budget.estimated_tokens = 0;
    let first = estimate_tokens(&prompt(context)?);
    context.budget.estimated_tokens = first;
    context.budget.estimated_tokens = estimate_tokens(&prompt(context)?);
    Ok(())
}

fn candidates(bundle: &EvidenceBundle) -> Vec<Candidate> {
    let mut output = Vec::new();
    output.push(candidate(
        bundle.manifest.run_id.clone(),
        "run",
        120,
        format!(
            "Command `{}` {} with exit code {:?}",
            bundle.manifest.command.program,
            if bundle.manifest.command.success {
                "succeeded"
            } else {
                "failed"
            },
            bundle.manifest.command.exit_code
        ),
        "manifest.json".to_owned(),
        Vec::new(),
    ));

    for diagnostic in &bundle.diagnostics {
        let frames = diagnostic
            .stack_frames
            .iter()
            .take(8)
            .map(|frame| frame.raw.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let content = if frames.is_empty() {
            format!("{}: {}", diagnostic.kind.label(), diagnostic.summary)
        } else {
            format!(
                "{}: {}\n{frames}",
                diagnostic.kind.label(),
                diagnostic.summary
            )
        };
        output.push(candidate(
            diagnostic.diagnostic_id.clone(),
            "sanitizer_diagnostic",
            115,
            content,
            format!(
                "{}:{}-{}",
                diagnostic.evidence.artifact,
                diagnostic.evidence.byte_start,
                diagnostic.evidence.byte_end
            ),
            diagnostic.event_ids.clone(),
        ));
    }

    for report in &bundle.tests {
        for test in report
            .tests
            .iter()
            .filter(|test| matches!(test.status, TestStatus::Failed | TestStatus::Error))
        {
            output.push(candidate(
                test.test_id.clone(),
                "failed_test",
                110,
                format!(
                    "Test {}::{} {:?}: {}",
                    test.suite,
                    test.name,
                    test.status,
                    test.message.as_deref().unwrap_or("no failure message")
                ),
                report.artifact.clone(),
                Vec::new(),
            ));
        }
    }

    for report in &bundle.crash.debugger_reports {
        let frames = report
            .stack_frames
            .iter()
            .take(12)
            .map(|frame| frame.raw.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        output.push(candidate(
            report.report_id.clone(),
            "debugger_report",
            108,
            format!("Signal {:?}\n{frames}", report.signal),
            report.artifact.clone(),
            Vec::new(),
        ));
    }
    for core in &bundle.crash.core_dumps {
        output.push(candidate(
            core.core_id.clone(),
            "core_dump",
            105,
            format!(
                "Core dump {}: {} bytes, format {}",
                core.path.display(),
                core.size,
                core.format
            ),
            core.path.display().to_string(),
            Vec::new(),
        ));
    }

    for pattern in bundle.patterns.iter().take(200) {
        let priority = 60 + u16::from(pattern.severity.priority()) * 5;
        output.push(candidate(
            pattern.pattern_id.clone(),
            "event_pattern",
            priority,
            format!(
                "{:?} pattern observed {} times: {}",
                pattern.severity, pattern.count, pattern.template
            ),
            "patterns.json".to_owned(),
            pattern.representative_event_ids.clone(),
        ));
    }

    for event in bundle
        .events
        .iter()
        .filter(|event| event.severity.priority() >= Severity::Warn.priority())
        .take(200)
    {
        output.push(candidate(
            event.event_id.clone(),
            "event",
            55 + u16::from(event.severity.priority()) * 5,
            event.message.clone(),
            format!(
                "{}:{}-{}",
                event.evidence.artifact, event.evidence.byte_start, event.evidence.byte_end
            ),
            Vec::new(),
        ));
    }
    output.sort_by(|left, right| {
        right
            .evidence
            .priority
            .cmp(&left.evidence.priority)
            .then_with(|| left.evidence.evidence_id.cmp(&right.evidence.evidence_id))
    });
    let mut seen = HashSet::new();
    output.retain(|candidate| seen.insert(candidate.evidence.evidence_id.clone()));
    output
}

fn candidate(
    evidence_id: String,
    kind: &str,
    priority: u16,
    content: String,
    source: String,
    related_evidence_ids: Vec<String>,
) -> Candidate {
    Candidate {
        fact: ContextFact {
            fact_id: stable_id("fact", &evidence_id),
            kind: kind.to_owned(),
            statement: content.clone(),
            evidence_ids: vec![evidence_id.clone()],
        },
        evidence: SelectedEvidence {
            evidence_id,
            kind: kind.to_owned(),
            priority,
            content,
            source,
            related_evidence_ids,
        },
    }
}

fn base_missing_information(bundle: &EvidenceBundle) -> Vec<MissingInformation> {
    let mut items = Vec::new();
    if bundle.tests.is_empty() {
        items.push(missing(
            "missing_tests",
            "No structured CTest/GoogleTest report was provided",
            "A failed command cannot be mapped reliably to a specific test case",
        ));
    }
    if bundle.diagnostics.is_empty() {
        items.push(missing(
            "missing_sanitizer",
            "No ASan, UBSan, or TSan diagnostic was captured",
            "Memory and concurrency faults may still exist but are not evidenced",
        ));
    }
    if bundle.crash.core_dumps.is_empty() && bundle.crash.debugger_reports.is_empty() {
        items.push(missing(
            "missing_crash_context",
            "No core metadata or debugger report was provided",
            "A crash root cause may require stack and signal context",
        ));
    }
    if bundle
        .manifest
        .sources
        .iter()
        .any(|source| source.rotation_detected && !source.rotation_recovered)
    {
        items.push(missing(
            "missing_rotated_tail",
            "At least one rotated log tail could not be recovered",
            "Relevant events may be absent from the evidence bundle",
        ));
    }
    items
}

fn missing(id: &str, description: &str, impact: &str) -> MissingInformation {
    MissingInformation {
        missing_id: id.to_owned(),
        description: description.to_owned(),
        impact: impact.to_owned(),
    }
}

pub fn prompt(context: &DiagnosticContext) -> Result<String> {
    let context_json = serde_json::to_string_pretty(context)?;
    Ok(format!(
        "# RunSift diagnostic analysis\n\n\
         Analyze only the supplied runtime evidence. Preserve business intent: do not recommend changing production code or tests merely to make a failure disappear.\n\n\
         Rules:\n\
         1. Treat `facts` as observed or mechanically derived evidence.\n\
         2. Keep uncertain causal explanations in `hypotheses`.\n\
         3. Put evidence gaps in `missing_information`.\n\
         4. Every finding and hypothesis must cite one or more IDs from `response_contract.allowed_evidence_ids`.\n\
         5. Return only JSON matching `response_contract.schema`; do not use Markdown fences.\n\n\
         Diagnostic context:\n```json\n{context_json}\n```\n"
    ))
}

pub fn estimate_tokens(value: &str) -> usize {
    let ascii = value
        .chars()
        .filter(|character| character.is_ascii())
        .count();
    let non_ascii = value.chars().count() - ascii;
    ascii.div_ceil(4) + non_ascii
}

pub fn parse_analysis(raw: &str) -> Result<AnalysisResult> {
    let trimmed = raw.trim();
    let json = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .and_then(|value| value.strip_suffix("```"))
        .map(str::trim)
        .unwrap_or(trimmed);
    serde_json::from_str(json).context("model did not return valid RunSift analysis JSON")
}

pub fn validate_analysis(analysis: &AnalysisResult, context: &DiagnosticContext) -> Result<()> {
    if analysis.schema_version != ANALYSIS_PROTOCOL_VERSION {
        bail!(
            "analysis schema version {} is unsupported, expected {}",
            analysis.schema_version,
            ANALYSIS_PROTOCOL_VERSION
        );
    }
    if analysis.summary.trim().is_empty() {
        bail!("analysis summary cannot be empty");
    }
    let allowed = context
        .response_contract
        .allowed_evidence_ids
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    for (kind, index, ids) in analysis
        .findings
        .iter()
        .enumerate()
        .map(|(index, value)| ("finding", index, &value.evidence_ids))
        .chain(
            analysis
                .hypotheses
                .iter()
                .enumerate()
                .map(|(index, value)| ("hypothesis", index, &value.evidence_ids)),
        )
    {
        if ids.is_empty() {
            bail!("{kind} {index} has no evidence citation");
        }
        for id in ids {
            if !allowed.contains(id.as_str()) {
                bail!("{kind} {index} cites unavailable evidence ID `{id}`");
            }
        }
    }
    Ok(())
}

fn run_local(command: &[OsString], prompt: &str) -> Result<String> {
    let (program, args) = command
        .split_first()
        .context("local adapter command is required after `--`")?;
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| {
            format!(
                "failed to start local adapter {}",
                program.to_string_lossy()
            )
        })?;
    child
        .stdin
        .take()
        .context("failed to open local adapter stdin")?
        .write_all(prompt.as_bytes())?;
    let output = child.wait_with_output()?;
    if !output.status.success() {
        bail!(
            "local adapter failed with {:?}: {}",
            output.status.code(),
            truncate(&String::from_utf8_lossy(&output.stderr), 1_000)
        );
    }
    String::from_utf8(output.stdout).context("local adapter stdout is not UTF-8")
}

fn run_openai(config: &OpenAiAdapterArgs, prompt: &str) -> Result<String> {
    if config.model.trim().is_empty() {
        bail!("OpenAI-compatible model cannot be empty");
    }
    if config.timeout == 0 {
        bail!("HTTP timeout must be greater than zero");
    }
    let endpoint = format!(
        "{}/{}",
        config.base_url.trim_end_matches('/'),
        match config.api {
            OpenAiApi::Responses => "responses",
            OpenAiApi::ChatCompletions => "chat/completions",
        }
    );
    let body = openai_request(config, prompt);
    let client = Client::builder()
        .timeout(Duration::from_secs(config.timeout))
        .user_agent(concat!("runsift/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let mut request = client.post(&endpoint).json(&body);
    if let Some(name) = &config.api_key_env {
        let key = std::env::var(name)
            .with_context(|| format!("API key environment variable `{name}` is not set"))?;
        if key.trim().is_empty() {
            bail!("API key environment variable `{name}` is empty");
        }
        request = request.bearer_auth(key);
    }
    let response = request
        .send()
        .with_context(|| format!("failed to call {endpoint}"))?;
    let status = response.status();
    let response_body = response.text()?;
    if !status.is_success() {
        bail!(
            "OpenAI-compatible endpoint returned {status}: {}",
            truncate(&response_body, 1_500)
        );
    }
    let value: Value = serde_json::from_str(&response_body)
        .context("OpenAI-compatible endpoint returned invalid JSON")?;
    extract_model_text(&value, config.api)
}

fn openai_request(config: &OpenAiAdapterArgs, prompt: &str) -> Value {
    let format = json!({
        "type": "json_schema",
        "name": "runsift_analysis",
        "strict": true,
        "schema": analysis_schema()
    });
    match config.api {
        OpenAiApi::Responses => {
            let mut body = json!({
                "model": config.model,
                "input": prompt,
                "store": false
            });
            if !config.plain_json {
                body["text"] = json!({ "format": format });
            }
            body
        }
        OpenAiApi::ChatCompletions => {
            let mut body = json!({
                "model": config.model,
                "messages": [{ "role": "user", "content": prompt }]
            });
            if !config.plain_json {
                body["response_format"] = json!({
                    "type": "json_schema",
                    "json_schema": {
                        "name": "runsift_analysis",
                        "strict": true,
                        "schema": analysis_schema()
                    }
                });
            }
            body
        }
    }
}

fn extract_model_text(value: &Value, api: OpenAiApi) -> Result<String> {
    if let Some(text) = value.get("output_text").and_then(Value::as_str) {
        return Ok(text.to_owned());
    }
    let text = match api {
        OpenAiApi::Responses => value
            .get("output")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .flat_map(|item| {
                item.get("content")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
            })
            .find_map(|content| content.get("text").and_then(Value::as_str)),
        OpenAiApi::ChatCompletions => value
            .pointer("/choices/0/message/content")
            .and_then(Value::as_str),
    };
    text.map(str::to_owned)
        .context("OpenAI-compatible response contains no model text")
}

pub fn analysis_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["schema_version", "summary", "findings", "hypotheses", "missing_information"],
        "properties": {
            "schema_version": { "type": "integer", "const": ANALYSIS_PROTOCOL_VERSION },
            "summary": { "type": "string", "minLength": 1 },
            "findings": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["title", "explanation", "severity", "evidence_ids"],
                    "properties": {
                        "title": { "type": "string", "minLength": 1 },
                        "explanation": { "type": "string", "minLength": 1 },
                        "severity": { "type": "string", "enum": ["info", "warning", "error", "critical"] },
                        "evidence_ids": { "type": "array", "minItems": 1, "items": { "type": "string" } }
                    }
                }
            },
            "hypotheses": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["statement", "confidence", "evidence_ids", "verification_step"],
                    "properties": {
                        "statement": { "type": "string", "minLength": 1 },
                        "confidence": { "type": "string", "enum": ["low", "medium", "high"] },
                        "evidence_ids": { "type": "array", "minItems": 1, "items": { "type": "string" } },
                        "verification_step": { "type": "string", "minLength": 1 }
                    }
                }
            },
            "missing_information": { "type": "array", "items": { "type": "string" } }
        }
    })
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    serde_json::from_reader(BufReader::new(file))
        .with_context(|| format!("invalid JSON in {}", path.display()))
}

fn read_jsonl<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<Vec<T>> {
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    BufReader::new(file)
        .lines()
        .enumerate()
        .filter_map(|(index, line)| match line {
            Ok(value) if value.trim().is_empty() => None,
            value => Some((index, value)),
        })
        .map(|(index, line)| {
            let line = line?;
            serde_json::from_str(&line)
                .with_context(|| format!("invalid JSON at {}:{}", path.display(), index + 1))
        })
        .collect()
}

fn write_json(path: &Path, value: &impl Serialize, force: bool) -> Result<()> {
    let mut content = serde_json::to_vec_pretty(value)?;
    content.push(b'\n');
    write_bytes(path, &content, force)
}

fn write_text(path: &Path, value: &str, force: bool) -> Result<()> {
    write_bytes(path, value.as_bytes(), force)
}

fn write_bytes(path: &Path, value: &[u8], force: bool) -> Result<()> {
    if path.exists() && !force {
        bail!(
            "{} already exists; pass --force to replace it",
            path.display()
        );
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, value).with_context(|| format!("failed to write {}", path.display()))
}

fn ensure_outputs_available(paths: &[&Path], force: bool) -> Result<()> {
    let mut unique = HashSet::new();
    for path in paths {
        if !unique.insert(*path) {
            bail!("output paths must be different: {}", path.display());
        }
        if path.exists() && !force {
            bail!(
                "{} already exists; pass --force to replace it",
                path.display()
            );
        }
    }
    Ok(())
}

fn truncate(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let result = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{result}…")
    } else {
        result
    }
}
