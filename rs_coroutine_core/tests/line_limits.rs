use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};

const MAX_LINES: usize = 1000;
const IGNORED_DIRS: &[&str] = &[".git", "target"];

#[test]
fn all_rust_files_are_under_limit() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root should exist")
        .to_path_buf();

    let mut queue: VecDeque<PathBuf> = VecDeque::from([workspace_root]);
    while let Some(path) = queue.pop_front() {
        let Ok(entries) = fs::read_dir(&path) else {
            continue;
        };
        for entry in entries.flatten() {
            let entry_path = entry.path();
            if entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                if should_ignore(&entry_path) {
                    continue;
                }
                queue.push_back(entry_path);
            } else if entry_path.extension().is_some_and(|ext| ext == "rs") {
                let content = fs::read_to_string(&entry_path)
                    .unwrap_or_else(|_| panic!("failed reading {:?}", entry_path));
                let lines = content.lines().count();
                assert!(
                    lines <= MAX_LINES,
                    "File {:?} has {} lines which exceeds the {} line limit",
                    entry_path,
                    lines,
                    MAX_LINES
                );
            }
        }
    }
}

fn should_ignore(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| IGNORED_DIRS.contains(&name))
        .unwrap_or(false)
}
