use anyhow::{Context, Result};
use include_dir::{Dir, include_dir};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static RESOURCE_MANAGER_COUNTER: AtomicU64 = AtomicU64::new(0);
static RESOURCES_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/resources");

/// A resource manager that embeds files at compile time and extracts them
/// to a temporary directory at runtime. Automatically cleans up on drop.
pub struct ResourceManager {
    temp_dir: PathBuf,
}

impl ResourceManager {
    /// Create a new resource manager with embedded resources
    pub fn new() -> Result<Self> {
        // Use both process ID and a unique counter to avoid conflicts between
        // multiple ResourceManager instances in the same process (e.g., parallel tests)
        let instance_id = RESOURCE_MANAGER_COUNTER.fetch_add(1, Ordering::SeqCst);
        let temp_dir = std::env::temp_dir().join(format!(
            "qmd-syntax-helper-{}-{}",
            std::process::id(),
            instance_id
        ));

        fs::create_dir_all(&temp_dir)
            .with_context(|| format!("Failed to create temp directory: {}", temp_dir.display()))?;

        Ok(Self { temp_dir })
    }

    /// Get a path to a resource, extracting it to temp dir if needed
    pub fn get_resource(&self, path: &str) -> Result<PathBuf> {
        // Find the file in the embedded directory
        let file = RESOURCES_DIR
            .get_file(path)
            .ok_or_else(|| anyhow::anyhow!("Resource not found: {}", path))?;

        // Determine output path in temp directory
        let output_path = self.temp_dir.join(path);

        // Create parent directories if needed
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Write the resource to the temp directory
        fs::write(&output_path, file.contents())
            .with_context(|| format!("Failed to write resource to: {}", output_path.display()))?;

        Ok(output_path)
    }

    /// Get the temp directory path
    #[allow(dead_code)]
    pub fn temp_dir(&self) -> &Path {
        &self.temp_dir
    }

    /// List all available resources
    #[allow(dead_code)]
    pub fn list_resources(&self) -> Vec<String> {
        let mut resources = Vec::new();
        Self::collect_files(&RESOURCES_DIR, "", &mut resources);
        resources
    }

    /// Recursively collect all file paths from a directory
    #[allow(dead_code)]
    fn collect_files(dir: &Dir, prefix: &str, resources: &mut Vec<String>) {
        for file in dir.files() {
            let name = file.path().file_name().unwrap().to_string_lossy();
            let full_path = if prefix.is_empty() {
                name.to_string()
            } else {
                format!("{}/{}", prefix, name)
            };
            resources.push(full_path);
        }

        for subdir in dir.dirs() {
            let name = subdir.path().file_name().unwrap().to_string_lossy();
            let new_prefix = if prefix.is_empty() {
                name.to_string()
            } else {
                format!("{}/{}", prefix, name)
            };
            Self::collect_files(subdir, &new_prefix, resources);
        }
    }
}

impl Drop for ResourceManager {
    fn drop(&mut self) {
        // Clean up temp directory
        if self.temp_dir.exists() {
            // Ignore errors here so it works well under stack unwinding
            let _ = fs::remove_dir_all(&self.temp_dir);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resource_manager_creates_temp_dir() {
        let rm = ResourceManager::new().unwrap();
        assert!(rm.temp_dir().exists());
    }

    #[test]
    fn test_resource_manager_lists_resources() {
        let rm = ResourceManager::new().unwrap();
        let resources = rm.list_resources();
        assert!(resources.contains(&"filters/grid-table-to-list-table.lua".to_string()));
    }

    #[test]
    fn test_resource_manager_extracts_resource() {
        let rm = ResourceManager::new().unwrap();
        let path = rm
            .get_resource("filters/grid-table-to-list-table.lua")
            .unwrap();
        assert!(path.exists());
        assert!(fs::read_to_string(&path).unwrap().contains("Lua filter"));
    }

    #[test]
    fn test_resource_manager_cleans_up() {
        let temp_dir = {
            let rm = ResourceManager::new().unwrap();
            rm.temp_dir().to_path_buf()
        };
        // After rm is dropped, temp dir should be cleaned up
        assert!(!temp_dir.exists());
    }
}
