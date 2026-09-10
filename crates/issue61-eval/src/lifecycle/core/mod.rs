mod execution;
mod finalization;

use super::model::{
    validate_index, CyclePath, Event, EventIdentity, EventType, EvidenceFile, LifecycleError,
    LoggedText, Phase, RawRecord, Sequence, Terminal,
};
use super::port::LifecyclePort;
use crate::{analyze_calibration, CampaignCycle, CampaignPhase, CampaignPlan};

pub(super) fn run<P: LifecyclePort>(
    plan: &CampaignPlan,
    port: &mut P,
) -> Result<Terminal, LifecycleError> {
    let staged = [
        EventIdentity::phase(Phase::StaticValidation).event(EventType::PhaseStarted, None),
        EventIdentity::phase(Phase::StaticValidation).event(EventType::PhaseCompleted, None),
        EventIdentity::phase(Phase::Initialization).event(EventType::PhaseStarted, None),
    ];
    port.validate_static()?;
    let cycle_path = CyclePath::derive(port.repository_root(), plan.cycle())?;
    if let Err(failure) = port.initialize(&cycle_path) {
        if !EvidenceFile::ALL
            .into_iter()
            .any(|file| port.evidence_is_open(file))
        {
            return Err(failure.error);
        }
        let mut driver = Driver::new(plan.cycle(), port);
        if failure.events_opened {
            for event in staged {
                let _staged_result = driver.append(event);
            }
        }
        return driver.fail(Phase::Initialization, failure.error);
    }
    let mut driver = Driver::new(plan.cycle(), port);
    for event in staged {
        if let Err(error) = driver.append(event) {
            return driver.fail(Phase::Initialization, error);
        }
    }
    if let Err(error) = driver.phase(Phase::Initialization, EventType::PhaseCompleted, None) {
        return driver.fail(Phase::Initialization, error);
    }
    match driver.run_open(plan) {
        Ok(terminal) => Ok(terminal),
        Err((phase, error)) => driver.fail(phase, error),
    }
}

struct Driver<'a, P> {
    cycle: CampaignCycle,
    next_sequence: u64,
    port: &'a mut P,
}

impl<'a, P: LifecyclePort> Driver<'a, P> {
    const fn new(cycle: CampaignCycle, port: &'a mut P) -> Self {
        Self {
            cycle,
            next_sequence: 1,
            port,
        }
    }

    fn append(&mut self, event: Event) -> Result<(), LifecycleError> {
        let sequence = Sequence::new(self.next_sequence);
        self.port.append_event(self.cycle, sequence, event)?;
        self.next_sequence += 1;
        Ok(())
    }

    fn phase(
        &mut self,
        phase: Phase,
        event_type: EventType,
        reason: Option<LoggedText>,
    ) -> Result<(), LifecycleError> {
        self.append(EventIdentity::phase(phase).event(event_type, reason))
    }

    fn run_open(&mut self, plan: &CampaignPlan) -> Result<Terminal, (Phase, LifecycleError)> {
        self.audit_equivalence()
            .map_err(|error| (Phase::EquivalenceAudit, error))?;
        self.capture_index()
            .map_err(|error| (Phase::IndexCapture, error))?;
        self.phase(Phase::EquivalenceGate, EventType::PhaseStarted, None)
            .map_err(|error| (Phase::EquivalenceGate, error))?;
        let equivalence_passes = self.port.evaluate_equivalence_gate();
        if !self
            .finish_gate(Phase::EquivalenceGate, equivalence_passes)
            .map_err(|error| (Phase::EquivalenceGate, error))?
        {
            return self.gate_terminal(Phase::EquivalenceGate);
        }
        let calibration = self
            .execute_phase(plan, CampaignPhase::Calibration, Phase::Calibration)
            .map_err(|error| (Phase::Calibration, error))?;
        self.phase(Phase::CalibrationGate, EventType::PhaseStarted, None)
            .map_err(|error| (Phase::CalibrationGate, error))?;
        let calibration_passes = self
            .calibration_passes(&calibration)
            .map_err(|error| (Phase::CalibrationGate, error))?;
        if !self
            .finish_gate(Phase::CalibrationGate, calibration_passes)
            .map_err(|error| (Phase::CalibrationGate, error))?
        {
            return self.gate_terminal(Phase::CalibrationGate);
        }
        self.execute_phase(plan, CampaignPhase::Warm, Phase::Warm)
            .map_err(|error| (Phase::Warm, error))?;
        self.execute_phase(plan, CampaignPhase::Cold, Phase::Cold)
            .map_err(|error| (Phase::Cold, error))?;
        self.finalize()
            .map_err(|error| (Phase::EvidenceFinalization, error))
    }

