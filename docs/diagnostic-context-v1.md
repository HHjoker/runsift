# RunSift diagnostic context protocol v1

`runsift.diagnostic-context` is the stable handoff between an evidence bundle
and a human, model, CI step, or agent tool. Version `1` is generated from
evidence schema version `2`.

## Invariants

- `run` and `context` perform no model or network call.
- `facts` are observations or deterministic transformations of bundle data.
- RunSift leaves `hypotheses` empty; inference belongs to the analyzer.
- `missing_information` records known evidence gaps and budget omissions.
- Every `finding` and model-generated `hypothesis` must cite at least one ID
  from `response_contract.allowed_evidence_ids`.
- Selected evidence keeps its source artifact or byte range so a consumer can
  trace a claim back to the bundle.
- `budget.estimated_tokens` is a deterministic approximation and never claims
  to match a provider tokenizer.

## Context shape

```json
{
  "protocol": "runsift.diagnostic-context",
  "protocol_version": 1,
  "generated_at": "2026-08-02T10:00:00Z",
  "source": {
    "bundle": ".runsift/runs/run_demo",
    "evidence_schema_version": 2,
    "run_id": "run_demo"
  },
  "budget": {
    "max_tokens": 8000,
    "estimated_tokens": 2600,
    "estimator": "runsift-char-v1 (approximate, provider-independent)",
    "candidate_count": 12,
    "selected_count": 8,
    "omitted_count": 4
  },
  "run": {},
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

The `response_contract.schema` field contains the full JSON Schema for the
expected analysis result. Consumers should use that embedded schema instead of
copying this abbreviated example.

## Analysis envelope

After an explicit `runsift analyze` call, RunSift parses the adapter response,
validates its schema version and citations, then writes:

```json
{
  "protocol": "runsift.analysis",
  "protocol_version": 1,
  "generated_at": "2026-08-02T10:01:00Z",
  "source_run_id": "run_demo",
  "adapter": "local:ollama",
  "analysis": {
    "schema_version": 1,
    "summary": "The run failed while parsing a record.",
    "findings": [
      {
        "title": "Record validation failed",
        "explanation": "The error event reports an invalid record length.",
        "severity": "error",
        "evidence_ids": ["evt_3adf68923eead391"]
      }
    ],
    "hypotheses": [],
    "missing_information": ["The original input bytes were not captured."]
  }
}
```

An adapter response that cites an ID outside the allowed set is rejected and
is not written as a validated analysis.
