//! Plan cache support for fast --plan output.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

/// A cached plan output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanCacheEntry {
    /// Cache key for this entry.
    pub key: String,
    /// When the entry was created.
    pub created_at: DateTime<Utc>,
    /// Lines printed before the plan sections (e.g., warnings).
    pub prefix: Vec<String>,
    /// Lines printed under the EXECUTION PLAN section.
    pub body: Vec<String>,
    /// Lines printed under the PLAN SUMMARY section.
    pub summary: Vec<String>,
}

impl PlanCacheEntry {
    /// Create a new cache entry from plan lines.
    pub fn new(
        key: impl Into<String>,
        prefix: Vec<String>,
        body: Vec<String>,
        summary: Vec<String>,
    ) -> Self {
        Self {
            key: key.into(),
            created_at: Utc::now(),
            prefix,
            body,
            summary,
        }
    }
}

/// Plan cache manager.
#[derive(Debug)]
pub struct PlanCache {
    dir: PathBuf,
    ttl: Duration,
}

impl PlanCache {
    /// Create a new plan cache using the provided directory and TTL.
    pub fn new(dir: PathBuf, ttl: Duration) -> Self {
        Self { dir, ttl }
    }

    /// Default cache directory.
    pub fn default_dir() -> PathBuf {
        dirs::cache_dir()
            .map(|d| d.join("rustible/plan"))
            .unwrap_or_else(|| PathBuf::from(".cache/rustible/plan"))
    }

    /// Load a cached plan entry by key if it exists and is fresh.
    pub fn load(&self, key: &str) -> std::io::Result<Option<PlanCacheEntry>> {
        if !self.dir.exists() {
            return Ok(None);
        }

        let path = self.entry_path(key);
        if !path.exists() {
            return Ok(None);
        }

        let content = match fs::read_to_string(&path) {
            Ok(content) => content,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err),
        };

        let entry: PlanCacheEntry = match serde_json::from_str(&content) {
            Ok(entry) => entry,
            Err(_) => {
                let _ = fs::remove_file(&path);
                return Ok(None);
            }
        };

        if entry.key != key {
            let _ = fs::remove_file(&path);
            return Ok(None);
        }

        if !self.is_fresh(&entry) {
            let _ = fs::remove_file(&path);
            return Ok(None);
        }

        Ok(Some(entry))
    }

    /// Store a cache entry on disk.
    pub fn store(&self, entry: &PlanCacheEntry) -> std::io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        let path = self.entry_path(&entry.key);
        let content = serde_json::to_string_pretty(entry)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err))?;
        fs::write(path, content)?;
        Ok(())
    }

    fn entry_path(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{}.json", key))
    }

    fn is_fresh(&self, entry: &PlanCacheEntry) -> bool {
        if self.ttl.is_zero() {
            return true;
        }

        let max_age = match chrono::Duration::from_std(self.ttl) {
            Ok(duration) => duration,
            Err(_) => return true,
        };
        Utc::now()
            .signed_duration_since(entry.created_at)
            .num_milliseconds()
            <= max_age.num_milliseconds()
    }
}