    fn audit_equivalence(&mut self) -> Result<(), LifecycleError> {
        self.phase(Phase::EquivalenceAudit, EventType::PhaseStarted, None)?;
        self.port.audit_equivalence()?;
        self.phase(Phase::EquivalenceAudit, EventType::PhaseCompleted, None)
    }

    fn capture_index(&mut self) -> Result<(), LifecycleError> {
        self.phase(Phase::IndexCapture, EventType::PhaseStarted, None)?;
        let artifacts = self.port.capture_index(self.cycle)?;
        validate_index(self.cycle, &artifacts).map_err(LifecycleError::Index)?;
        self.phase(Phase::IndexCapture, EventType::PhaseCompleted, None)
    }

    fn calibration_passes(&mut self, records: &[RawRecord]) -> Result<bool, LifecycleError> {
        self.port.calibration_analyzed(records.len());
        let result =
            analyze_calibration(self.cycle, records).map_err(LifecycleError::Calibration)?;
        Ok(match (result.native, result.solr) {
            (Some(native), Some(solr)) => native.passed && solr.passed,
            (Some(_), None) | (None, Some(_)) | (None, None) => false,
        })
    }

    fn finish_gate(&mut self, phase: Phase, passes: bool) -> Result<bool, LifecycleError> {
        let (event_type, reason) = if passes {
            (EventType::GatePassed, None)
        } else {
            (
                EventType::GateFailed,
                Some(LoggedText::Public(format!("{phase:?}_failed"))),
            )
        };
        self.phase(phase, event_type, reason)?;
        if passes {
            self.phase(phase, EventType::PhaseCompleted, None)?;
        }
        Ok(passes)
    }

    fn gate_terminal(&mut self, phase: Phase) -> Result<Terminal, (Phase, LifecycleError)> {
        let reason = LoggedText::Public(format!("{phase:?}_failed"));
        self.cleanup(phase, None, reason)
            .map_err(|error| (phase, error))?;
        Ok(Terminal::GateFailed(phase))
    }

    fn fail<T>(&mut self, phase: Phase, error: LifecycleError) -> Result<T, LifecycleError> {
        let reason = error.classified_reason();
        let fallback = error.clone();
        match self.cleanup(phase, Some(error), reason) {
            Err(primary) => Err(primary),
            Ok(()) => Err(fallback),
        }
    }

    fn cleanup(
        &mut self,
        phase: Phase,
        mut primary: Option<LifecycleError>,
        reason: LoggedText,
    ) -> Result<(), LifecycleError> {
        if let Err(error) = self.port.teardown_environment() {
            primary.get_or_insert(error);
        }
        if self.port.evidence_is_open(EvidenceFile::Events) {
            if let Err(error) = self.phase(phase, EventType::LifecycleTerminated, Some(reason)) {
                primary.get_or_insert(error);
            }
        }
        for file in EvidenceFile::NON_EVENT_CLOSE_ORDER {
            if let Err(error) = self.durable_close(file) {
                primary.get_or_insert(error);
            }
        }
        if let Err(error) = self.durable_close(EvidenceFile::Events) {
            primary.get_or_insert(error);
        }
        match primary {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}
