# RunSift

English | [简体中文](README.zh-CN.md)

> Turn a live failure or historical log set into compact, traceable engineering
> evidence for developers and AI.

RunSift is a local-first, model-agnostic diagnostic context tool. It can wrap a
live test or program command, or import complete historical logs when a problem
cannot be reproduced. Both paths turn unstructured text into structured events,
high-signal patterns, traceable evidence, and a readable summary.

The `run`, `import`, and `context` commands do not call an AI model or modify
source evidence. Model access happens only through an explicit `analyze`
command and adapter. This keeps the fundamental problem separate: giving humans
and downstream AI concise, verifiable evidence of what happened.

> [!IMPORTANT]
> RunSift is at an early stage. Evidence schemas and command-line options may
> change before `1.0`. The Rust library API is also experimental. RunSift
> `0.4` writes evidence schema version `3` and diagnostic-context protocol
> version `2`. Existing schema version `2` bundles remain readable.

## Why RunSift?

After a test or program fails, developers often face:

- thousands of lines across stdout, stderr, and application logs;
- repeated errors whose dynamic parameters make them look unrelated;
- test output, application logs, and source revisions stored separately;
- credentials or private data embedded in logs;
- AI that can read the code but cannot see what happened at runtime;
- conclusions that cannot be traced back to exact source evidence.

RunSift does not replace CTest, spdlog, an observability platform, or an AI
assistant. It connects them:

```text
live command                 historical incident
├── stdout / stderr          ├── one or more log files
├── spdlog files             ├── directories
├── Git revision             ├── test/debugger reports
└── exit status              └── core metadata
          │
          ▼
       RunSift
├── live capture or full-file import
├── event parsing
├── multiline reconstruction
├── secret redaction
├── dynamic-field normalization
├── recurring-pattern aggregation
└── evidence references
          │
          ▼
local evidence bundle
├── human review
├── AI analysis
├── CI follow-up steps
└── other analysis tools
```

## Implemented

| Capability | Current behavior |
|---|---|
| Command wrapper | Runs any command and preserves its exit code |
| Process output | Streams and captures stdout and stderr |
| Incremental logs | Collects only bytes appended while the command runs |
| spdlog parsing | Detects common timestamps, levels, and thread IDs |
| Multiline events | Joins indented lines and common stack-trace forms |
| Pattern aggregation | Normalizes timestamps, numbers, addresses, UUIDs, and IPs |
| Evidence tracing | Adds stable IDs, original paths, and byte offsets |
| Default redaction | Redacts bearer tokens, JWTs, API keys, passwords, and secrets |
| Git context | Records repository, commit, branch, and changed files |
| Dual output | Generates JSON/JSONL for tools and Markdown for humans |
| CI compatibility | Returns the wrapped command's exit code |

These features are useful without an AI model. RunSift can be used purely as a
failed-run organizer and log compactor.

Historical evidence import is a first-class entry point:

| Capability | Current behavior |
|---|---|
| Complete-file import | Reads all bytes from existing logs without rerunning the program |
| Files and directories | Imports multiple paths, immediate directory files, or nested files with `--recursive` |
| Source integrity | Records original path, size, modification time, SHA-256, and byte-addressable evidence |
| Key information | Highlights WARN+, sanitizer output, and failure-related keywords even without an explicit level |
| Developer summary | Produces source inventory, time range, severity counts, key-event timeline, and recurring patterns |
| Case correlation | Groups historical evidence under a stable `case_id` without inventing a command or exit status |
| Atomic output | Publishes a bundle only after every requested input is processed successfully |

Phase two adds C++-specific engineering context:

| Capability | Current behavior |
|---|---|
| CTest and GoogleTest | Imports JUnit XML into stable test-case records |
| spdlog profiles | Parses project formats through a named-capture JSON profile |
| Sanitizers | Extracts ASan, UBSan, and TSan findings and stack frames |
| Crash context | Records core metadata and imports GDB/LLDB text reports |
| Correlation | Propagates `run_id`, `batch_id`, and `test_id` into evidence |
| Log rotation | Uses file identity on Unix-like systems to recover both sides of rename-based rotation |

Phase three turns that evidence into a controlled AI handoff:

