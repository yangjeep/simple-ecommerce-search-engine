use super::{read_file, CgroupError, CgroupReader};
use std::collections::BTreeSet;
use std::path::Path;

impl CgroupReader {
    pub fn single_process_id(&self) -> Result<u32, CgroupError> {
        let mut pids = BTreeSet::new();
        collect_process_ids(self.dir(), &mut pids)?;
        if pids.len() != 1 {
            return Err(CgroupError::ProcessScope {
                pids: pids.into_iter().collect(),
            });
        }
        pids.into_iter()
            .next()
            .ok_or(CgroupError::ProcessScope { pids: Vec::new() })
    }
}

fn collect_process_ids(dir: &Path, pids: &mut BTreeSet<u32>) -> Result<(), CgroupError> {
    for value in read_file(&dir.join("cgroup.procs"))?.lines() {
        let pid = value
            .parse::<u32>()
            .map_err(|_| CgroupError::InvalidInteger {
                field: dir.join("cgroup.procs").display().to_string(),
                value: value.to_owned(),
            })?;
        pids.insert(pid);
    }
    let entries = std::fs::read_dir(dir).map_err(|source| CgroupError::Io {
        path: dir.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| CgroupError::Io {
            path: dir.to_path_buf(),
            source,
        })?;
        if entry
            .file_type()
            .map_err(|source| CgroupError::Io {
                path: entry.path(),
                source,
            })?
            .is_dir()
        {
            collect_process_ids(&entry.path(), pids)?;
        }
    }
    Ok(())
}
