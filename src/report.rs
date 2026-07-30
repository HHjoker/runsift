use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;

use anyhow::{Context, Result};

use crate::model::{Event, Manifest, Pattern, Severity};

pub fn write_bundle(
    directory: &Path,
    manifest: &Manifest,
    events: &[Event],
    patterns: &[Pattern],
) -> Result<()> {
    write_json(directory.join("manifest.json"), manifest)?;
    write_json(directory.join("patterns.json"), patterns)?;

    let mut writer = BufWriter::new(File::create(directory.join("events.jsonl"))?);
    for event in events {
        serde_json::to_writer(&mut writer, event)?;
        writer.write_all(b"\n")?;
    }
    writer.flush()?;

    fs::write(directory.join("summary.md"), markdown(manifest, patterns))
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

fn markdown(manifest: &Manifest, patterns: &[Pattern]) -> String {
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
        "# LogLens 运行摘要\n\n\
         - 运行结果：{status}\n\
         - 退出码：{exit}\n\
         - 命令：`{command}`\n\
         - 开始时间：{}\n\
         - 结束时间：{}\n\
         - 事件数：{}\n\
         - 事件模式数：{}\n\
         - 输出已脱敏：{}\n\n",
        manifest.started_at.to_rfc3339(),
        manifest.finished_at.to_rfc3339(),
        manifest.event_count,
        manifest.pattern_count,
        if manifest.redacted { "是" } else { "否" }
    );

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

fn truncate(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let result: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{result}…")
    } else {
        result
    }
}
