# RunSift diagnostic context protocol v2

`runsift.diagnostic-context` version `2` is the stable handoff for both live
captures and historical incident imports. It is generated from evidence schema
version `2` or `3`; new RunSift `0.4` bundles use schema version `3`.

## Invariants

- `run`, `import`, and `context` perform no model or network call.
- Historical import never invents a command, exit status, Git revision, or
  process lifetime. Those fields are `null` and the gap is listed under
  `missing_information`.
- `facts` are observations or deterministic transformations of bundle data.
- RunSift leaves `hypotheses` empty; inference belongs to the analyzer.
- Every model finding and hypothesis must cite at least one selected evidence
  ID from `response_contract.allowed_evidence_ids`.
- Imported sources retain their original path, SHA-256, copied artifact, and
  original byte offsets.
- Token estimates are deterministic approximations, not provider token counts.

## Historical context shape

```json
{
  "protocol": "runsift.diagnostic-context",
  "protocol_version": 2,
  "generated_at": "2026-08-02T10:10:00Z",
  "source": {
    "bundle": ".runsift/cases/field-4821",
    "evidence_schema_version": 3,
    "correlation_id": "field-4821",
    "run_id": null,
    "capture_mode": "import",
    "case_id": "field-4821"
  },
  "budget": {
    "max_tokens": 8000,
    "estimated_tokens": 2700,
    "estimator": "runsift-char-v1 (approximate, provider-independent)",
    "candidate_count": 20,
    "selected_count": 12,
    "omitted_count": 8
  },
  "subject": {
    "capture_mode": "import",
    "case_id": "field-4821",
    "command": null,
    "success": null,
    "exit_code": null,
    "started_at": "2026-08-02T10:10:00Z",
    "finished_at": "2026-08-02T10:10:01Z",
    "observed_started_at": "2026-08-02T02:00:00Z",
    "observed_finished_at": "2026-08-02T02:05:00Z",
    "git_commit": null,
    "git_branch": null,
    "git_dirty": null
  },
  "facts": [],
  "hypotheses": [],
  "missing_information": [],
  "evidence": [],
  "response_contract": {
    "format": "json",
    "require_evidence_citations": true,
    "allowed_evidence_ids": [],
    "schema": {}
  }
}
```

For a live capture, `capture_mode` is `live`, `case_id` is `null`, and command,
exit status, runtime range, and available Git fields are populated.

The embedded `response_contract.schema` is authoritative for the expected model
result. RunSift validates the adapter response before writing
`ai/analysis.json`. The analysis envelope remains `runsift.analysis` version
`1`; its `source_run_id` contains the bundle correlation ID, which is the
`case_id` for historical imports.
