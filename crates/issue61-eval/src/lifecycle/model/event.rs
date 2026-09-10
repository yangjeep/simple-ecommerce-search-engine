use super::{
    Attempt, DatasetIdentity, EngineIdentity, LoggedText, Phase, Projection, Sequence,
    SeriesIdentity, SlotIndex,
};
use crate::CampaignCycle;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) enum EventType {
    PhaseStarted,
    PhaseCompleted,
    BlockStarted,
    PrescreenRejected,
    SlotStarted,
    SlotCompleted,
    SlotFailed,
    TeardownStarted,
    TeardownCompleted,
    TeardownFailed,
    BlockCompleted,
    GatePassed,
    GateFailed,
    LifecycleTerminated,
    EvidenceFinalized,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EventIdentity {
    phase: Phase,
    series: Option<SeriesIdentity>,
    block_index: Option<usize>,
    attempt: Option<Attempt>,
    slot_index: Option<SlotIndex>,
    engine: Option<EngineIdentity>,
    dataset: Option<DatasetIdentity>,
    projection: Option<Projection>,
}

impl EventIdentity {
    pub(crate) const fn phase(phase: Phase) -> Self {
        Self {
            phase,
            series: None,
            block_index: None,
            attempt: None,
            slot_index: None,
            engine: None,
            dataset: None,
            projection: None,
        }
    }

    pub(crate) const fn block(
        phase: Phase,
        series: SeriesIdentity,
        block_index: usize,
        attempt: Attempt,
        projection: Option<Projection>,
    ) -> Self {
        Self {
            phase,
            series: Some(series),
            block_index: Some(block_index),
            attempt: Some(attempt),
            slot_index: None,
            engine: None,
            dataset: None,
            projection,
        }
    }

    pub(crate) const fn dataset(mut self, dataset: DatasetIdentity) -> Self {
        self.dataset = Some(dataset);
        self
    }

    pub(crate) const fn slot(mut self, slot: SlotIndex, engine: EngineIdentity) -> Self {
        self.slot_index = Some(slot);
        self.engine = Some(engine);
        self
    }

    pub(crate) fn event(self, event_type: EventType, reason: Option<LoggedText>) -> Event {
        Event {
            phase: self.phase,
            series: self.series,
            block_index: self.block_index,
            attempt: self.attempt.map(Attempt::get),
            slot_index: self.slot_index.map(SlotIndex::get),
            engine: self.engine,
            dataset: self.dataset,
            projection: self.projection,
            event_type,
            reason,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Event {
    phase: Phase,
    series: Option<SeriesIdentity>,
    block_index: Option<usize>,
    attempt: Option<u32>,
    slot_index: Option<u8>,
    engine: Option<EngineIdentity>,
    dataset: Option<DatasetIdentity>,
    projection: Option<Projection>,
    event_type: EventType,
    reason: Option<LoggedText>,
}

impl Event {
    pub(crate) const fn phase(&self) -> Phase {
        self.phase
    }

    pub(crate) const fn event_type(&self) -> EventType {
        self.event_type
    }

    pub(crate) const fn series(&self) -> Option<SeriesIdentity> {
        self.series
    }

    pub(crate) const fn block_index(&self) -> Option<usize> {
        self.block_index
    }

    pub(crate) const fn attempt(&self) -> Option<u32> {
        self.attempt
    }

    pub(crate) const fn projection(&self) -> Option<Projection> {
        self.projection
    }
}

#[derive(Serialize)]
pub(crate) struct EventRecord {
    schema_version: u8,
    experiment_id: &'static str,
    cycle: &'static str,
    seq: u64,
    #[serde(flatten)]
    event: Event,
}

impl EventRecord {
    pub(crate) const fn new(cycle: CampaignCycle, sequence: Sequence, event: Event) -> Self {
        Self {
            schema_version: 1,
            experiment_id: "I61-E1",
            cycle: cycle.as_str(),
            seq: sequence.get(),
            event,
        }
    }
}
