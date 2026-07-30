use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::logs::stable_id;
use crate::model::{TestCase, TestFramework, TestReport, TestStatus};
use crate::redact;

pub struct ImportedTestReport {
    pub content: String,
    pub report: TestReport,
}

#[derive(Debug, Default, Deserialize)]
struct SuitesXml {
    #[serde(rename = "testsuite", default)]
    suites: Vec<SuiteXml>,
}

#[derive(Debug, Default, Deserialize)]
struct SuiteXml {
    #[serde(rename = "@name", default)]
    name: String,
    #[serde(rename = "testcase", default)]
    cases: Vec<CaseXml>,
    #[serde(rename = "testsuite", default)]
    suites: Vec<SuiteXml>,
}

#[derive(Debug, Default, Deserialize)]
struct CaseXml {
    #[serde(rename = "@name", default)]
    name: String,
    #[serde(rename = "@classname")]
    class_name: Option<String>,
    #[serde(rename = "@time")]
    time: Option<String>,
    #[serde(default)]
    failure: Vec<OutcomeXml>,
    #[serde(default)]
    error: Vec<OutcomeXml>,
    #[serde(default)]
    skipped: Vec<OutcomeXml>,
}

#[derive(Debug, Default, Deserialize)]
struct OutcomeXml {
    #[serde(rename = "@message")]
    message: Option<String>,
    #[serde(rename = "$text")]
    text: Option<String>,
}

pub fn import(path: &Path, artifact: String, redact_enabled: bool) -> Result<ImportedTestReport> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read test report {}", path.display()))?;
    let framework = detect_framework(path, &content);
    let trimmed = content.trim_start();
    let suites = if trimmed.starts_with("<testsuites")
        || trimmed.starts_with("<?xml") && trimmed.contains("<testsuites")
    {
        quick_xml::de::from_str::<SuitesXml>(&content)
            .context("invalid JUnit testsuites XML")?
            .suites
    } else if trimmed.starts_with("<testsuite")
        || trimmed.starts_with("<?xml") && trimmed.contains("<testsuite")
    {
        vec![quick_xml::de::from_str::<SuiteXml>(&content).context("invalid JUnit testsuite XML")?]
    } else {
        bail!("unsupported XML root; expected testsuites or testsuite");
    };

    let mut tests = Vec::new();
    flatten_suites(path, "", suites, redact_enabled, &mut tests);
    let failed = tests
        .iter()
        .filter(|test| test.status == TestStatus::Failed)
        .count();
    let errors = tests
        .iter()
        .filter(|test| test.status == TestStatus::Error)
        .count();
    let skipped = tests
        .iter()
        .filter(|test| test.status == TestStatus::Skipped)
        .count();
    let passed = tests.len() - failed - errors - skipped;

    Ok(ImportedTestReport {
        content: redact_xml(&content, redact_enabled),
        report: TestReport {
            source_path: path.to_path_buf(),
            artifact,
            framework,
            total: tests.len(),
            passed,
            failed,
            errors,
            skipped,
            tests,
        },
    })
}

fn redact_xml(content: &str, enabled: bool) -> String {
    redact::text(content, enabled)
        .replace("<redacted>", "&lt;redacted&gt;")
        .replace("<redacted-jwt>", "&lt;redacted-jwt&gt;")
}

pub fn artifact(index: usize, path: &Path) -> String {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("test-results.xml")
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    format!("tests/{index:03}-{name}")
}

fn flatten_suites(
    source: &Path,
    parent: &str,
    suites: Vec<SuiteXml>,
    redact_enabled: bool,
    output: &mut Vec<TestCase>,
) {
    for suite in suites {
        let suite_name = match (parent.is_empty(), suite.name.is_empty()) {
            (true, true) => "default".to_owned(),
            (true, false) => suite.name.clone(),
            (false, true) => parent.to_owned(),
            (false, false) => format!("{parent}::{}", suite.name),
        };

        for case in suite.cases {
            let (status, outcome) = if let Some(value) = case.error.first() {
                (TestStatus::Error, Some(value))
            } else if let Some(value) = case.failure.first() {
                (TestStatus::Failed, Some(value))
            } else if let Some(value) = case.skipped.first() {
                (TestStatus::Skipped, Some(value))
            } else {
                (TestStatus::Passed, None)
            };
            let message = outcome
                .and_then(|value| value.message.as_ref().or(value.text.as_ref()))
                .map(|value| redact::text(value.trim(), redact_enabled))
                .filter(|value| !value.is_empty());
            let duration_ms = case
                .time
                .as_deref()
                .and_then(|value| value.parse::<f64>().ok())
                .map(|seconds| seconds * 1000.0);
            let test_id = stable_id(
                "test",
                &format!("{}:{suite_name}:{}", source.display(), case.name),
            );

            output.push(TestCase {
                test_id,
                suite: suite_name.clone(),
                name: case.name,
                class_name: case.class_name,
                status,
                duration_ms,
                message,
            });
        }

        flatten_suites(source, &suite_name, suite.suites, redact_enabled, output);
    }
}

fn detect_framework(path: &Path, content: &str) -> TestFramework {
    let hint = path.to_string_lossy().to_ascii_lowercase();
    if content.contains("status=\"run\"")
        || content.contains("result=\"completed\"")
        || hint.contains("gtest")
        || hint.contains("googletest")
    {
        TestFramework::GoogleTest
    } else if hint.contains("ctest") || content.contains("CTest") {
        TestFramework::CTest
    } else {
        TestFramework::JUnit
    }
}
