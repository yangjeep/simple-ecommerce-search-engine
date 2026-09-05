use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionMode {
    Warm,
    Cold,
    CalibrationFour,
    CalibrationFive,
}

impl SessionMode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Warm => "warm",
            Self::Cold => "cold",
            Self::CalibrationFour => "calibration-four",
            Self::CalibrationFive => "calibration-five",
        }
    }

    #[must_use]
    pub const fn plan(self) -> SessionPlan {
        match self {
            Self::Warm => SessionPlan::new(self, 3, 2),
            Self::Cold => SessionPlan::new(self, 0, 1),
            Self::CalibrationFour => SessionPlan::new(self, 3, 4),
            Self::CalibrationFive => SessionPlan::new(self, 3, 5),
        }
    }

    #[must_use]
    pub const fn is_calibration(self) -> bool {
        match self {
            Self::Warm | Self::Cold => false,
            Self::CalibrationFour | Self::CalibrationFive => true,
        }
    }
}

impl FromStr for SessionMode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "warm" => Ok(Self::Warm),
            "cold" => Ok(Self::Cold),
            "calibration-four" => Ok(Self::CalibrationFour),
            "calibration-five" => Ok(Self::CalibrationFive),
            other => Err(format!(
                "invalid session mode {other:?}; expected warm, cold, calibration-four, or calibration-five"
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionPlan {
    mode: SessionMode,
    warmup_passes: usize,
    measured_passes: usize,
}

impl SessionPlan {
    const fn new(mode: SessionMode, warmup_passes: usize, measured_passes: usize) -> Self {
        Self {
            mode,
            warmup_passes,
            measured_passes,
        }
    }

    #[must_use]
    pub const fn mode(self) -> SessionMode {
        self.mode
    }

    #[must_use]
    pub const fn pass_counts(self) -> (usize, usize) {
        (self.warmup_passes, self.measured_passes)
    }

    pub fn steps(self) -> impl Iterator<Item = SessionStep> {
        std::iter::repeat_n(SessionStep::WarmupPass, self.warmup_passes)
            .chain(std::iter::once(SessionStep::OpenCounters))
            .chain(std::iter::repeat_n(
                SessionStep::MeasuredPass,
                self.measured_passes,
            ))
            .chain(std::iter::once(SessionStep::CloseCounters))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStep {
    WarmupPass,
    OpenCounters,
    MeasuredPass,
    CloseCounters,
}
