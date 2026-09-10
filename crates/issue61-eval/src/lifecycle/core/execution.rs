use super::Driver;
use crate::lifecycle::model::{
    prepare_command, serialize_command, Attempt, BlockContext, DatasetIdentity, EngineIdentity,
    EventIdentity, EventType, LifecycleError, LoggedText, Phase, Projection, Sequence,
    SeriesIdentity, SlotContext, SlotIndex,
};
use crate::lifecycle::port::LifecyclePort;
use crate::lifecycle::port::SlotExecution;
use crate::{CampaignPhase, CampaignPlan, CampaignSeries, EngineOrder};

impl<P: LifecyclePort> Driver<'_, P> {
    pub(super) fn execute_phase(
        &mut self,
        plan: &CampaignPlan,
        campaign_phase: CampaignPhase,
        phase: Phase,
    ) -> Result<Vec<crate::RawRecord>, LifecycleError> {
        self.phase(phase, EventType::PhaseStarted, None)?;
        let mut accepted = Vec::new();
        let mut active_series = None;
        let mut rejection_count = 0;
        for block in plan
            .blocks()
            .iter()
            .filter(|block| block.series().phase() == campaign_phase)
        {
            if active_series != Some(block.series()) {
                active_series = Some(block.series());
                rejection_count = 0;
            }
            let mut attempt = Attempt::new(1);
            loop {
                let context = block_context(block.series(), block.index().get(), attempt, phase);
                if self.port.prescreen(context)? {
                    accepted.extend(self.execute_block(context, block.sessions())?);
                    break;
                }
                rejection_count += 1;
                self.append(block_identity(context).event(
                    EventType::PrescreenRejected,
                    Some(LoggedText::Public("prescreen_rejected".to_owned())),
                ))?;
                if rejection_count >= rejection_limit(block.series()) {
                    return Err(LifecycleError::RejectionLimit {
                        series: context.campaign_series,
                    });
                }
                attempt = attempt.next();
            }
        }
        self.phase(phase, EventType::PhaseCompleted, None)?;
        Ok(accepted)
    }

    fn execute_block(
        &mut self,
        block: BlockContext,
        sessions: &[crate::SessionSpec; 2],
    ) -> Result<[crate::RawRecord; 2], LifecycleError> {
        self.append(block_identity(block).event(EventType::BlockStarted, None))?;
        let first = self.execute_slot(slot_context(block, sessions[0]))?;
        let second = self.execute_slot(slot_context(block, sessions[1]))?;
        let records = [first, second];
        self.port.append_raw(records.clone())?;
        self.append(block_identity(block).event(EventType::BlockCompleted, None))?;
        Ok(records)
    }

    fn execute_slot(
        &mut self,
        slot: SlotContext,
    ) -> Result<crate::lifecycle::model::RawRecord, LifecycleError> {
        let prepared = prepare_command(self.port.command_request(slot))?;
        let identity = slot_identity(slot);
        self.append(identity.event(EventType::SlotStarted, None))?;
        let execution = self.port.execute_slot(slot, &prepared);
        let (evidence, result) = match execution {
            SlotExecution::Completed { evidence, record } if evidence.outcome.succeeded() => {
                (evidence, Ok(*record))
            }
            SlotExecution::Completed {
                evidence,
                record: _,
            } => (evidence, Err(LifecycleError::SlotFailed)),
            SlotExecution::Failed { evidence, error } => (evidence, Err(error)),
        };
        let sequence = Sequence::new(self.next_sequence);
        let capture = prepared.capture(evidence);
        let command_result = serialize_command(self.cycle, sequence, capture)
            .and_then(|line| self.port.append_command(sequence, line));
        if command_result.is_ok() {
            self.next_sequence += 1;
        }
        self.finish_started_slot(slot, identity, result, command_result)
    }

    fn finish_started_slot(
        &mut self,
        slot: SlotContext,
        identity: EventIdentity,
        result: Result<crate::lifecycle::model::RawRecord, LifecycleError>,
        command_result: Result<(), LifecycleError>,
    ) -> Result<crate::lifecycle::model::RawRecord, LifecycleError> {
        let (record, mut primary_error) = match (command_result, result) {
            (Err(error), _) => (None, Some(error)),
            (Ok(()), Err(error)) => (None, Some(error)),
            (Ok(()), Ok(record)) => (Some(record), None),
        };
        let event_type = if primary_error.is_some() {
            EventType::SlotFailed
        } else {
            EventType::SlotCompleted
        };
        let detail = primary_error
            .as_ref()
            .map(LifecycleError::classified_reason);
        if let Err(error) = self.append(identity.event(event_type, detail)) {
            primary_error.get_or_insert(error);
        }
        if let Err(error) = self.append(identity.event(EventType::TeardownStarted, None)) {
            primary_error.get_or_insert(error);
        }
        let teardown_result = self.port.teardown(slot);
        let teardown_event = if teardown_result.is_ok() {
            EventType::TeardownCompleted
        } else {
            EventType::TeardownFailed
        };
        let teardown_detail = teardown_result
            .as_ref()
            .err()
            .map(|_| LoggedText::Public("teardown_failed".to_owned()));
        if let Err(error) = self.append(identity.event(teardown_event, teardown_detail)) {
            primary_error.get_or_insert(error);
        }
        if teardown_result.is_err() {
            primary_error.get_or_insert(LifecycleError::TeardownFailed);
        }
        match primary_error {
            Some(error) => Err(error),
            None => record.ok_or(LifecycleError::SlotFailed),
        }
    }
}

fn block_context(
    series: CampaignSeries,
    block_index: usize,
    attempt: Attempt,
    phase: Phase,
) -> BlockContext {
    let projection = match series {
        CampaignSeries::Warm { projection, .. } => Some(Projection::from(projection)),
        CampaignSeries::Calibration { .. } | CampaignSeries::Cold { .. } => None,
    };
    BlockContext {
        phase,
        series: SeriesIdentity::from(series),
        campaign_series: series,
        block_index,
        attempt,
        dataset: DatasetIdentity::from(series.dataset()),
        projection,
    }
}

fn block_identity(block: BlockContext) -> EventIdentity {
    EventIdentity::block(
        block.phase,
        block.series,
        block.block_index,
        block.attempt,
        block.projection,
    )
    .dataset(block.dataset)
}

fn slot_context(block: BlockContext, session: crate::SessionSpec) -> SlotContext {
    let slot = match session.slot() {
        EngineOrder::First => SlotIndex::zero(),
        EngineOrder::Second => SlotIndex::one(),
    };
    SlotContext {
        block,
        slot,
        engine: EngineIdentity::from(session.engine()),
        session,
    }
}

fn slot_identity(slot: SlotContext) -> EventIdentity {
    block_identity(slot.block).slot(slot.slot, slot.engine)
}

const fn rejection_limit(series: CampaignSeries) -> usize {
    match series {
        CampaignSeries::Calibration { .. } | CampaignSeries::Warm { .. } => 7,
        CampaignSeries::Cold { .. } => 2,
    }
}
