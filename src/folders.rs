//! The folders inside the output folder, which the LLM picks from to file a letter.

use std::{
    fs, io,
    path::Path,
};

/// Every folder under `root`, at any depth, as a path relative to `root` with `/`
/// between the parts, sorted: for example `["Gesundheit", "Gesundheit/teeth"]`.
/// Folders whose name starts with `.` (hidden ones) are skipped, with everything inside.
/// A missing `root` gives an empty list.
pub fn scan_folders(root: &Path) -> io::Result<Vec<String>> {
    let mut found = Vec::new();
    if root.is_dir() {
        collect_folders(root, "", &mut found)?;
    }
    found.sort();
    Ok(found)
}

/// Adds the folders in `dir` to `found`, then the folders inside each of them.
/// `prefix` is `dir`'s own relative path ("" for the root), put in front of each name.
/// A function that calls itself like this ("recursion") walks a tree of any depth.
fn collect_folders(dir: &Path, prefix: &str, found: &mut Vec<String>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !entry.file_type()?.is_dir() || name.starts_with('.') {
            continue;
        }
        let relative = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        collect_folders(&entry.path(), &relative, found)?;
        found.push(relative); // after the call: `relative` was only lent to it
    }
    Ok(())
}

// Small file, so the tests stay here.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::test_folder;

    #[test]
    fn test_scan_folders_lists_nested_folders_as_relative_paths() {
        let root = test_folder("scan_nested");
        fs::create_dir_all(root.join("Gesundheit/teeth")).unwrap();
        fs::create_dir_all(root.join("Finanzamt")).unwrap();

        let folders = scan_folders(&root).unwrap();

        assert_eq!(folders, ["Finanzamt", "Gesundheit", "Gesundheit/teeth"]);
    }

    #[test]
    fn test_scan_folders_skips_files_and_hidden_folders() {
        let root = test_folder("scan_skips");
        fs::create_dir_all(root.join("Musik")).unwrap();
        fs::create_dir_all(root.join(".hidden/inside")).unwrap();
        fs::write(root.join("Brief.pdf"), "").unwrap();
        fs::write(root.join(".DS_Store"), "").unwrap();

        assert_eq!(scan_folders(&root).unwrap(), ["Musik"]);
    }

    #[test]
    fn test_scan_folders_missing_root_is_empty() {
        let root = test_folder("scan_missing").join("no_such_folder");
        assert!(scan_folders(&root).unwrap().is_empty());
    }
}
