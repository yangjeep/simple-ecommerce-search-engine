use issue61_eval::NativePidIdentity;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct ProcFixture {
    root: PathBuf,
}

impl ProcFixture {
    fn new() -> Self {
        let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("issue61-proc-status-{}-{id}", std::process::id()));
        std::fs::create_dir_all(&root).expect("fixture root should be created");
        Self { root }
    }

    fn root(&self) -> &Path {
        &self.root
    }

    fn write_status(&self, host_pid: u32, status: &str) {
        let process_dir = self.root.join(host_pid.to_string());
        std::fs::create_dir_all(&process_dir).expect("process fixture should be created");
        std::fs::write(process_dir.join("status"), status)
            .expect("status fixture should be written");
    }
}

impl Drop for ProcFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn maps_cgroup_host_pid_to_last_nspid_value() {
    let fixture = ProcFixture::new();
    fixture.write_status(2_334_505, "Name:\ti61_native\nNSpid:\t2334505\t1\n");

    let identity = NativePidIdentity::read(fixture.root(), 2_334_505, 1)
        .expect("observed Docker PID mapping should be accepted");

    assert_eq!(identity.cgroup_host_pid, 2_334_505);
    assert_eq!(identity.pid_namespace, 1);
}

#[test]
fn rejects_missing_malformed_duplicate_and_empty_nspid() {
    let cases = [
        ("Name:\ti61_native\n", "missing"),
        ("NSpid:\t2334505\tbad\n", "malformed"),
        ("NSpid:\t2334505\t1\nNSpid:\t2334505\t1\n", "duplicate"),
        ("NSpid:\t\n", "empty"),
    ];

    for (status, case) in cases {
        let fixture = ProcFixture::new();
        fixture.write_status(2_334_505, status);

        assert!(
            NativePidIdentity::read(fixture.root(), 2_334_505, 1).is_err(),
            "{case} NSpid must be rejected"
        );
    }
}

#[test]
fn rejects_missing_proc_status_and_endpoint_pid_mismatch() {
    let fixture = ProcFixture::new();
    assert!(NativePidIdentity::read(fixture.root(), 2_334_505, 1).is_err());

    fixture.write_status(2_334_505, "NSpid:\t2334505\t1\n");
    assert!(NativePidIdentity::read(fixture.root(), 2_334_505, 2).is_err());
}

#[test]
fn requires_stable_host_and_namespace_pids_across_boundaries() {
    let before = NativePidIdentity {
        cgroup_host_pid: 2_334_505,
        pid_namespace: 1,
    };

    assert!(before.ensure_stable(&before).is_ok());
    assert!(before
        .ensure_stable(&NativePidIdentity {
            cgroup_host_pid: 2_334_506,
            pid_namespace: 1,
        })
        .is_err());
    assert!(before
        .ensure_stable(&NativePidIdentity {
            cgroup_host_pid: 2_334_505,
            pid_namespace: 2,
        })
        .is_err());
}
