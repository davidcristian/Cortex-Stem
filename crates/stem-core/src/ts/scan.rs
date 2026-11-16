//! The textual scans of the drift check: the types this crate's sources
//! annotate with `ts(export)`, and the binding files on disk.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Names of every `pub struct`/`pub enum` in this crate's sources that is
/// preceded by a `ts(export)` attribute. A textual scan, deliberately simple:
/// it fails loudly when the attribute is not followed by a public item.
pub(super) fn annotated_types() -> BTreeSet<String> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut names = BTreeSet::new();
    for file in rust_files(&src) {
        let text = std::fs::read_to_string(&file)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", file.display()));
        let lines: Vec<&str> = text.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            let line = line.trim_start();
            if !(line.starts_with("#[") && line.contains("ts(export)")) {
                continue;
            }
            let item = lines[i + 1..]
                .iter()
                .map(|l| l.trim_start())
                .find_map(|l| {
                    l.strip_prefix("pub struct ")
                        .or_else(|| l.strip_prefix("pub enum "))
                })
                .unwrap_or_else(|| {
                    panic!(
                        "{}:{}: `ts(export)` is not followed by a `pub struct`/`pub enum`",
                        file.display(),
                        i + 1
                    )
                });
            let name: String = item
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            assert!(
                !name.is_empty(),
                "{}:{}: unnamed item",
                file.display(),
                i + 1
            );
            names.insert(name);
        }
    }
    names
}

fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let entries =
        std::fs::read_dir(dir).unwrap_or_else(|e| panic!("cannot list {}: {e}", dir.display()));
    for entry in entries {
        let path = entry.expect("directory entry").path();
        if path.is_dir() {
            files.extend(rust_files(&path));
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
    files.sort();
    files
}

/// `*.ts` file names present in the export directory.
pub(super) fn binding_files(dir: &Path) -> BTreeSet<PathBuf> {
    std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot list {}: {e}", dir.display()))
        .map(|entry| entry.expect("directory entry").path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|ext| ext == "ts"))
        .filter_map(|p| p.file_name().map(PathBuf::from))
        .collect()
}
