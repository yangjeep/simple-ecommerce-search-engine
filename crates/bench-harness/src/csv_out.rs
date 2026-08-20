use std::io::Write;
use std::path::Path;

/// Writes raw per-rep timing samples as CSV (`rep_index,method,sample_ms`),
/// one row per measured repetition -- Issue #6: "capture raw observations,
/// not just summaries." Appends if the file already exists (so multiple
/// methods in one sweep can share a single raw-samples file), writing the
/// header only when creating a new file.
pub fn append_raw_samples(
    path: &Path,
    experiment: &str,
    method: &str,
    samples: &[f64],
) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let is_new = !path.exists();
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    if is_new {
        writeln!(file, "experiment,method,rep_index,sample_ms")?;
    }
    for (i, sample) in samples.iter().enumerate() {
        writeln!(file, "{experiment},{method},{i},{sample}")?;
    }
    Ok(())
}

/// Writes one summary row per (method, distribution) pair -- the
/// percentile table Issue #6 asks every result to report, in a shape
/// ready for a spreadsheet or a `pandas.read_csv` figure-generation
/// script. Appends like [`append_raw_samples`].
#[allow(clippy::too_many_arguments)]
pub fn append_summary_row(
    path: &Path,
    experiment: &str,
    method: &str,
    metric: &str,
    d: &crate::Distribution,
) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let is_new = !path.exists();
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    if is_new {
        writeln!(
            file,
            "experiment,method,metric,n,mean,stddev,min,p10,p25,p50,p75,p90,p95,p99,max"
        )?;
    }
    writeln!(
        file,
        "{experiment},{method},{metric},{},{},{},{},{},{},{},{},{},{},{},{}",
        d.n, d.mean, d.stddev, d.min, d.p10, d.p25, d.p50, d.p75, d.p90, d.p95, d.p99, d.max
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Distribution;

    #[test]
    fn raw_samples_round_trip_header_once_and_all_rows() {
        let dir = std::env::temp_dir().join(format!(
            "bench_harness_test_{}_{}",
            std::process::id(),
            "raw"
        ));
        let path = dir.join("samples.csv");
        let _ = std::fs::remove_dir_all(&dir);

        append_raw_samples(&path, "exp1", "method_a", &[1.0, 2.0, 3.0]).unwrap();
        append_raw_samples(&path, "exp1", "method_b", &[4.0, 5.0]).unwrap();

        let contents = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines[0], "experiment,method,rep_index,sample_ms");
        assert_eq!(lines.len(), 6, "1 header + 3 + 2 data rows");
        assert!(lines[1].starts_with("exp1,method_a,0,1"));
        assert!(lines[4].starts_with("exp1,method_b,0,4"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn summary_rows_append_across_multiple_methods() {
        let dir = std::env::temp_dir().join(format!(
            "bench_harness_test_{}_{}",
            std::process::id(),
            "summary"
        ));
        let path = dir.join("summary.csv");
        let _ = std::fs::remove_dir_all(&dir);

        let d1 = Distribution::compute(&[1.0, 2.0, 3.0]);
        let d2 = Distribution::compute(&[10.0, 20.0, 30.0]);
        append_summary_row(&path, "exp1", "method_a", "latency_ms", &d1).unwrap();
        append_summary_row(&path, "exp1", "method_b", "latency_ms", &d2).unwrap();

        let contents = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 3, "1 header + 2 summary rows");
        assert!(lines[1].starts_with("exp1,method_a,latency_ms,3,2"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
