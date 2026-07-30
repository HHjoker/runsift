use std::fs;
use std::io::Read;
use std::path::Path;
use std::sync::LazyLock;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use regex::Regex;

use crate::diagnostics::parse_stack_frames;
use crate::logs::stable_id;
use crate::model::{CoreDump, CorrelationContext, DebuggerKind, DebuggerReport};
use crate::redact;

static SIGNAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?im)(?:Program received signal|stop reason\s*=\s*signal)\s*[: ]*\s*(?P<signal>SIG[A-Z0-9]+)",
    )
    .unwrap()
});

pub struct ImportedDebuggerReport {
    pub content: String,
    pub report: DebuggerReport,
}

pub fn inspect_core(path: &Path) -> Result<CoreDump> {
    let metadata = path
        .metadata()
        .with_context(|| format!("failed to inspect core dump {}", path.display()))?;
    let modified_at = metadata.modified().ok().map(DateTime::<Utc>::from);
    let format = detect_binary_format(path).unwrap_or_else(|_| "unknown".to_owned());
    let core_id = stable_id(
        "core",
        &format!("{}:{}:{modified_at:?}", path.display(), metadata.len()),
    );
    Ok(CoreDump {
        core_id,
        path: path.to_path_buf(),
        size: metadata.len(),
        modified_at,
        format,
    })
}

pub fn import_debugger_report(
    path: &Path,
    artifact: String,
    context: &CorrelationContext,
    redact_enabled: bool,
) -> Result<ImportedDebuggerReport> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read debugger report {}", path.display()))?;
    let content = redact::text(&raw, redact_enabled);
    let debugger = detect_debugger(&content);
    let signal = SIGNAL
        .captures(&content)
        .and_then(|captures| captures.name("signal"))
        .map(|value| value.as_str().to_owned());
    let report_id = stable_id(
        "debug",
        &format!("{}:{debugger:?}:{signal:?}:{content}", path.display()),
    );

    Ok(ImportedDebuggerReport {
        report: DebuggerReport {
            report_id,
            context: context.clone(),
            source_path: path.to_path_buf(),
            artifact,
            debugger,
            signal,
            stack_frames: parse_stack_frames(&content),
        },
        content,
    })
}

fn detect_debugger(content: &str) -> DebuggerKind {
    if content.contains("GNU gdb")
        || content.contains("Program received signal")
        || content.lines().any(|line| line.starts_with("#0"))
    {
        DebuggerKind::Gdb
    } else if content.contains("(lldb)")
        || content.contains("stop reason =")
        || content
            .lines()
            .any(|line| line.trim_start().starts_with("frame #"))
    {
        DebuggerKind::Lldb
    } else {
        DebuggerKind::Unknown
    }
}

fn detect_binary_format(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut magic = [0_u8; 4];
    let count = file.read(&mut magic)?;
    let value = if count < 4 {
        "unknown"
    } else if magic == [0x7f, b'E', b'L', b'F'] {
        "elf"
    } else if magic == *b"MDMP" {
        "minidump"
    } else if matches!(
        u32::from_be_bytes(magic),
        0xfeedface | 0xfeedfacf | 0xcefaedfe | 0xcffaedfe
    ) {
        "mach-o"
    } else {
        "unknown"
    };
    Ok(value.to_owned())
}

pub fn debugger_artifact(index: usize, path: &Path) -> String {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("debugger.txt")
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    format!("debugger/{index:03}-{name}")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{detect_binary_format, detect_debugger, import_debugger_report};
    use crate::model::{CorrelationContext, DebuggerKind};

    #[test]
    fn detects_lldb_report() {
        assert_eq!(
            detect_debugger(
                "Process 10 stopped\n* thread #1, stop reason = signal SIGSEGV\n  frame #0: 0x1 app`parse"
            ),
            DebuggerKind::Lldb
        );
    }

    #[test]
    fn detects_elf_magic() {
        let path = std::env::temp_dir().join(format!("runsift-core-{}", std::process::id()));
        fs::write(&path, [0x7f, b'E', b'L', b'F']).unwrap();
        assert_eq!(detect_binary_format(&path).unwrap(), "elf");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn imports_gdb_signal_and_frames() {
        let path = std::env::temp_dir().join(format!("runsift-gdb-report-{}", std::process::id()));
        fs::write(
            &path,
            "GNU gdb\nProgram received signal SIGSEGV\n#0  0x1000 in parse at /work/parser.cpp:42\npassword=hunter2\n",
        )
        .unwrap();
        let imported = import_debugger_report(
            &path,
            "debugger/000-gdb.txt".to_owned(),
            &CorrelationContext {
                run_id: "run_test".to_owned(),
                ..Default::default()
            },
            true,
        )
        .unwrap();
        let _ = fs::remove_file(path);

        assert_eq!(imported.report.debugger, DebuggerKind::Gdb);
        assert_eq!(imported.report.signal.as_deref(), Some("SIGSEGV"));
        assert_eq!(imported.report.stack_frames[0].line, Some(42));
        assert!(imported.content.contains("password=<redacted>"));
    }
}