| Capability | Current behavior |
|---|---|
| Token budget | Selects higher-priority evidence under a deterministic approximate budget |
| Context protocol | Separates facts, hypotheses, missing information, evidence, and response rules |
| Evidence citations | Rejects model findings and hypotheses that cite unavailable evidence IDs |
| Local adapter | Sends the generated prompt to an explicit local process over stdin |
| OpenAI-compatible adapter | Supports Responses by default and Chat Completions as a compatibility mode |
| Tool integration | Emits context as JSON on stdout for agents, CI, and custom tools |

## Quick start

### Requirements

- Rust 1.85 or newer (Edition 2024)
- Linux, macOS, or another platform supported by the current dependencies
- Git is optional; RunSift also works outside a Git repository

### Build

```bash
git clone https://github.com/HHjoker/runsift.git
cd runsift
cargo build --release
```

The executable is generated at:

```text
target/release/runsift
```

### Import historical logs

Import one log that came back from a field incident:

```bash
./target/release/runsift import \
  --case-id field-4821 \
  /path/to/application.log
```

The original file is read-only. RunSift writes a new bundle under
`.runsift/cases/field-4821/` and prints the path to its `summary.md`.

Import several logs or a directory tree:

```bash
./target/release/runsift import \
  --case-id customer-crash-4821 \
  --recursive \
  ./field-logs/ ./gateway.log
```

Attach related evidence when available:

```bash
./target/release/runsift import application.log \
  --test-report gtest-results.xml \
  --debugger-report gdb-backtrace.txt \
  --core core.1234
```

Read the developer view, then optionally build local AI context:

```bash
less .runsift/cases/field-4821/summary.md

./target/release/runsift context \
  .runsift/cases/field-4821 \
  --token-budget 8000
```

### Capture a CTest run

Assume the C++ application writes spdlog output to
`build/logs/application.log`:

```bash
touch build/logs/application.log

./target/release/runsift run \
  --log build/logs/application.log \
  --output .runsift/runs \
  -- \
  ctest --test-dir build --output-on-failure
```

Everything after `--` is the original command. If CTest returns exit code `8`,
RunSift writes the evidence bundle and also returns `8`, making it safe to use
inside CI without hiding failures.

### Capture a regular executable

```bash
./target/release/runsift run \
  --log ./logs/service.log \
  -- \
  ./build/bin/service --config ./config/test.yaml
```

Without `--log`, RunSift still captures stdout, stderr, the exit status, and
Git context:

```bash
./target/release/runsift run -- ./build/bin/unit_tests
```

### Capture multiple log files

```bash
./target/release/runsift run \
  --log ./logs/parser.log \
  --log ./logs/statistics.log \
  --log ./logs/error.log \
  -- \
  ctest --test-dir build --output-on-failure
```

### Capture structured C++ test and runtime evidence

CTest can generate a JUnit XML report while RunSift captures the same run:

```bash
./target/release/runsift run \
  --test-report build/ctest-results.xml \
  --batch-id ci-4821 \
  -- \
  ctest --test-dir build \
    --output-on-failure \
    --output-junit build/ctest-results.xml
```

GoogleTest works with its XML output:

```bash
./target/release/runsift run \
  --test-report build/gtest-results.xml \
  --test-id parser-suite \
  -- \
  ./build/parser_tests --gtest_output=xml:build/gtest-results.xml
```

ASan, UBSan, and TSan findings written to stderr are detected automatically.
Existing core dumps and debugger reports can be attached with `--core` and
`--debugger-report`:

```bash
runsift run \
  --core build/core.1234 \
  --debugger-report build/lldb-backtrace.txt \
  -- \
  ./build/parser_tests
```

RunSift records core metadata but does not copy a potentially large core file
or execute a debugger. The supplied GDB/LLDB text report is redacted and copied
into the bundle.

### Build AI-ready context locally

After `run` or `import` creates a bundle, select its highest-signal evidence
under an approximate token budget:

```bash
runsift context .runsift/runs/<run_id> --token-budget 8000
```

For a historical case, pass `.runsift/cases/<case_id>` instead.

This writes `ai/context.json` and `ai/prompt.md`. It is local-only and does not
contact a model. For a machine-readable tool interface:

```bash
runsift context .runsift/runs/<run_id> --stdout
```

The context contains observed facts, an intentionally empty hypothesis list,
known evidence gaps, selected evidence, and the exact response schema. See the
[diagnostic context protocol](docs/diagnostic-context-v2.md).

