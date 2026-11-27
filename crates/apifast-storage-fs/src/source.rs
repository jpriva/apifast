//! Where an import came from, as kept in `.apifast/import.yml`: the file's
//! path relative to the collection's folder, so it holds for anyone who
//! clones the collection with its source next to it.

use std::path::{Component, Path, PathBuf};

/// `file` relative to the collection at `root` (with `..` if needed), in
/// `/` form; absolute if they share no ancestor (other drives).
pub fn import_source(root: &Path, file: &Path) -> String {
    let absolute = |p: &Path| {
        dunce::canonicalize(p).unwrap_or_else(|_| {
            std::env::current_dir().map_or_else(|_| p.to_path_buf(), |dir| dir.join(p))
        })
    };
    let (root, file) = (absolute(root), absolute(file));
    let base: Vec<Component> = root.components().collect();
    let target: Vec<Component> = file.components().collect();
    let common = base.iter().zip(&target).take_while(|(a, b)| a == b).count();
    if common == 0 {
        return file.display().to_string();
    }
    let ups = std::iter::repeat_n("..".to_owned(), base.len() - common);
    let downs = target[common..]
        .iter()
        .map(|c| c.as_os_str().to_string_lossy().into_owned());
    ups.chain(downs).collect::<Vec<_>>().join("/")
}

/// The file an import came from, `source` as [`import_source`] wrote it.
pub fn import_source_path(root: &Path, source: &str) -> PathBuf {
    root.join(source)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_are_relative_to_the_collection() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("work/pets");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(dir.path().join("specs")).unwrap();
        let spec = dir.path().join("specs/pets.yaml");
        std::fs::write(&spec, "x").unwrap();
        let source = import_source(&root, &spec);
        assert_eq!(source, "../../specs/pets.yaml");
        let back = dunce::canonicalize(import_source_path(&root, &source)).unwrap();
        assert_eq!(back, dunce::canonicalize(&spec).unwrap());
        let inside = root.join("api.yaml");
        std::fs::write(&inside, "x").unwrap();
        assert_eq!(import_source(&root, &inside), "api.yaml");
    }
}
