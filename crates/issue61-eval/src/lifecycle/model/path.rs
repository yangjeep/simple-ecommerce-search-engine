use super::LifecycleError;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CyclePath(PathBuf);

impl CyclePath {
    pub(crate) fn derive(root: &Path, cycle: crate::CampaignCycle) -> Result<Self, LifecycleError> {
        if !root.is_absolute()
            || root
                .components()
                .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
        {
            return Err(LifecycleError::Initialization);
        }
        Ok(Self(
            root.join("artifacts/issue61")
                .join(format!("i61_e1_{}", cycle.as_str())),
        ))
    }

    pub(crate) fn as_path(&self) -> &Path {
        &self.0
    }
}
