use super::super::model::{EventType, EvidenceFile, Phase};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileOperation {
    CreateNew,
    WriteAll,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Operation {
    ValidateStatic,
    InitializeCreateNew,
    Evidence {
        file: EvidenceFile,
        operation: FileOperation,
    },
    AppendEvent {
        phase: Phase,
        event_type: EventType,
    },
    AuditEquivalence,
    CaptureIndex,
    EvaluateEquivalenceGate,
    AnalyzeCalibration {
        records: usize,
    },
    ExecuteSlot,
    TeardownEnvironment,
    Flush(EvidenceFile),
    Sync(EvidenceFile),
    Close(EvidenceFile),
    ComputeHash,
    CreateSeal,
    WriteSeal {
        entries: usize,
    },
    FlushSeal,
    SyncSeal,
    CloseSeal,
    VerifySeal,
    InvokeAnalyzer,
}

impl Operation {
    pub(crate) const fn close(file: EvidenceFile) -> Self {
        Self::Close(file)
    }

    pub(super) const fn create_new(file: EvidenceFile) -> Self {
        Self::Evidence {
            file,
            operation: FileOperation::CreateNew,
        }
    }

    pub(super) const fn write_all(file: EvidenceFile) -> Self {
        Self::Evidence {
            file,
            operation: FileOperation::WriteAll,
        }
    }
}
