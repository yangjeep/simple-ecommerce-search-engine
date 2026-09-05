use issue61_eval::{CgroupError, CgroupReader};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("issue61-cgroup-{}-{id}", std::process::id()));
        std::fs::create_dir_all(&root).expect("fixture root should be created");
        Self { root }
    }

    fn write(&self, relative: &str, content: &str) {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("fixture parent should be created");
        }
        std::fs::write(path, content).expect("fixture file should be written");
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn write_complete_snapshot(fixture: &Fixture, dir: &Path) {
    let relative = dir
        .strip_prefix(&fixture.root)
        .expect("dir belongs to fixture");
    fixture.write(
        relative
            .join("cpu.stat")
            .to_str()
            .expect("UTF-8 fixture path"),
        "usage_usec 120\nuser_usec 80\nsystem_usec 40\nnr_periods 5\nnr_throttled 2\nthrottled_usec 11\n",
    );
    fixture.write(
        relative
            .join("cpu.pressure")
            .to_str()
            .expect("UTF-8 fixture path"),
        "some avg10=0.00 avg60=0.01 avg300=0.02 total=17\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=3\n",
    );
    fixture.write(
        relative
            .join("memory.current")
            .to_str()
            .expect("UTF-8 fixture path"),
        "4096\n",
    );
    fixture.write(
        relative
            .join("memory.peak")
            .to_str()
            .expect("UTF-8 fixture path"),
        "8192\n",
    );
    for (name, content) in [
        ("memory.stat", "anon 1000\nfile 2000\nkernel 300\nsock 40\n"),
        (
            "memory.events",
            "low 1\nhigh 2\nmax 3\noom 4\noom_kill 5\noom_group_kill 6\n",
        ),
        ("memory.swap.current", "256\n"),
        ("memory.swap.peak", "512\n"),
        ("memory.swap.events", "high 7\nmax 8\nfail 9\n"),
        ("memory.swap.max", "1024\n"),
        ("cpuset.cpus.effective", "0-2\n"),
        ("cpu.max", "300000 100000\n"),
        ("memory.max", "6442450944\n"),
    ] {
        fixture.write(
            relative.join(name).to_str().expect("UTF-8 fixture path"),
            content,
        );
    }
}

#[test]
fn cpu_stat_and_memory_current_parse_from_fixture_tree() {
    let fixture = Fixture::new();
    let dir = fixture.root.join("group");
    write_complete_snapshot(&fixture, &dir);

    let snapshot = CgroupReader::at_dir(dir)
        .snapshot()
        .expect("complete fixture should parse");

    assert_eq!(snapshot.usage_usec, 120);
    assert_eq!(snapshot.user_usec, 80);
    assert_eq!(snapshot.system_usec, 40);
    assert_eq!(snapshot.nr_periods, 5);
    assert_eq!(snapshot.nr_throttled, 2);
    assert_eq!(snapshot.throttled_usec, 11);
    assert_eq!(snapshot.cpu_pressure_some_usec, 17);
    assert_eq!(snapshot.cpu_pressure_full_usec, 3);
    assert_eq!(snapshot.memory_current_bytes, 4096);
    assert_eq!(snapshot.memory_peak_bytes, 8192);
    assert_eq!(snapshot.memory_anon_bytes, 1000);
    assert_eq!(snapshot.memory_file_bytes, 2000);
    assert_eq!(snapshot.memory_kernel_bytes, 300);
    assert_eq!(snapshot.memory_sock_bytes, 40);
    assert_eq!(snapshot.memory_swap_current_bytes, 256);
    assert_eq!(snapshot.memory_swap_peak_bytes, 512);
    assert_eq!(snapshot.memory_events.oom_kill, 5);
    assert_eq!(snapshot.memory_swap_events.fail, 9);
    assert_eq!(snapshot.cpuset_cpus_effective, "0-2");
    assert_eq!(snapshot.cpu_max, "300000 100000");
    assert_eq!(snapshot.memory_max, "6442450944");
    assert_eq!(snapshot.memory_swap_max, "1024");
}