### Analyze through an explicit adapter

Use any local command that reads a prompt from stdin and returns RunSift
analysis JSON on stdout:

```bash
runsift analyze .runsift/runs/<run_id> local -- \
  ollama run qwen3
```

Or use an OpenAI-compatible endpoint:

```bash
export OPENAI_API_KEY="..."

runsift analyze .runsift/runs/<run_id> openai \
  --model <MODEL> \
  --api-key-env OPENAI_API_KEY
```

The Responses API shape is the default. Use `--api chat-completions` for a
compatible server that only implements that endpoint. RunSift validates the
returned JSON and refuses to write findings or hypotheses without citations to
evidence selected into the context.

## Evidence bundle

Live runs and historical cases use separate parent directories but share the
same downstream evidence interface:

```text
.runsift/runs/
└── run_<UTC-time>_<process-id>/
    ├── manifest.json
    ├── summary.md
    ├── events.jsonl
    ├── patterns.json
    ├── tests.json
    ├── diagnostics.json
    ├── crash.json
    ├── stdout.log
    ├── stderr.log
    ├── ai/
    │   ├── context.json
    │   ├── prompt.md
    │   └── analysis.json
    ├── debugger/
    │   └── 000-lldb-backtrace.txt
    ├── tests/
    │   └── 000-ctest-results.xml
    └── logs/
        ├── 000-application.log
        └── 001-error.log

.runsift/cases/
└── case_<UTC-time>_<process-id>/
    ├── manifest.json
    ├── summary.md
    ├── events.jsonl
    ├── patterns.json
    ├── tests.json
    ├── diagnostics.json
    ├── crash.json
    └── logs/
        ├── 000-application.log
        └── 001-application.log.1
```

### `manifest.json`

Contains deterministic capture or import metadata:

- `capture_mode` (`live` or `import`);
- `run_id` for live evidence or `case_id` for historical evidence;
- optional command, arguments, and exit code when a live command was captured;
- processing and observed-log time ranges;
- working directory;
- optional Git commit, branch, and working-tree state for live capture;
- original source paths, sizes, modification times, SHA-256 values, and copied artifacts;
- log sizes before and after collection;
- structured test, sanitizer, core, and debugger counts;
- generated artifact names.

### `events.jsonl`

Contains one structured event per line:

```json
{
  "event_id": "evt_3adf68923eead391",
  "context": {
    "run_id": "run_ci_4821",
    "batch_id": "ci_4821",
    "test_id": "parser_suite"
  },
  "timestamp": "2026-07-30T02:00:01Z",
  "severity": "error",
  "source": "/var/log/application.log",
  "thread_id": "17",
  "logger": "parser",
  "message": "invalid record length 18 at offset 8192",
  "evidence": {
    "artifact": "logs/000-application.log",
    "source_path": "/var/log/application.log",
    "byte_start": 420,
    "byte_end": 527
  }
}
```

Downstream findings should cite `event_id` instead of returning unverifiable
natural-language claims.

### `patterns.json`

Groups events whose structures match even when dynamic values differ:

```text
invalid record length 18 at offset 8192
invalid record length 21 at offset 17664
```

becomes:

```text
invalid record length <num> at offset <num>
```

Each pattern retains its count, severity, first and last observation times, and
representative event IDs.

### `summary.md`

Provides a directly readable report containing:

- evidence type and source inventory;
- historical log time range when reliable timestamps exist;
- severity counts and high-signal event timeline;
- live command, exit status, and Git context when available;
- high-signal event patterns;
- representative evidence IDs;
- evidence-tracing guidance.

### C++ evidence files

- `tests.json` contains CTest/GoogleTest cases, status, duration, failure
  details, stable `test_id` values, and a reference to the redacted source XML
  copied under `tests/`.
- `diagnostics.json` contains structured ASan, UBSan, and TSan findings with
  stack frames and byte-addressable evidence.
- `crash.json` contains lightweight core-dump metadata and parsed GDB/LLDB
  reports.
- `ai/context.json` and `ai/prompt.md` contain the local, budgeted model handoff;
  `ai/analysis.json` is created only after an explicit adapter call and
  citation validation.

## spdlog compatibility

RunSift currently uses heuristic parsing for common spdlog-style records:

```text
[2026-07-30T10:00:00+08:00] [error] [thread 17] parse failed
  at parser.cpp:42
```

