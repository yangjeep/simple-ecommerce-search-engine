//! cgroup-v2 counters for the serving budget (the `i65-serving.slice` and the
//! scopes inside it): `cpu.stat usage_usec` and `memory.current`.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Group {
    pub name: String,
    pub dir: PathBuf,
}

fn read_value(path: &Path, key: Option<&str>) -> Option<u64> {
    let text = std::fs::read_to_string(path).ok()?;
    match key {
        None => text.trim().parse().ok(),
        Some(k) => text.lines().find_map(|l| {
            let (name, v) = l.split_once(' ')?;
            (name == k).then(|| v.trim().parse().ok()).flatten()
        }),
    }
}

impl Group {
    #[must_use]
    pub fn cpu_usec(&self) -> Option<u64> {
        read_value(&self.dir.join("cpu.stat"), Some("usage_usec"))
    }

    #[must_use]
    pub fn memory_bytes(&self) -> Option<u64> {
        read_value(&self.dir.join("memory.current"), None)
    }

    /// Parses `name=/sys/fs/cgroup/...` specs.
    pub fn parse(spec: &str) -> Result<Self, String> {
        let (name, dir) = spec
            .split_once('=')
            .ok_or_else(|| format!("bad cgroup spec {spec:?}, want name=dir"))?;
        Ok(Group {
            name: name.to_owned(),
            dir: PathBuf::from(dir),
        })
    }
}
