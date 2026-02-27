use std::path::Path;

/// Loads site URLs from a config file, stripping blank lines and `#` comments.
pub fn load_sites(path: &Path) -> Vec<String> {
    match std::fs::read_to_string(path) {
        Ok(content) => content
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect(),
        Err(e) => {
            tracing::error!("[MONITOR] Config file not found or unreadable ({}): {e}", path.display());
            vec![]
        }
    }
}
