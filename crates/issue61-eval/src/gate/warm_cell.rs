use super::CellStability;
use crate::{Dataset, Engine, MetricIdentity, WorkloadProjection};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WarmCellKey {
    pub(crate) engine: Engine,
    pub(crate) dataset: Dataset,
    pub(crate) projection: WorkloadProjection,
    pub(crate) metric: MetricIdentity,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DatasetCellStability {
    pub(crate) key: WarmCellKey,
    pub(crate) cell: CellStability,
}
