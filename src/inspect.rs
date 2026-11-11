use std::collections::VecDeque;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;

use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

use crate::cli::{InspectArgs, InspectQuery};
use crate::model::{Event, Manifest};

pub fn run(args: InspectArgs) -> Result<i32> {
    print_json(&query(&args.bundle, args.query)?)?;
    Ok(0)
}

pub(crate) fn query(bundle: &Path, query: InspectQuery) -> Result<Value> {
    let manifest: Manifest = serde_json::from_reader(open(bundle, "manifest.json")?)?;
    ensure!(
        matches!(manifest.schema_version, 2 | 3),
        "unsupported evidence schema {}",
        manifest.schema_version
    );

    Ok(match query {
        InspectQuery::Overview => json!({ "manifest": manifest }),
        InspectQuery::CheckBatch { batch_id, profile } => {
            crate::batch::check(events(bundle)?, &batch_id, &profile)?
        }
        InspectQuery::Search { text, limit } => search(bundle, &text, limit as usize)?,
        InspectQuery::Evidence { event_id, around } => {
            evidence(bundle, &event_id, around as usize, &manifest)?
        }
    })
}

pub(crate) fn print_json(result: &Value) -> Result<()> {
    let mut stdout = io::stdout().lock();
    serde_json::to_writer(&mut stdout, result)?;
    writeln!(stdout)?;
    Ok(())
}

fn open(bundle: &Path, name: &str) -> Result<BufReader<File>> {
    let path = bundle.join(name);
    File::open(&path)
        .map(BufReader::new)
        .with_context(|| format!("failed to open {}", path.display()))
}

fn events(bundle: &Path) -> Result<impl Iterator<Item = Result<Event>>> {
    Ok(open(bundle, "events.jsonl")?
        .lines()
        .enumerate()
        .filter_map(|(index, line)| match line {
            Ok(line) if line.trim().is_empty() => None,
            line => Some((|| {
                serde_json::from_str(&line?)
                    .with_context(|| format!("invalid events.jsonl line {}", index + 1))
            })()),
        }))
}

fn search(bundle: &Path, text: &str, limit: usize) -> Result<Value> {
    ensure!(!text.trim().is_empty(), "search text cannot be empty");
    let mut matches = Vec::new();
    for event in events(bundle)? {
        let event = event?;
        if event.message.contains(text) {
            if matches.len() == limit {
                return Ok(json!({ "events": matches, "truncated": true }));
            }
            matches.push(event);
        }
    }
    Ok(json!({ "events": matches, "truncated": false }))
}

fn evidence(bundle: &Path, event_id: &str, around: usize, manifest: &Manifest) -> Result<Value> {
    let mut events = events(bundle)?;
    let target = events
        .find(|event| {
            event.as_ref().is_err() || event.as_ref().is_ok_and(|e| e.event_id == event_id)
        })
        .transpose()?
        .with_context(|| format!("event {event_id} was not found"))?;

    let same_artifact = |event: &Event| event.evidence.artifact == target.evidence.artifact;
    let mut before = VecDeque::with_capacity(around);
    let mut after = Vec::new();
    if around > 0 {
        for event in self::events(bundle)? {
            let event = event?;
            if event.event_id == event_id {
                break;
            }
            if same_artifact(&event) {
                if before.len() == around {
                    before.pop_front();
                }
                before.push_back(event);
            }
        }
        for event in events {
            let event = event?;
            if same_artifact(&event) {
                after.push(event);
                if after.len() == around {
                    break;
                }
            }
        }
    }
    let artifact_excerpt = artifact_excerpt(bundle, &target, manifest)?;
    Ok(json!({
        "event": target,
        "before": before,
        "after": after,
        "representation": "parsed_event",
        "offset_basis": "original_source",
        "artifact_excerpt": artifact_excerpt
    }))
}

fn artifact_excerpt(bundle: &Path, event: &Event, manifest: &Manifest) -> Result<Option<Value>> {
    let Some([start, end]) = event.evidence.artifact_range else {
        return Ok(None);
    };
    let name = &event.evidence.artifact;
    ensure!(
        manifest.artifacts.contains(name),
        "unlisted evidence artifact"
    );
    ensure!(
        Path::new(name)
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_))),
        "invalid evidence artifact path"
    );
    let root = bundle.canonicalize()?;
    let path = root.join(name).canonicalize()?;
    ensure!(
        path.starts_with(&root),
        "evidence artifact is outside the bundle"
    );
    ensure!(path.is_file(), "evidence artifact is not a regular file");
    let mut file = File::open(path)?;
    ensure!(
        start <= end && end <= file.metadata()?.len(),
        "invalid evidence artifact range"
    );
    let length = end - start;
    let mut bytes = vec![0; length.min(16 * 1024) as usize];
    file.seek(SeekFrom::Start(start))?;
    file.read_exact(&mut bytes)?;
    Ok(Some(json!({
        "text": String::from_utf8_lossy(&bytes),
        "redacted": manifest.redacted,
        "truncated": length > bytes.len() as u64
    })))
}
