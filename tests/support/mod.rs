use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn temp_directory(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("runsift-{label}-{}-{nonce}", std::process::id()))
}
