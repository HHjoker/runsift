# LogLens

English | [简体中文](README.zh-CN.md)

> Turn a failed run into a compact, traceable engineering evidence bundle for
> developers and AI.

LogLens is a local-first, model-agnostic diagnostic context tool. It wraps a
test or program command, captures its output and selected logs, then turns
large amounts of unstructured text into structured events, recurring patterns,
and a readable summary.

LogLens does not call an AI model or modify code. It first solves the more
fundamental problem: giving humans and downstream AI complete, concise, and
verifiable evidence of what happened during a failed run.

> [!IMPORTANT]
> LogLens is at an early stage. Evidence schemas and command-line options may
> change before `1.0`.

## Why LogLens?

After a test or program fails, developers often face:

- thousands of lines across stdout, stderr, and application logs;
- repeated errors whose dynamic parameters make them look unrelated;
- test output, application logs, and source revisions stored separately;
- credentials or private data embedded in logs;
- AI that can read the code but cannot see what happened at runtime;
- conclusions that cannot be traced back to exact source evidence.

LogLens does not replace CTest, spdlog, an observability platform, or an AI
assistant. It connects them:

```text
CTest / executable
├── stdout / stderr
├── spdlog files
├── Git revision and working tree
└── exit status
          │
          ▼
       LogLens
├── incremental collection
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

## Implemented in phase one

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

These features are useful without an AI model. LogLens can be used purely as a
failed-run organizer and log compactor.

## Quick start

### Requirements

- Rust 1.85 or newer (Edition 2024)
- Linux, macOS, or another platform supported by the current dependencies
- Git is optional; LogLens also works outside a Git repository

### Build

```bash
git clone <your-loglens-repository>
cd loglens
cargo build --release
```

The executable is generated at:

```text
target/release/loglens
```

### Capture a CTest run

Assume the C++ application writes spdlog output to
`build/logs/application.log`:

```bash
touch build/logs/application.log

./target/release/loglens run \
  --log build/logs/application.log \
  --output .loglens/runs \
  -- \
  ctest --test-dir build --output-on-failure
```

Everything after `--` is the original command. If CTest returns exit code `8`,
LogLens writes the evidence bundle and also returns `8`, making it safe to use
inside CI without hiding failures.

### Capture a regular executable

```bash
./target/release/loglens run \
  --log ./logs/service.log \
  -- \
  ./build/bin/service --config ./config/test.yaml
```

Without `--log`, LogLens still captures stdout, stderr, the exit status, and
Git context:

```bash
./target/release/loglens run -- ./build/bin/unit_tests
```

### Capture multiple log files

```bash
./target/release/loglens run \
  --log ./logs/parser.log \
  --log ./logs/statistics.log \
  --log ./logs/error.log \
  -- \
  ctest --test-dir build --output-on-failure
```

## Evidence bundle

Each run creates an isolated directory:

```text
.loglens/runs/
└── run_<UTC-time>_<process-id>/
    ├── manifest.json
    ├── summary.md
    ├── events.jsonl
    ├── patterns.json
    ├── stdout.log
    ├── stderr.log
    └── logs/
        ├── 000-application.log
        └── 001-error.log
```

### `manifest.json`

Contains deterministic run metadata:

- command and arguments;
- start and finish times;
- exit code;
- working directory;
- Git commit, branch, and working-tree state;
- log sizes before and after collection;
- generated artifact names.

### `events.jsonl`

Contains one structured event per line:

```json
{
  "event_id": "evt_3adf68923eead391",
  "timestamp": "2026-07-30T02:00:01Z",
  "severity": "error",
  "source": "/var/log/application.log",
  "thread_id": "17",
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

- success or failure status;
- command and exit code;
- Git context;
- high-signal event patterns;
- representative evidence IDs;
- evidence-tracing guidance.

## spdlog compatibility

LogLens currently uses heuristic parsing for common spdlog-style records:

```text
[2026-07-30T10:00:00+08:00] [error] [thread 17] parse failed
  at parser.cpp:42
```

For best results, include at least:

```text
timestamp | level | thread ID | module or logger | message
```

Timestamps with an explicit timezone are normalized to UTC. Timestamps without
a timezone remain in the original message; LogLens does not guess their
timezone.

Explicit parsing configuration and project-specific format profiles are
planned.

## Security and privacy

Generated bundles apply basic redaction by default for:

- `Authorization: Bearer ...`
- JWTs
- `api_key`
- `access_token` / `auth_token`
- `password` / `passwd`
- `secret`

Redaction can be disabled for a trusted, local-only workflow:

```bash
loglens run --no-redact -- ./build/bin/unit_tests
```

Review the bundle before uploading or sharing it. Pattern-based redaction
cannot guarantee removal of every project-specific secret.

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

The current version collects and organizes facts. It does not claim that it has
identified the root cause.

### Derived data never replaces evidence

Patterns and Markdown summaries are derived views. Important findings should
remain traceable to original events.

### Low intrusion

Phase one works through an external command wrapper and file reads. It does not
require replacing spdlog or modifying C++ business logic.

## Current limitations

- Only bytes appended while the wrapped command runs are collected.
- A shrinking file is treated as truncation or rotation and read from byte
  zero.
- Renamed rotation files are not discovered automatically.
- CTest and JUnit do not yet have dedicated structured parsers.
- Project-specific log parsing is not yet configurable.
- Core dumps and ASan, UBSan, or TSan output do not yet have dedicated fields.
- Full source code and Git diffs are not copied into the bundle.
- LogLens does not call AI, produce root-cause claims, or modify code.
- Collection and aggregation are primarily in-memory and are not intended for
  unbounded log streams yet.

## Roadmap

### Phase one: local evidence bundles — current

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

- [ ] Structured CTest and GoogleTest results
- [ ] Configurable spdlog profiles
- [ ] ASan, UBSan, and TSan parsers
- [ ] GDB/LLDB and core-dump metadata
- [ ] Context correlation using `run_id`, `test_id`, and `batch_id`
- [ ] More reliable log-rotation tracking

### Phase three: AI context gateway

- [ ] Evidence selection under a token budget
- [ ] Explicit separation of facts, hypotheses, and missing information
- [ ] Stable diagnostic context protocol for AI
- [ ] Local-model and OpenAI-compatible adapters
- [ ] MCP or equivalent tool interface
- [ ] Required evidence citations in analysis results

### Phase four: general ecosystem

- [ ] JUnit, pytest, and cargo test adapters
- [ ] CI integrations
- [ ] Offline field-log import
- [ ] OpenTelemetry compatibility
- [ ] Pluggable inputs, parsers, and outputs

The roadmap communicates direction, not compatibility or delivery commitments.

## Development

```bash
cargo fmt -- --check
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release
```

The repository includes a simulated failed run:

```bash
mkdir -p /tmp/loglens-demo

./target/release/loglens run \
  --log /tmp/loglens-demo/application.log \
  --output /tmp/loglens-demo/runs \
  -- \
  sh examples/demo_failure.sh /tmp/loglens-demo/application.log
```

The example deliberately returns a non-zero exit code to verify that LogLens
does not mask a failed test.

## Contributing

LogLens is at an early stage. The following contributions are particularly
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

LogLens is licensed under the [Apache License 2.0](LICENSE).
