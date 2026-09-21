use std::path::{Path, PathBuf};

/// Keep each invocation's source immutable until Python has opened it.
/// Agents may share a workspace and independently choose the same filename.
pub(crate) async fn save_script(
    root: &Path,
    name: &Path,
    content: &str,
) -> std::io::Result<PathBuf> {
    tokio::fs::create_dir_all(root).await?;
    let invocation = root.join(uuid::Uuid::new_v4().simple().to_string());
    tokio::fs::create_dir(&invocation).await?;
    let script = invocation.join(name);
    tokio::fs::write(&script, content).await?;
    Ok(script)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[tokio::test]
    async fn concurrent_scripts_with_same_filename_keep_their_own_source() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("ptc_temp");
        let results = futures::future::join_all((0..32).map(|index| {
            let root = root.clone();
            async move {
                let content = format!("print({index})");
                let path = save_script(&root, Path::new("script.py"), &content)
                    .await
                    .unwrap();
                (path, content)
            }
        }))
        .await;
        assert_eq!(
            results
                .iter()
                .map(|(path, _)| path)
                .collect::<HashSet<_>>()
                .len(),
            32
        );
        for (path, content) in results {
            assert_eq!(tokio::fs::read_to_string(path).await.unwrap(), content);
        }
    }
}
