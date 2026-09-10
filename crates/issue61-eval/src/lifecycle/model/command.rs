use super::{LifecycleError, Sequence};
use crate::CampaignCycle;
use serde::{Serialize, Serializer};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) enum LoggedText {
    Public(String),
    Redacted(#[serde(serialize_with = "redacted")] String),
}

fn redacted<S>(_: &str, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str("<redacted>")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CommandInput {
    Public(String),
    Redacted(String),
    Unclassified(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EnvironmentInput {
    pub(crate) name: String,
    pub(crate) value: CommandInput,
}

impl EnvironmentInput {
    pub(crate) fn public(name: &str, value: &str) -> Self {
        Self {
            name: name.to_owned(),
            value: CommandInput::Public(value.to_owned()),
        }
    }

    pub(crate) fn redacted(name: &str, value: &str) -> Self {
        Self {
            name: name.to_owned(),
            value: CommandInput::Redacted(value.to_owned()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OutputVisibility {
    Public,
    Redacted,
}

pub(crate) struct CommandRequest {
    pub(crate) executable: String,
    pub(crate) args: Vec<CommandInput>,
    pub(crate) env: Vec<EnvironmentInput>,
    pub(crate) environment_allowlist: Vec<String>,
    pub(crate) stdout_visibility: OutputVisibility,
    pub(crate) stderr_visibility: OutputVisibility,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) enum CommandOutcome {
    Exited { exit_code: i32 },
    SpawnFailed { reason: LoggedText },
    Signaled { signal: i32 },
    TimedOut { timeout_ms: u64 },
}

impl CommandOutcome {
    pub(crate) const fn succeeded(&self) -> bool {
        match self {
            Self::Exited { exit_code: 0 } => true,
            Self::Exited { exit_code: _ }
            | Self::SpawnFailed { reason: _ }
            | Self::Signaled { signal: _ }
            | Self::TimedOut { timeout_ms: _ } => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct PreparedEnvironment {
    name: String,
    value: LoggedText,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DirectEnvironment {
    name: String,
    value: String,
}

pub(crate) struct PreparedCommand {
    executable: String,
    direct_args: Vec<String>,
    direct_env: Vec<DirectEnvironment>,
    args: Vec<LoggedText>,
    env: Vec<PreparedEnvironment>,
    stdout_visibility: OutputVisibility,
    stderr_visibility: OutputVisibility,
}

impl PreparedCommand {
    pub(crate) const fn executable(&self) -> &str {
        self.executable.as_str()
    }

    pub(crate) fn direct_args(&self) -> &[String] {
        &self.direct_args
    }

    pub(crate) fn direct_environment(&self) -> impl Iterator<Item = (&str, &str)> {
        self.direct_env
            .iter()
            .map(|entry| (entry.name.as_str(), entry.value.as_str()))
    }

    pub(crate) fn args(&self) -> &[LoggedText] {
        &self.args
    }

    pub(crate) fn environment_names(&self) -> impl Iterator<Item = &str> {
        self.env.iter().map(|entry| entry.name.as_str())
    }

    pub(crate) fn capture(self, evidence: ExecutionEvidence) -> CommandCapture {
        CommandCapture {
            executable: self.executable,
            args: self.args,
            env: self.env,
            outcome: evidence.outcome,
            stdout: classify_output(evidence.stdout, self.stdout_visibility),
            stderr: classify_output(evidence.stderr, self.stderr_visibility),
        }
    }
}

pub(crate) struct ExecutionEvidence {
    pub(crate) outcome: CommandOutcome,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

pub(crate) struct CommandCapture {
    executable: String,
    args: Vec<LoggedText>,
    env: Vec<PreparedEnvironment>,
    outcome: CommandOutcome,
    stdout: LoggedText,
    stderr: LoggedText,
}

#[derive(Serialize)]
struct CommandRecord {
    schema_version: u8,
    experiment_id: &'static str,
    cycle: &'static str,
    seq: u64,
    executable: String,
    args: Vec<LoggedText>,
    env: Vec<PreparedEnvironment>,
    outcome: CommandOutcome,
    stdout: LoggedText,
    stderr: LoggedText,
}

pub(crate) fn prepare_command(request: CommandRequest) -> Result<PreparedCommand, LifecycleError> {
    let mut allowlist = request.environment_allowlist;
    allowlist.sort_unstable();
    let mut environment_names = request
        .env
        .iter()
        .map(|entry| entry.name.as_str())
        .collect::<Vec<_>>();
    environment_names.sort_unstable();
    if allowlist.windows(2).any(|pair| pair[0] == pair[1])
        || environment_names.windows(2).any(|pair| pair[0] == pair[1])
        || !environment_names
            .into_iter()
            .eq(allowlist.iter().map(String::as_str))
    {
        return Err(LifecycleError::InvalidEnvironment);
    }
    let classified_args = request
        .args
        .into_iter()
        .map(classify_input)
        .collect::<Result<Vec<_>, _>>()?;
    let mut classified_env = request
        .env
        .into_iter()
        .map(|entry| {
            let (value, evidence) = classify_input(entry.value)?;
            Ok((entry.name, value, evidence))
        })
        .collect::<Result<Vec<_>, LifecycleError>>()?;
    classified_env.sort_unstable_by(|left, right| left.0.cmp(&right.0));
    let (direct_args, args): (Vec<_>, Vec<_>) = classified_args.into_iter().unzip();
    let direct_env = classified_env
        .iter()
        .map(|(name, value, _)| DirectEnvironment {
            name: name.clone(),
            value: value.clone(),
        })
        .collect();
    let env = classified_env
        .into_iter()
        .map(|(name, _, value)| PreparedEnvironment { name, value })
        .collect();
    Ok(PreparedCommand {
        executable: request.executable,
        direct_args,
        direct_env,
        args,
        env,
        stdout_visibility: request.stdout_visibility,
        stderr_visibility: request.stderr_visibility,
    })
}

pub(crate) fn serialize_command(
    cycle: CampaignCycle,
    sequence: Sequence,
    capture: CommandCapture,
) -> Result<String, LifecycleError> {
    let record = CommandRecord {
        schema_version: 1,
        experiment_id: "I61-E1",
        cycle: cycle.as_str(),
        seq: sequence.get(),
        executable: capture.executable,
        args: capture.args,
        env: capture.env,
        outcome: capture.outcome,
        stdout: capture.stdout,
        stderr: capture.stderr,
    };
    serde_json::to_string(&record)
        .map(|mut line| {
            line.push('\n');
            line
        })
        .map_err(|_| LifecycleError::Serialization)
}

fn classify_input(value: CommandInput) -> Result<(String, LoggedText), LifecycleError> {
    match value {
        CommandInput::Public(value) => Ok((value.clone(), LoggedText::Public(value))),
        CommandInput::Redacted(value) => Ok((value, LoggedText::Redacted("<redacted>".to_owned()))),
        CommandInput::Unclassified(_) => Err(LifecycleError::UnclassifiedInput),
    }
}

fn classify_output(value: String, visibility: OutputVisibility) -> LoggedText {
    match visibility {
        OutputVisibility::Public => LoggedText::Public(value),
        OutputVisibility::Redacted => LoggedText::Redacted("<redacted>".to_owned()),
    }
}
