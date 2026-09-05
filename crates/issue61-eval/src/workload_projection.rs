use crate::{AdmissionClass, FrozenQuery};
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkloadProjection {
    All,
    FastPath,
    Hybrid,
    Punt,
}

impl WorkloadProjection {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::FastPath => "fast-path",
            Self::Hybrid => "hybrid",
            Self::Punt => "punt",
        }
    }

    const fn includes(self, class: AdmissionClass) -> bool {
        match (self, class) {
            (
                Self::All,
                AdmissionClass::FastPath | AdmissionClass::Hybrid | AdmissionClass::Punt,
            )
            | (Self::FastPath, AdmissionClass::FastPath)
            | (Self::Hybrid, AdmissionClass::Hybrid)
            | (Self::Punt, AdmissionClass::Punt) => true,
            (Self::FastPath, AdmissionClass::Hybrid | AdmissionClass::Punt)
            | (Self::Hybrid, AdmissionClass::FastPath | AdmissionClass::Punt)
            | (Self::Punt, AdmissionClass::FastPath | AdmissionClass::Hybrid) => false,
        }
    }
}

impl FromStr for WorkloadProjection {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "all" => Ok(Self::All),
            "fast-path" => Ok(Self::FastPath),
            "hybrid" => Ok(Self::Hybrid),
            "punt" => Ok(Self::Punt),
            other => Err(format!(
                "invalid query class {other:?}; expected all, fast-path, hybrid, or punt"
            )),
        }
    }
}

#[derive(Clone, Copy)]
pub struct ProjectedWorkload<'a> {
    queries: &'a [FrozenQuery],
    projection: WorkloadProjection,
    len: usize,
}

impl<'a> ProjectedWorkload<'a> {
    #[must_use]
    pub const fn query_count(self) -> usize {
        self.len
    }

    pub fn iter(self) -> impl Iterator<Item = &'a FrozenQuery> {
        self.queries
            .iter()
            .filter(move |query| self.projection.includes(query.admission_class))
    }
}

pub fn project_workload(
    queries: &[FrozenQuery],
    projection: WorkloadProjection,
) -> Result<ProjectedWorkload<'_>, String> {
    let len = queries
        .iter()
        .filter(|query| projection.includes(query.admission_class))
        .count();
    if len == 0 {
        return Err(format!(
            "dataset projection {} is empty",
            projection.as_str()
        ));
    }
    Ok(ProjectedWorkload {
        queries,
        projection,
        len,
    })
}
