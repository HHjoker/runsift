use std::fs;

use runsift::crash;
use runsift::model::{CorrelationContext, DebuggerKind};

mod support;

fn context() -> CorrelationContext {
    CorrelationContext {
        run_id: "run_test".to_owned(),
        ..Default::default()
    }
}

#[test]
fn imports_lldb_report() {
    let directory = support::temp_directory("lldb");
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join("lldb.txt");
    fs::write(
        &path,
        "Process 10 stopped\n* thread #1, stop reason = signal SIGSEGV\n  frame #0: 0x1 app`parse\n",
    )
    .unwrap();
    let imported =
        crash::import_debugger_report(&path, "debugger/000-lldb.txt".to_owned(), &context(), true)
            .unwrap();
    let _ = fs::remove_dir_all(directory);

    assert_eq!(imported.report.debugger, DebuggerKind::Lldb);
    assert_eq!(imported.report.signal.as_deref(), Some("SIGSEGV"));
}

#[test]
fn detects_elf_core_metadata() {
    let directory = support::temp_directory("core");
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join("core");
    fs::write(&path, [0x7f, b'E', b'L', b'F']).unwrap();
    let core = crash::inspect_core(&path).unwrap();
    let _ = fs::remove_dir_all(directory);

    assert_eq!(core.format, "elf");
    assert_eq!(core.size, 4);
}

#[test]
fn imports_gdb_signal_and_frames() {
    let directory = support::temp_directory("gdb");
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join("gdb.txt");
    fs::write(
        &path,
        "GNU gdb\nProgram received signal SIGSEGV\n#0  0x1000 in parse at /work/parser.cpp:42\npassword=hunter2\n",
    )
    .unwrap();
    let imported =
        crash::import_debugger_report(&path, "debugger/000-gdb.txt".to_owned(), &context(), true)
            .unwrap();
    let _ = fs::remove_dir_all(directory);

    assert_eq!(imported.report.debugger, DebuggerKind::Gdb);
    assert_eq!(imported.report.signal.as_deref(), Some("SIGSEGV"));
    assert_eq!(imported.report.stack_frames[0].line, Some(42));
    assert!(imported.content.contains("password=<redacted>"));
}
