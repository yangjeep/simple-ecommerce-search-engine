use issue61_eval::{CgroupError, CgroupReader, CgroupSnapshot};
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
        "usage_usec 120\nuser_usec 80\nsystem_usec 40\nnr_periods 5\n",
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
    assert_eq!(snapshot.memory_current_bytes, 4096);
    assert_eq!(snapshot.memory_peak_bytes, 8192);
}

#[test]
fn memory_peak_absent_is_zero_not_an_error() {
    let fixture = Fixture::new();
    fixture.write(
        "group/cpu.stat",
        "usage_usec 120\nuser_usec 80\nsystem_usec 40\n",
    );
    fixture.write("group/memory.current", "4096\n");

    let snapshot = CgroupReader::at_dir(fixture.root.join("group"))
        .snapshot()
        .expect("missing optional peak should parse");

    assert_eq!(snapshot.memory_peak_bytes, 0);
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
    fixture.write("group/cpu.stat", "user_usec 80\nsystem_usec 40\n");
    fixture.write("group/memory.current", "4096\n");

    let error = CgroupReader::at_dir(fixture.root.join("group"))
        .snapshot()
        .expect_err("required usage counter is absent");

    assert!(matches!(error, CgroupError::MissingField("usage_usec")));
}

#[test]
fn delta_rejects_a_backwards_counter_instead_of_reporting_zero() {
    // Given: a later snapshot whose cgroup CPU counters went backwards.
    let earlier = CgroupSnapshot {
        usage_usec: 100,
        user_usec: 80,
        system_usec: 20,
        memory_current_bytes: 10,
        memory_peak_bytes: 20,
    };
    let later = CgroupSnapshot {
        usage_usec: 10,
        user_usec: 8,
        system_usec: 2,
        memory_current_bytes: 5,
        memory_peak_bytes: 10,
    };

    // When: the counter delta is computed.
    let error = later
        .delta_since(&earlier)
        .expect_err("a counter rollback must invalidate the measurement");

    // Then: the first rolled-back counter is reported instead of favourable zero CPU.
    assert!(matches!(
        error,
        CgroupError::CounterRollback {
            field: "usage_usec",
            earlier: 100,
            later: 10,
        }
    ));
}
