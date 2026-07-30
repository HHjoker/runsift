use std::path::{Path, PathBuf};
use std::process::Command;

use crate::model::GitInfo;

pub fn inspect(cwd: &Path) -> Option<GitInfo> {
    let root = command(cwd, &["rev-parse", "--show-toplevel"])?;
    let root = PathBuf::from(root.trim());
    let commit = command(&root, &["rev-parse", "HEAD"]).map(|value| value.trim().to_owned());
    let branch = command(&root, &["branch", "--show-current"])
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    let status = command(&root, &["status", "--porcelain=v1"])?;
    let changed_files = status
        .lines()
        .filter_map(|line| line.get(3..))
        .map(str::to_owned)
        .collect::<Vec<_>>();

    Some(GitInfo {
        root,
        commit,
        branch,
        dirty: !changed_files.is_empty(),
        changed_files,
    })
}

fn command(cwd: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .ok()?;
    output.status.success().then(|| {
        String::from_utf8_lossy(&output.stdout)
            .trim_end()
            .to_owned()
    })
}