For best results, include at least:

```text
timestamp | level | thread ID | module or logger | message
```

Timestamps with an explicit timezone are normalized to UTC. Timestamps without
a timezone remain in the original message; RunSift does not guess their
timezone.

For a project-specific format, provide a JSON profile:

```bash
runsift run \
  --log build/logs/application.log \
  --log-profile examples/spdlog-profile.json \
  -- \
  ./build/parser_tests
```

The same profile can parse historical files:

```bash
runsift import --log-profile examples/spdlog-profile.json ./field-logs
```

The regular expression must define named `level` and `message` captures. It may
also define `timestamp`, `thread`, and `logger`. `timestamp_format` uses chrono
format syntax; a timezone is required when the timestamp itself has no offset.
See [`examples/spdlog-profile.json`](examples/spdlog-profile.json).

## Security and privacy

Generated bundles apply basic redaction by default for:

- `Authorization: Bearer ...`
- JWTs
- `api_key`
- `access_token` / `auth_token`
- `password` / `passwd`
- `secret`

The same redaction is applied to captured command arguments and is applied
again while building AI context, including when reading an older bundle.

Redaction can be disabled for a trusted, local-only workflow:

```bash
runsift run --no-redact -- ./build/bin/unit_tests
```

Review the bundle before uploading or sharing it. Pattern-based redaction
cannot guarantee removal of every project-specific secret.

`run`, `import`, and `context` never upload evidence. `import` does not modify
its source files and hashes the original bytes before writing redacted copies.
`analyze local` invokes only the provided process. `analyze openai` sends the
generated prompt to the configured base URL; API keys are read from the named
environment variable and are not stored in the bundle.

Original logs remain in their original locations. Evidence byte offsets refer
to those original files. If redaction changes text length, those offsets do not
refer to the redacted copies inside the bundle.

## Design principles

### Local first

Phase one requires no server, database, account, or cloud platform.

### Model agnostic

Bundles can be consumed by any AI, IDE, CI system, or custom analysis tool.
Core collection does not depend on a model provider.

### Facts before inference

RunSift-generated context keeps observed facts separate from model-generated
hypotheses and names missing information explicitly. A model may propose a root
cause, but its claims must cite selected evidence.

### Derived data never replaces evidence

Patterns and Markdown summaries are derived views. Important findings should
remain traceable to original events.

### Low intrusion

Phase one works through an external command wrapper and file reads. It does not
require replacing spdlog or modifying C++ business logic.

## Current limitations

- `run` collects only bytes appended while its wrapped command runs; use
  `import` to read complete existing files.
- Historical import currently accepts regular files and directories. Compressed
  `.gz`, `.zip`, and `.tar.gz` inputs are not yet expanded.
- A source file must remain stable while it is read; RunSift rejects files whose
  size or modification time changes during import.
- Imported text is processed in memory and is not yet intended for unbounded or
  multi-gigabyte log sets.
- Without explicit-timezone timestamps, RunSift preserves source and byte order
  but does not claim a reliable cross-file timeline.
- Rotation families are accepted as multiple files but are not yet inferred or
  reordered by filename when their records lack timestamps.
- A shrinking file is treated as truncation or rotation and read from byte
  zero.
- Rename-based rotation recovery uses file identity on Unix-like systems and
  scans only the watched file's immediate directory.
- Copy-truncate rotation can be detected, but bytes written and removed before
  final collection cannot be recovered.
- CTest support currently consumes its JUnit XML output, not legacy
  `Testing/*/Test.xml`.
- GDB/LLDB must be run separately; RunSift imports their text reports and does
  not execute debugger commands.
- Core files are described but intentionally not copied into the bundle.
- Full source code and Git diffs are not copied into the bundle.
- Token counts use a deterministic provider-independent estimate, not a model's
  exact tokenizer.
- The OpenAI adapter is deliberately small and does not implement retries,
  streaming, conversation state, or provider-specific tool calls.
- The JSON stdout interface is available; a dedicated MCP server is not yet
  implemented.
- RunSift validates evidence IDs and response shape, but cannot prove that a
  model's interpretation is logically correct.
- RunSift never modifies source code.
- Collection and aggregation are primarily in-memory and are not intended for
  unbounded log streams yet.

## Roadmap

### Phase one: local evidence bundles

