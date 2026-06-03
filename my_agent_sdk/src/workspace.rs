use std::path::{Component, Path, PathBuf};
use tokio::fs;

/// A sandboxed Workspace that restricts filesystem operations to a designated root directory.
#[derive(Debug, Clone)]
pub struct Workspace {
    /// Absolute, canonical path of the workspace root directory
    root_dir: PathBuf,
}

impl Workspace {
    /// Creates a new sandboxed Workspace.
    /// If the path does not exist, it will be created.
    pub fn new<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let path = path.as_ref();
        
        // Ensure the root directory exists so it can be canonicalized
        if !path.exists() {
            std::fs::create_dir_all(path)?;
        }
        
        let root_dir = std::fs::canonicalize(path)?;
        Ok(Self { root_dir })
    }

    /// Returns the absolute, canonical root directory of the workspace.
    pub fn root_dir(&self) -> &Path {
        &self.root_dir
    }

    /// Checks if the given path is safe (i.e. is inside the workspace root)
    /// and returns the absolute canonicalized path.
    ///
    /// If the path (or parent path) contains traversing elements like `..`, they
    /// are resolved lexically and matched against the root.
    fn safe_path(&self, sub_path: &str) -> anyhow::Result<PathBuf> {
        let sub_path = Path::new(sub_path);

        // Reject absolute paths to avoid jailbreaks like joining `/etc/passwd`
        if sub_path.is_absolute() {
            return Err(anyhow::anyhow!(
                "PermissionDenied: Absolute paths are not allowed in the sandboxed workspace. Path: {:?}",
                sub_path
            ));
        }

        // Combine root directory with the sub-path
        let combined = self.root_dir.join(sub_path);

        // Lexically normalize the combined path (resolve '.' and '..' without checking existence)
        let normalized = self.lexically_normalize(&combined);

        // To support directory canonicalization and symlink resolution for parts that exist,
        // we find the deepest existing ancestor, canonicalize it, and append the remaining components.
        let mut ancestor = normalized.as_path();
        let mut components_to_append = Vec::new();

        while !ancestor.exists() {
            if let Some(parent) = ancestor.parent() {
                if let Some(file_name) = ancestor.file_name() {
                    components_to_append.push(file_name);
                }
                ancestor = parent;
            } else {
                break;
            }
        }

        // Canonicalize the existing ancestor to resolve any real symlinks
        let mut resolved = std::fs::canonicalize(ancestor)?;
        
        // Append the remaining components (in reverse order)
        for comp in components_to_append.into_iter().rev() {
            resolved.push(comp);
        }

        // Normalize the final path again to resolve any '..' that might have been introduced
        let final_path = self.lexically_normalize(&resolved);

        // Check if the final path starts with the workspace root directory
        if final_path.starts_with(&self.root_dir) {
            Ok(final_path)
        } else {
            Err(anyhow::anyhow!(
                "PermissionDenied: Agent attempted to escape sandbox space! Sub-path: {:?}",
                sub_path
            ))
        }
    }

    /// Helper to lexically resolve '.' and '..' in a path.
    /// Does not query the filesystem, so it works for non-existent paths.
    fn lexically_normalize(&self, path: &Path) -> PathBuf {
        let mut components = Vec::new();
        for component in path.components() {
            match component {
                Component::Prefix(..) => {
                    components.push(component);
                }
                Component::RootDir => {
                    components.push(component);
                }
                Component::CurDir => {}
                Component::ParentDir => {
                    if let Some(last) = components.last() {
                        match last {
                            Component::Normal(_) => {
                                components.pop();
                            }
                            Component::ParentDir => {
                                components.push(component);
                            }
                            Component::RootDir | Component::Prefix(..) | Component::CurDir => {
                                // Root, prefix, or CurDir parent cannot go higher / ignore
                            }
                        }
                    } else {
                        components.push(component);
                    }
                }
                Component::Normal(_) => {
                    components.push(component);
                }
            }
        }
        components.iter().collect()
    }

    /// Safe API to write content to a file in the workspace.
    /// Parent directories are automatically created if they do not exist.
    pub async fn write_file(&self, sub_path: &str, content: &str) -> anyhow::Result<()> {
        let secure_path = self.safe_path(sub_path)?;

        if let Some(parent) = secure_path.parent() {
            fs::create_dir_all(parent).await?;
        }

        fs::write(secure_path, content).await?;
        Ok(())
    }

    /// Safe API to read the content of a file in the workspace as a String.
    pub async fn read_file(&self, sub_path: &str) -> anyhow::Result<String> {
        let secure_path = self.safe_path(sub_path)?;
        let content = fs::read_to_string(secure_path).await?;
        Ok(content)
    }

    /// Safe API to edit a file in the workspace by replacing all occurrences of a search string.
    pub async fn edit_file(&self, sub_path: &str, find: &str, replace: &str) -> anyhow::Result<()> {
        let secure_path = self.safe_path(sub_path)?;
        let content = fs::read_to_string(&secure_path).await?;
        let updated_content = content.replace(find, replace);
        fs::write(secure_path, updated_content).await?;
        Ok(())
    }

    /// Safe API to recursively list all files and folders in a subdirectory,
    /// returning paths relative to the workspace root.
    pub async fn list_dir(&self, sub_path: &str) -> anyhow::Result<Vec<String>> {
        let secure_path = self.safe_path(sub_path)?;

        let mut results = Vec::new();
        let mut stack = vec![secure_path.clone()];

        while let Some(current_dir) = stack.pop() {
            let mut entries = fs::read_dir(&current_dir).await?;
            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                let metadata = entry.metadata().await?;
                if metadata.is_dir() {
                    stack.push(path.clone());
                }

                // Convert to relative path string from workspace root
                if let Ok(rel_path) = path.strip_prefix(&self.root_dir) {
                    if let Some(rel_str) = rel_path.to_str() {
                        results.push(rel_str.to_string());
                    }
                }
            }
        }

        results.sort();
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_safe_path_valid() {
        let dir = tempdir().unwrap();
        let ws = Workspace::new(dir.path()).unwrap();
        
        let path = ws.safe_path("src/main.rs").unwrap();
        assert!(path.starts_with(ws.root_dir()));
    }

    #[tokio::test]
    async fn test_safe_path_absolute_rejected() {
        let dir = tempdir().unwrap();
        let ws = Workspace::new(dir.path()).unwrap();
        
        let err = ws.safe_path("/etc/passwd").unwrap_err();
        assert!(err.to_string().contains("PermissionDenied"));
    }

    #[tokio::test]
    async fn test_safe_path_traversal_rejected() {
        let dir = tempdir().unwrap();
        let ws = Workspace::new(dir.path()).unwrap();
        
        let err = ws.safe_path("../etc/passwd").unwrap_err();
        assert!(err.to_string().contains("PermissionDenied"));
    }

    #[tokio::test]
    async fn test_safe_path_complex_traversal_rejected() {
        let dir = tempdir().unwrap();
        let ws = Workspace::new(dir.path()).unwrap();
        
        let err = ws.safe_path("src/nonexistent/../../../../evil.txt").unwrap_err();
        assert!(err.to_string().contains("PermissionDenied"));
    }

    #[tokio::test]
    async fn test_safe_path_internal_traversal_allowed() {
        let dir = tempdir().unwrap();
        let ws = Workspace::new(dir.path()).unwrap();
        
        let path = ws.safe_path("src/components/../main.rs").unwrap();
        assert_eq!(path, ws.root_dir().join("src/main.rs"));
    }
}