#[test]
fn required_memory_peak_absent_is_an_error() {
    let fixture = Fixture::new();
    let dir = fixture.root.join("group");
    write_complete_snapshot(&fixture, &dir);
    std::fs::remove_file(dir.join("memory.peak")).expect("fixture peak should be removed");

    let error = CgroupReader::at_dir(dir)
        .snapshot()
        .expect_err("required peak must fail closed");

    assert!(matches!(error, CgroupError::Io { .. }));
}

#[test]
fn cgroup_path_resolves_from_proc_pid_cgroup_v2_line() {
    let fixture = Fixture::new();
    let mount = fixture.root.join("cgroup");

    let reader = CgroupReader::from_proc_cgroup_content(
        "1:name=systemd:/legacy\n0::/system.slice/docker-abc.scope\n",
        &mount,
    )
    .expect("v2 line should resolve");

    assert_eq!(reader.dir(), mount.join("system.slice/docker-abc.scope"));
}

#[test]
fn cgroup_v1_only_content_is_rejected_as_not_v2() {
    let error = CgroupReader::from_proc_cgroup_content("1:cpu:/foo\n", Path::new("/cg"))
        .expect_err("v1 content must be rejected");

    assert!(matches!(error, CgroupError::NotV2));
}

#[test]
fn missing_usage_usec_key_is_missing_field_error() {
    let fixture = Fixture::new();
    let dir = fixture.root.join("group");
    write_complete_snapshot(&fixture, &dir);
    fixture.write(
        "group/cpu.stat",
        "user_usec 80\nsystem_usec 40\nnr_periods 5\nnr_throttled 2\nthrottled_usec 11\n",
    );

    let error = CgroupReader::at_dir(dir)
        .snapshot()
        .expect_err("required usage counter is absent");

    assert!(matches!(error, CgroupError::MissingField("usage_usec")));
}

#[test]
fn delta_rejects_a_backwards_counter_instead_of_reporting_zero() {
    // Given
    let fixture = Fixture::new();
    let dir = fixture.root.join("group");
    write_complete_snapshot(&fixture, &dir);
    let earlier = CgroupReader::at_dir(dir.clone())
        .snapshot()
        .expect("earlier snapshot");
    fixture.write(
        "group/cpu.stat",
        "usage_usec 10\nuser_usec 8\nsystem_usec 2\nnr_periods 5\nnr_throttled 2\nthrottled_usec 11\n",
    );
    let later = CgroupReader::at_dir(dir)
        .snapshot()
        .expect("later snapshot");

    // When
    let error = later
        .delta_since(&earlier)
        .expect_err("a counter rollback must invalidate the measurement");

    // Then
    assert!(matches!(
        error,
        CgroupError::CounterRollback {
            field: "usage_usec",
            earlier: 120,
            later: 10,
        }
    ));
}

#[test]
fn delta_rejects_pressure_and_event_counter_rollbacks() {
    // Given
    let fixture = Fixture::new();
    let dir = fixture.root.join("group");
    write_complete_snapshot(&fixture, &dir);
    let earlier = CgroupReader::at_dir(dir.clone())
        .snapshot()
        .expect("earlier snapshot");
    fixture.write(
        "group/cpu.pressure",
        "some avg10=0 avg60=0 avg300=0 total=1\nfull avg10=0 avg60=0 avg300=0 total=3\n",
    );
    let pressure_rollback = CgroupReader::at_dir(dir.clone())
        .snapshot()
        .expect("pressure rollback snapshot");

    // When / Then
    assert!(pressure_rollback.delta_since(&earlier).is_err());
    write_complete_snapshot(&fixture, &dir);
    fixture.write(
        "group/memory.events",
        "low 1\nhigh 2\nmax 3\noom 4\noom_kill 1\noom_group_kill 6\n",
    );
    let event_rollback = CgroupReader::at_dir(dir)
        .snapshot()
        .expect("event rollback snapshot");
    assert!(event_rollback.delta_since(&earlier).is_err());
}
