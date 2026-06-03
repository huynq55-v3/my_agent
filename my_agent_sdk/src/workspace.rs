use std::path::{Component, Path, PathBuf};

/// A sandboxed Workspace that represents a named project containing multiple allowed directories.
#[derive(Debug, Clone)]
pub struct Workspace {
    /// The name of the project
    pub name: String,
    /// Absolute, canonical paths of the folders added to this project
    pub folders: Vec<PathBuf>,
}

impl Workspace {
    /// Creates a new, empty Workspace with a name.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            folders: Vec::new(),
        }
    }

    /// Adds a code folder to the project, validating and canonicalizing it.
    pub fn add_folder<P: AsRef<Path>>(&mut self, path: P) -> anyhow::Result<()> {
        let path = path.as_ref();
        
        // Ensure the directory exists
        if !path.exists() {
            std::fs::create_dir_all(path)?;
        }
        
        let canonical = std::fs::canonicalize(path)?;
        if !self.folders.contains(&canonical) {
            self.folders.push(canonical);
        }
        Ok(())
    }

    /// Removes a folder from the project.
    pub fn remove_folder<P: AsRef<Path>>(&mut self, path: P) {
        let path = path.as_ref();
        // Try to canonicalize path to match stored canonicalized paths
        let path_to_remove = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        self.folders.retain(|f| f != &path_to_remove);
    }

    /// Validates if the given target path (absolute or relative) is safe
    /// (i.e. resides inside one of the workspace folders) and returns the absolute canonicalized path.
    pub fn safe_path(&self, target_path: &Path) -> anyhow::Result<PathBuf> {
        if self.folders.is_empty() {
            return Err(anyhow::anyhow!(
                "PermissionDenied: Workspace has no folders configured. Path: {:?}",
                target_path
            ));
        }

        // Determine if target_path is absolute or relative
        let abs_path = if target_path.is_absolute() {
            target_path.to_path_buf()
        } else {
            // For relative paths, we search if the path already exists under any workspace folder.
            // If it doesn't exist, we resolve it against the first folder as a default workspace folder.
            let mut chosen_base = &self.folders[0];
            for folder in &self.folders {
                let candidate = folder.join(target_path);
                if candidate.exists() {
                    chosen_base = folder;
                    break;
                }
            }
            chosen_base.join(target_path)
        };

        // Lexically normalize to resolve any initial `.` or `..` components
        let normalized = self.lexically_normalize(&abs_path);

        // Find the deepest existing ancestor to canonicalize and handle real symlinks
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

        // Canonicalize the existing ancestor to resolve real symlinks
        let mut resolved = std::fs::canonicalize(ancestor)?;
        
        // Append the remaining non-existing components
        for comp in components_to_append.into_iter().rev() {
            resolved.push(comp);
        }

        // Normalize again to clean up any final relative paths
        let final_path = self.lexically_normalize(&resolved);

        // Check if the final path starts with any of the workspace folders
        for folder in &self.folders {
            if final_path.starts_with(folder) {
                return Ok(final_path);
            }
        }

        Err(anyhow::anyhow!(
            "PermissionDenied: Agent attempted to escape sandbox space! Target path: {:?}",
            target_path
        ))
    }

    /// Helper to lexically resolve '.' and '..' in a path without hitting the filesystem.
    fn lexically_normalize(&self, path: &Path) -> PathBuf {
        let mut components = Vec::new();
        for component in path.components() {
            match component {
                Component::Prefix(..) | Component::RootDir => {
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
                                // Root or prefix parent cannot go higher
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
}

/// Tool for reading files in a workspace
pub struct ReadFileTool;

impl ReadFileTool {
    pub fn new() -> Self {
        Self
    }

    pub async fn run(&self, ws: &Workspace, path: &Path) -> anyhow::Result<String> {
        let secure_path = ws.safe_path(path)?;
        let content = tokio::fs::read_to_string(secure_path).await?;
        Ok(content)
    }
}

/// Tool for writing files in a workspace
pub struct WriteFileTool;

impl WriteFileTool {
    pub fn new() -> Self {
        Self
    }

    pub async fn run(&self, ws: &Workspace, path: &Path, content: &str) -> anyhow::Result<()> {
        let secure_path = ws.safe_path(path)?;
        if let Some(parent) = secure_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(secure_path, content).await?;
        Ok(())
    }
}

/// Tool for editing files in a workspace (find and replace)
pub struct EditFileTool;

impl EditFileTool {
    pub fn new() -> Self {
        Self
    }

    pub async fn run(&self, ws: &Workspace, path: &Path, find: &str, replace: &str) -> anyhow::Result<()> {
        let secure_path = ws.safe_path(path)?;
        let content = tokio::fs::read_to_string(&secure_path).await?;
        let updated_content = content.replace(find, replace);
        tokio::fs::write(secure_path, updated_content).await?;
        Ok(())
    }
}

/// Tool for listing files and folders recursively in a workspace
pub struct ListDirTool;

impl ListDirTool {
    pub fn new() -> Self {
        Self
    }

    /// Recursively lists files under target subdirectory.
    /// If relative_dir is empty or ".", lists files under ALL workspace folders.
    pub async fn run(&self, ws: &Workspace, relative_dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
        let mut results = Vec::new();
        
        let is_empty = relative_dir.as_os_str().is_empty() 
            || relative_dir == Path::new("") 
            || relative_dir == Path::new(".");
            
        if is_empty {
            // List recursively for each workspace folder
            for folder in &ws.folders {
                let mut stack = vec![folder.clone()];
                while let Some(current_dir) = stack.pop() {
                    let mut entries = tokio::fs::read_dir(&current_dir).await?;
                    while let Some(entry) = entries.next_entry().await? {
                        let path = entry.path();
                        let metadata = entry.metadata().await?;
                        if metadata.is_dir() {
                            stack.push(path.clone());
                        }
                        results.push(path);
                    }
                }
            }
        } else {
            // Validate the path starts in a workspace folder
            let secure_path = ws.safe_path(relative_dir)?;
            let mut stack = vec![secure_path];
            while let Some(current_dir) = stack.pop() {
                let mut entries = tokio::fs::read_dir(&current_dir).await?;
                while let Some(entry) = entries.next_entry().await? {
                    let path = entry.path();
                    let metadata = entry.metadata().await?;
                    if metadata.is_dir() {
                        stack.push(path.clone());
                    }
                    results.push(path);
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
    async fn test_multi_folder_workspace() {
        let dir1 = tempdir().unwrap();
        let dir2 = tempdir().unwrap();

        let mut ws = Workspace::new("TestProject");
        ws.add_folder(dir1.path()).unwrap();
        ws.add_folder(dir2.path()).unwrap();

        assert_eq!(ws.folders.len(), 2);

        // Test safe paths in both folders
        let path1 = ws.safe_path(Path::new("src/main.rs")).unwrap();
        assert!(path1.starts_with(dir1.path()));

        // Create a file in dir2 and check if safe_path finds it there
        let file2_name = "lib.rs";
        let file2_abs = dir2.path().join(file2_name);
        tokio::fs::write(&file2_abs, "fn test(){}").await.unwrap();

        let path2 = ws.safe_path(Path::new(file2_name)).unwrap();
        assert!(path2.starts_with(dir2.path()));
        assert_eq!(path2, std::fs::canonicalize(file2_abs).unwrap());
    }

    #[tokio::test]
    async fn test_escape_blocked() {
        let dir1 = tempdir().unwrap();
        let mut ws = Workspace::new("TestProject");
        ws.add_folder(dir1.path()).unwrap();

        // Traversal out of dir1
        let err = ws.safe_path(Path::new("../outside.txt")).unwrap_err();
        assert!(err.to_string().contains("PermissionDenied"));

        // Absolute path outside
        let err_abs = ws.safe_path(Path::new("/etc/passwd")).unwrap_err();
        assert!(err_abs.to_string().contains("PermissionDenied"));
    }
}
