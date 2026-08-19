use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Everything Issue #6 asks every run to record: commit SHA, hardware/
/// environment, dataset version, query-set version, config, seed, and
/// method. A `Distribution` or a relevance table without this attached is
/// a number, not evidence -- another run cannot tell whether it is a
/// replication or a different experiment.
///
/// **Known limitation, stated rather than hidden**: `dataset_fingerprint`/
/// `query_set_fingerprint` are `(size_bytes, modified_unix_secs)`, not a
/// full content hash -- the real catalog file is ~1.5GB, and hashing it
/// on every run would materially slow down every experiment for a
/// property (path + size + mtime) that in practice never lies within this
/// project's own gitignored `dataset_cache/` workflow (the file is
/// downloaded/exported once and not silently mutated in place). A full
/// hash would be strictly more rigorous; this is the deliberate,
/// documented tradeoff.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunManifest {
    pub experiment: String,
    pub method: String,
    pub commit_sha: String,
    pub git_dirty: bool,
    pub hostname: String,
    pub cpu_count: usize,
    pub os: String,
    pub dataset_path: String,
    pub dataset_fingerprint: String,
    pub query_set_path: String,
    pub query_set_fingerprint: String,
    pub config: Value,
    pub seed: u64,
    pub warmup: usize,
    pub reps: usize,
}

fn run(cmd: &str, args: &[&str]) -> Option<String> {
    Command::new(cmd)
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

fn file_fingerprint(path: &Path) -> String {
    match std::fs::metadata(path) {
        Ok(meta) => {
            let size = meta.len();
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            format!("size={size}b_mtime={mtime}")
        }
        Err(_) => "unavailable (file not found at manifest-capture time)".to_string(),
    }
}

impl RunManifest {
    /// Captures everything derivable from the environment at call time.
    /// `config` is caller-supplied (e.g. `serde_json::json!({"min_enum_frequency": 25, "selectivity_threshold": 0.05})`)
    /// since only the caller knows its own experiment's tunable parameters.
    #[allow(clippy::too_many_arguments)]
    pub fn capture(
        experiment: &str,
        method: &str,
        dataset_path: &Path,
        query_set_path: &Path,
        config: Value,
        seed: u64,
        warmup: usize,
        reps: usize,
    ) -> Self {
        let commit_sha =
            run("git", &["rev-parse", "HEAD"]).unwrap_or_else(|| "unknown".to_string());
        let git_dirty = run("git", &["status", "--porcelain"])
            .map(|s| !s.is_empty())
            .unwrap_or(true); // fail closed: unknown status counts as dirty
        let hostname = run("hostname", &[]).unwrap_or_else(|| "unknown".to_string());
        let os = std::env::consts::OS.to_string();
        let cpu_count = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(0);

        RunManifest {
            experiment: experiment.to_string(),
            method: method.to_string(),
            commit_sha,
            git_dirty,
            hostname,
            cpu_count,
            os,
            dataset_path: dataset_path.display().to_string(),
            dataset_fingerprint: file_fingerprint(dataset_path),
            query_set_path: query_set_path.display().to_string(),
            query_set_fingerprint: file_fingerprint(query_set_path),
            config,
            seed,
            warmup,
            reps,
        }
    }

    pub fn print(&self) {
        println!(
            "  [manifest] experiment={} method={} commit={} dirty={} host={} cpus={} os={} seed={} warmup={} reps={}",
            self.experiment,
            self.method,
            &self.commit_sha[..self.commit_sha.len().min(12)],
            self.git_dirty,
            self.hostname,
            self.cpu_count,
            self.os,
            self.seed,
            self.warmup,
            self.reps
        );
        println!(
            "  [manifest] dataset={} ({})  query_set={} ({})",
            self.dataset_path,
            self.dataset_fingerprint,
            self.query_set_path,
            self.query_set_fingerprint
        );
        println!("  [manifest] config={}", self.config);
    }

    pub fn write_json(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(self)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_fingerprint_of_a_nonexistent_path_does_not_panic() {
        let fp = file_fingerprint(Path::new("/definitely/does/not/exist/at/all.jsonl"));
        assert!(fp.contains("unavailable"));
    }

    #[test]
    fn capture_produces_a_non_empty_commit_sha_in_this_git_repo() {
        let manifest = RunManifest::capture(
            "test_experiment",
            "test_method",
            Path::new("Cargo.toml"),
            Path::new("Cargo.toml"),
            serde_json::json!({"k": 1}),
            7,
            3,
            10,
        );
        // This crate always runs inside the real git repo in CI/dev, so a
        // real SHA (40 hex chars) is expected -- "unknown" would mean the
        // git subprocess call itself is broken, worth failing loudly on.
        assert_eq!(
            manifest.commit_sha.len(),
            40,
            "expected a real 40-char commit SHA, got {:?}",
            manifest.commit_sha
        );
    }
}
