use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    Native,
    Solr,
}

impl Engine {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Solr => "solr",
        }
    }
}

impl FromStr for Engine {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "native" => Ok(Self::Native),
            "solr" => Ok(Self::Solr),
            other => Err(format!("invalid engine {other:?}; expected native or solr")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionMode {
    Warm,
}

impl SessionMode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Warm => "warm",
        }
    }
}

impl FromStr for SessionMode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "warm" => Ok(Self::Warm),
            other => Err(format!("invalid session mode {other:?}; expected warm")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStep {
    WarmupPass,
    OpenCounters,
    MeasuredPass,
    CloseCounters,
}

pub const WARM_SESSION_STEPS: [SessionStep; 7] = [
    SessionStep::WarmupPass,
    SessionStep::WarmupPass,
    SessionStep::WarmupPass,
    SessionStep::OpenCounters,
    SessionStep::MeasuredPass,
    SessionStep::MeasuredPass,
    SessionStep::CloseCounters,
];
