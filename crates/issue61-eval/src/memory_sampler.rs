use crate::{CgroupError, CgroupReader};
use std::error::Error;
use std::fmt;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub const MEMORY_SAMPLE_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemorySampleSummary {
    pub median_bytes: u64,
    pub max_bytes: u64,
}

#[derive(Debug)]
pub enum MemorySamplerError {
    Cgroup(CgroupError),
    EmptySamples,
    ThreadPanicked,
}

impl fmt::Display for MemorySamplerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cgroup(source) => write!(formatter, "memory sampling failed: {source}"),
            Self::EmptySamples => write!(formatter, "memory sampler returned no samples"),
            Self::ThreadPanicked => write!(formatter, "memory sampler thread panicked"),
        }
    }
}

impl Error for MemorySamplerError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Cgroup(source) => Some(source),
            Self::EmptySamples | Self::ThreadPanicked => None,
        }
    }
}

pub struct MemorySampler {
    stop: Option<Sender<()>>,
    handle: Option<JoinHandle<Result<Vec<u64>, CgroupError>>>,
}

impl MemorySampler {
    pub fn start(reader: CgroupReader) -> Result<Self, CgroupError> {
        let initial = reader.read_memory_current()?;
        let (stop, receiver) = mpsc::channel();
        let handle = thread::spawn(move || {
            let mut samples = vec![initial];
            loop {
                match receiver.recv_timeout(MEMORY_SAMPLE_INTERVAL) {
                    Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
                    Err(RecvTimeoutError::Timeout) => {
                        samples.push(reader.read_memory_current()?);
                    }
                }
            }
            samples.push(reader.read_memory_current()?);
            Ok(samples)
        });
        Ok(Self {
            stop: Some(stop),
            handle: Some(handle),
        })
    }

    pub fn finish(mut self) -> Result<MemorySampleSummary, MemorySamplerError> {
        self.stop_and_join()
    }

    fn stop_and_join(&mut self) -> Result<MemorySampleSummary, MemorySamplerError> {
        drop(self.stop.take());
        let samples = self
            .handle
            .take()
            .ok_or(MemorySamplerError::ThreadPanicked)?
            .join()
            .map_err(|_| MemorySamplerError::ThreadPanicked)?
            .map_err(MemorySamplerError::Cgroup)?;
        summarize_memory_samples(&samples)
    }
}

impl Drop for MemorySampler {
    fn drop(&mut self) {
        if self.handle.is_some() {
            let _ = self.stop_and_join();
        }
    }
}

pub fn summarize_memory_samples(
    samples: &[u64],
) -> Result<MemorySampleSummary, MemorySamplerError> {
    if samples.is_empty() {
        return Err(MemorySamplerError::EmptySamples);
    }
    let mut ordered = samples.to_vec();
    ordered.sort_unstable();
    let middle = ordered.len() / 2;
    let median_bytes = if ordered.len().is_multiple_of(2) {
        ordered[middle - 1] + (ordered[middle] - ordered[middle - 1]) / 2
    } else {
        ordered[middle]
    };
    Ok(MemorySampleSummary {
        median_bytes,
        max_bytes: *ordered.last().ok_or(MemorySamplerError::EmptySamples)?,
    })
}
