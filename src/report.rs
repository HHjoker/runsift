use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;

use anyhow::{Context, Result};

use crate::model::{
    CrashEvidence, Diagnostic, Event, Manifest, Pattern, Severity, TestReport, TestStatus,
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
        markdown(manifest, patterns, tests, diagnostics, crash),
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
    patterns: &[Pattern],
    tests: &[TestReport],
    diagnostics: &[Diagnostic],
    crash: &CrashEvidence,
) -> String {
    let status = if manifest.command.success {
        "成功"
    } else {
        "失败"
    };
    let exit = manifest
        .command
        .exit_code
        .map_or_else(|| "被信号终止".to_owned(), |code| code.to_string());
    let command = std::iter::once(&manifest.command.program)
        .chain(manifest.command.args.iter())
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");

    let mut output = format!(
        "# RunSift 运行摘要\n\n\
         - 运行结果：{status}\n\
         - 退出码：{exit}\n\
         - 命令：`{command}`\n\
         - 开始时间：{}\n\
         - 结束时间：{}\n\
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
    );

    output.push_str("## 关联上下文\n\n");
    output.push_str(&format!("- Run ID：`{}`\n", manifest.context.run_id));
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

    write_test_summary(&mut output, tests);
    write_diagnostic_summary(&mut output, diagnostics);
    write_crash_summary(&mut output, crash);
    write_rotation_summary(&mut output, manifest);

    output.push_str("## 高信号事件模式\n\n");
    let relevant = patterns
        .iter()
        .filter(|pattern| {
            pattern.severity.priority() >= Severity::Warn.priority() || !manifest.command.success
        })
        .take(20)
        .collect::<Vec<_>>();

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
