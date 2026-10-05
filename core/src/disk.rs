use std::io::{ErrorKind, Result};
use std::path::Path;

/// Deletes a file, or a directory with everything in it; nothing being there counts as done.
pub async fn delete(path: &Path) -> Result<()> {
    let deleted = match tokio::fs::symlink_metadata(path).await {
        Ok(found) if found.is_dir() => tokio::fs::remove_dir_all(path).await,
        Ok(_) => tokio::fs::remove_file(path).await,
        Err(e) => Err(e),
    };
    match deleted {
        Err(e) if e.kind() != ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn deletes_files_and_whole_directories() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("marker");
        let tree = dir.path().join("tree");
        std::fs::write(&file, "x").unwrap();
        std::fs::create_dir_all(tree.join("nested")).unwrap();
        std::fs::write(tree.join("nested/file"), "x").unwrap();

        delete(&file).await.unwrap();
        delete(&tree).await.unwrap();

        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn nothing_to_delete_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        delete(&dir.path().join("absent")).await.unwrap();
    }
}
