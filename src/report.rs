use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;

use anyhow::{Context, Result};

use crate::model::{
    CaptureMode, CrashEvidence, Diagnostic, Event, Manifest, Pattern, Severity, TestReport,
    TestStatus,
};

pub fn write_bundle(
    directory: &Path,
    manifest: &Manifest,
    events: &[Event],
    patterns: &[Pattern],
    tests: &[TestReport],
    diagnostics: &[Diagnostic],
    crash: &CrashEvidence,
) -> Result<()> {
    write_json(directory.join("manifest.json"), manifest)?;
    write_json(directory.join("patterns.json"), patterns)?;
    write_json(directory.join("tests.json"), tests)?;
    write_json(directory.join("diagnostics.json"), diagnostics)?;
    write_json(directory.join("crash.json"), crash)?;

    let mut writer = BufWriter::new(File::create(directory.join("events.jsonl"))?);
    for event in events {
        serde_json::to_writer(&mut writer, event)?;
        writer.write_all(b"\n")?;
    }
    writer.flush()?;

    fs::write(
        directory.join("summary.md"),
        markdown(manifest, events, patterns, tests, diagnostics, crash),
    )
    .context("failed to write summary.md")?;
    Ok(())
}

fn write_json<T: serde::Serialize + ?Sized>(path: impl AsRef<Path>, value: &T) -> Result<()> {
    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer_pretty(&mut writer, value)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn markdown(
    manifest: &Manifest,
    events: &[Event],
    patterns: &[Pattern],
    tests: &[TestReport],
    diagnostics: &[Diagnostic],
    crash: &CrashEvidence,
) -> String {
    let title = match manifest.capture_mode {
        CaptureMode::Live => "# RunSift 运行摘要\n\n",
        CaptureMode::Import => "# RunSift 历史日志摘要\n\n",
    };
    let mut output = title.to_owned();
    let evidence_type = match manifest.capture_mode {
        CaptureMode::Live => "实时运行",
        CaptureMode::Import => "历史导入",
    };
    output.push_str(&format!(
        "- 证据类型：`{evidence_type}`\n\
         - 处理开始时间：{}\n\
         - 处理结束时间：{}\n\
         - 事件数：{}\n\
         - 事件模式数：{}\n\
         - 测试数：{}（失败/错误 {}）\n\
         - Sanitizer 诊断数：{}\n\
         - 输出已脱敏：{}\n\n",
        manifest.started_at.to_rfc3339(),
        manifest.finished_at.to_rfc3339(),
        manifest.event_count,
        manifest.pattern_count,
        manifest.test_count,
        manifest.failed_test_count,
        manifest.diagnostic_count,
        if manifest.redacted { "是" } else { "否" }
    ));

    if let Some(command) = &manifest.command {
        let status = if command.success { "成功" } else { "失败" };
        let exit = command
            .exit_code
            .map_or_else(|| "被信号终止".to_owned(), |code| code.to_string());
        let command_line = std::iter::once(&command.program)
            .chain(command.args.iter())
            .cloned()
            .collect::<Vec<_>>()
            .join(" ");
        output.push_str(&format!(
            "- 运行结果：{status}\n- 退出码：{exit}\n- 命令：`{command_line}`\n\n"
        ));
    } else {
        output.push_str("- 原始命令与退出状态：历史日志未提供\n\n");
    }

    output.push_str("## 日志时间范围\n\n");
    match (manifest.observed_started_at, manifest.observed_finished_at) {
        (Some(start), Some(end)) => output.push_str(&format!(
            "- 最早时间：{}\n- 最晚时间：{}\n\n",
            start.to_rfc3339(),
            end.to_rfc3339()
        )),
        _ => output.push_str("日志中没有识别到带时区的时间戳，无法可靠建立绝对时间范围。\n\n"),
    }

    output.push_str("## 关联上下文\n\n");
    if let Some(case_id) = &manifest.case_id {
        output.push_str(&format!("- Case ID：`{case_id}`\n"));
    } else {
        output.push_str(&format!("- Run ID：`{}`\n", manifest.context.run_id));
    }
    if let Some(batch_id) = &manifest.context.batch_id {
        output.push_str(&format!("- Batch ID：`{batch_id}`\n"));
    }
    if let Some(test_id) = &manifest.context.test_id {
        output.push_str(&format!("- Test ID：`{test_id}`\n"));
    }
    if let Some(profile) = &manifest.log_profile {
        output.push_str(&format!("- 日志 Profile：`{profile}`\n"));
    }
    output.push('\n');

    if let Some(git) = &manifest.git {
        output.push_str("## 代码现场\n\n");
        if let Some(commit) = &git.commit {
            output.push_str(&format!("- Commit：`{commit}`\n"));
        } else {
            output.push_str("- Commit：尚无提交\n");
        }
        if let Some(branch) = &git.branch {
            output.push_str(&format!("- 分支：`{branch}`\n"));
        }
        output.push_str(&format!(
            "- 工作区状态：{}\n",
            if git.dirty {
                "存在未提交修改"
            } else {
                "干净"
            }
        ));
        for path in git.changed_files.iter().take(20) {
            output.push_str(&format!("  - `{path}`\n"));
        }
        output.push('\n');
    }

    write_source_summary(&mut output, manifest);
    write_event_overview(&mut output, events);

    write_test_summary(&mut output, tests);
    write_diagnostic_summary(&mut output, diagnostics);
    write_crash_summary(&mut output, crash);
    write_rotation_summary(&mut output, manifest);

    output.push_str("## 高信号事件模式\n\n");
    let failed_run = manifest
        .command
        .as_ref()
        .is_some_and(|command| !command.success);
    let mut relevant = patterns
        .iter()
        .filter(|pattern| {
            pattern.severity.priority() >= Severity::Warn.priority()
                || crate::logs::is_key_message(&pattern.template)
                || failed_run
        })
        .take(20)
        .collect::<Vec<_>>();
    if relevant.is_empty() && manifest.capture_mode == CaptureMode::Import {
        relevant.extend(patterns.iter().take(10));
    }

    if relevant.is_empty() {
        output.push_str("没有识别到 WARN 及以上事件。\n\n");
    } else {
        for pattern in relevant {
            output.push_str(&format!(
                "### `{:?}` × {} — `{}`\n\n",
                pattern.severity, pattern.count, pattern.pattern_id
            ));
            output.push_str("```text\n");
            output.push_str(&truncate(&pattern.template, 800));
            output.push_str("\n```\n\n");
            output.push_str(&format!(
                "代表事件：{}\n\n",
                pattern.representative_event_ids.join(", ")
            ));
        }
    }

    output.push_str(
        "## 证据说明\n\n\
         每个事件都包含原始来源路径和字节偏移。AI 或人工结论应引用 `event_id`，\
         并通过 `events.jsonl` 回到原始证据。`patterns.json` 仅为派生信息，不替代原始事件。\n",
    );
    output
}

fn write_source_summary(output: &mut String, manifest: &Manifest) {
    output.push_str("## 证据来源\n\n");
    if manifest.sources.is_empty() {
        output.push_str("没有记录日志来源。\n\n");
        return;
    }
    for source in manifest.sources.iter().take(50) {
        output.push_str(&format!(
            "- `{}`：{} bytes",
            source.path.display(),
            source.collected_bytes
        ));
        if let Some(hash) = &source.sha256 {
            output.push_str(&format!("，SHA-256 `{hash}`"));
        }
        output.push('\n');
    }
    output.push('\n');
}

fn write_event_overview(output: &mut String, events: &[Event]) {
    let critical = events
        .iter()
        .filter(|event| event.severity == Severity::Critical)
        .count();
    let errors = events
        .iter()
        .filter(|event| event.severity == Severity::Error)
        .count();
    let warnings = events
        .iter()
        .filter(|event| event.severity == Severity::Warn)
        .count();
    let inferred = events
        .iter()
        .filter(|event| {
            event.severity.priority() < Severity::Warn.priority()
                && crate::logs::is_key_message(&event.message)
        })
        .count();
    output.push_str("## 关键信息概览\n\n");
    output.push_str(&format!(
        "- Critical：{critical}\n- Error：{errors}\n- Warn：{warnings}\n- 关键字命中但无明确级别：{inferred}\n\n"
    ));

    let key_events = events
        .iter()
        .filter(|event| {
            event.severity.priority() >= Severity::Warn.priority()
                || crate::logs::is_key_message(&event.message)
        })
        .take(30)
        .collect::<Vec<_>>();
    if key_events.is_empty() {
        output.push_str("没有识别到高信号事件。\n\n");
        return;
    }
    for event in key_events {
        let time = event
            .timestamp
            .map_or_else(|| "无可靠时间戳".to_owned(), |value| value.to_rfc3339());
        output.push_str(&format!(
            "- `{time}` `{:?}` `{}`：{}\n  - 证据：`{}:{}-{}`\n",
            event.severity,
            event.event_id,
            truncate(&event.message.replace('\n', " "), 300),
            event.evidence.artifact,
            event.evidence.byte_start,
            event.evidence.byte_end
        ));
    }
    output.push('\n');
}

fn write_test_summary(output: &mut String, reports: &[TestReport]) {
    output.push_str("## C++ 测试结果\n\n");
    if reports.is_empty() {
        output.push_str("未提供 CTest/GoogleTest JUnit XML 报告。\n\n");
        return;
    }

    for report in reports {
        output.push_str(&format!(
            "- `{:?}` `{}`：总计 {}，通过 {}，失败 {}，错误 {}，跳过 {}\n",
            report.framework,
            report.source_path.display(),
            report.total,
            report.passed,
            report.failed,
            report.errors,
            report.skipped
        ));
    }
    output.push('\n');
    for test in reports
        .iter()
        .flat_map(|report| &report.tests)
        .filter(|test| matches!(test.status, TestStatus::Failed | TestStatus::Error))
        .take(20)
    {
        output.push_str(&format!(
            "- `{:?}` `{}` (`{}`)\n",
            test.status, test.name, test.test_id
        ));
        if let Some(message) = &test.message {
            output.push_str(&format!("  - {}\n", truncate(message, 400)));
        }
    }
    output.push('\n');
}

fn write_diagnostic_summary(output: &mut String, diagnostics: &[Diagnostic]) {
    output.push_str("## Sanitizer 诊断\n\n");
    if diagnostics.is_empty() {
        output.push_str("未识别到 ASan、UBSan 或 TSan 报告。\n\n");
        return;
    }

    for diagnostic in diagnostics.iter().take(20) {
        output.push_str(&format!(
            "- `{}`：{} (`{}`)\n",
            diagnostic.kind.label(),
            diagnostic.summary,
            diagnostic.diagnostic_id
        ));
        if let Some(frame) = diagnostic.stack_frames.first() {
            output.push_str(&format!("  - 首个栈帧：`{}`\n", frame.raw));
        }
        output.push_str(&format!(
            "  - 证据：`{}:{}`–`{}`\n",
            diagnostic.evidence.artifact,
            diagnostic.evidence.byte_start,
            diagnostic.evidence.byte_end
        ));
    }
    output.push('\n');
}

fn write_crash_summary(output: &mut String, crash: &CrashEvidence) {
    output.push_str("## 崩溃现场\n\n");
    if crash.core_dumps.is_empty() && crash.debugger_reports.is_empty() {
        output.push_str("未提供 core dump 或 GDB/LLDB 报告。\n\n");
        return;
    }
    for core in &crash.core_dumps {
        output.push_str(&format!(
            "- Core `{}`：{} bytes，格式 `{}` (`{}`)\n",
            core.path.display(),
            core.size,
            core.format,
            core.core_id
        ));
    }
    for report in &crash.debugger_reports {
        output.push_str(&format!(
            "- `{:?}` 报告 `{}`：{} 个栈帧",
            report.debugger,
            report.report_id,
            report.stack_frames.len()
        ));
        if let Some(signal) = &report.signal {
            output.push_str(&format!("，信号 `{signal}`"));
        }
        output.push('\n');
    }
    output.push('\n');
}

fn write_rotation_summary(output: &mut String, manifest: &Manifest) {
    let rotated = manifest
        .sources
        .iter()
        .filter(|source| source.rotation_detected)
        .collect::<Vec<_>>();
    if rotated.is_empty() {
        return;
    }

    output.push_str("## 日志轮转\n\n");
    for source in rotated {
        output.push_str(&format!(
            "- `{}`：{}，采集 {} 个分段\n",
            source.path.display(),
            if source.rotation_recovered {
                "已找回轮转前文件尾部"
            } else {
                "未找到轮转前文件，证据可能不完整"
            },
            source.segments.len()
        ));
    }
    output.push('\n');
}

fn truncate(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let result: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{result}…")
    } else {
        result
    }
}