- [x] Wrap arbitrary commands
- [x] Capture stdout, stderr, and exit status
- [x] Incrementally collect selected log files
- [x] Parse common spdlog records
- [x] Reconstruct multiline events
- [x] Aggregate recurring patterns
- [x] Apply basic redaction
- [x] Record Git metadata
- [x] Generate JSONL, JSON, and Markdown bundles

### Phase two: C++ engineering context

- [x] Structured CTest and GoogleTest results
- [x] Configurable spdlog profiles
- [x] ASan, UBSan, and TSan parsers
- [x] GDB/LLDB and core-dump metadata
- [x] Context correlation using `run_id`, `test_id`, and `batch_id`
- [x] More reliable log-rotation tracking

### Phase three: AI context gateway — current

- [x] Evidence selection under a token budget
- [x] Explicit separation of facts, hypotheses, and missing information
- [x] Stable diagnostic context protocol for AI
- [x] Local-model and OpenAI-compatible adapters
- [x] Machine-readable CLI tool interface (`context --stdout`)
- [x] Required evidence citations in analysis results
- [ ] Dedicated MCP server

### Phase four: historical incident evidence — current

- [x] Complete historical log-file import
- [x] Multi-file and recursive-directory input
- [x] Source SHA-256, metadata, artifact, and byte-offset provenance
- [x] Historical time range and high-signal developer summary
- [x] Historical evidence support in `context` and `analyze`
- [x] Optional test, debugger, and core evidence attachment
- [ ] Compressed archive input
- [ ] Explicit incident anchors and time windows
- [ ] Rotation-family inference without timestamps

### Phase five: general ecosystem

- [ ] JUnit, pytest, and cargo test adapters
- [ ] CI integrations
- [ ] OpenTelemetry compatibility
- [ ] Pluggable inputs, parsers, and outputs

The roadmap communicates direction, not compatibility or delivery commitments.

## Development

The crate keeps production code and tests separate:

```text
src/
├── lib.rs          # reusable collection and parsing library
├── main.rs         # thin CLI entry point
└── *.rs            # production modules
tests/
├── support/        # shared test utilities
└── *.rs            # black-box integration tests
```

```bash
cargo fmt -- --check
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release
```

The repository includes a simulated failed run:

```bash
mkdir -p /tmp/runsift-demo

./target/release/runsift run \
  --log /tmp/runsift-demo/application.log \
  --output /tmp/runsift-demo/runs \
  -- \
  sh examples/demo_failure.sh /tmp/runsift-demo/application.log
```

The example deliberately returns a non-zero exit code to verify that RunSift
does not mask a failed test.

The historical-import path can be exercised without running an application:

```bash
./target/release/runsift import \
  --case-id historical-demo \
  --output /tmp/runsift-cases \
  examples/historical_failure.log

less /tmp/runsift-cases/historical-demo/summary.md
```

Phase two has a C++-context demo with GoogleTest XML, a custom spdlog profile,
and a simulated ASan report:

```bash
mkdir -p /tmp/runsift-phase2
touch /tmp/runsift-phase2/application.log

./target/release/runsift run \
  --log /tmp/runsift-phase2/application.log \
  --log-profile examples/spdlog-profile.json \
  --test-report /tmp/runsift-phase2/gtest.xml \
  --output /tmp/runsift-phase2/runs \
  --batch-id demo-batch \
  --test-id parser-suite \
  -- \
  sh examples/demo_phase2_failure.sh \
    /tmp/runsift-phase2/application.log \
    /tmp/runsift-phase2/gtest.xml
```

Use the generated directory to exercise phase three without contacting a
model:

```bash
runsift context /tmp/runsift-phase2/runs/<run_id> --token-budget 8000
```

## Contributing

RunSift is at an early stage. The following contributions are particularly
valuable:

- anonymized failure logs from real projects;
- CTest, GoogleTest, and spdlog format samples;
- edge cases involving rotation, crashes, and concurrent output;
- feedback on the evidence schema and privacy policy;
- build and test results from different platforms;
- fixtures that verify important root-cause evidence survives compaction.

Before submitting logs, remove private data. When opening an issue, include:

1. the command that was run;
2. the expected behavior;
3. the actual behavior;
4. a minimal log sample;
5. the operating system and Rust version.

## License

RunSift is licensed under the [Apache License 2.0](LICENSE).
