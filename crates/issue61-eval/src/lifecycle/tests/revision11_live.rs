use crate::lifecycle::live::NativeLaunchContract;
use crate::Dataset;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

const NATIVE_IMAGE: &str =
    "debian@sha256:88200866dfff7ea7f5cbcb6ec7c8a701889efe6fe859fe64d6990e4b07ea4171";
const NATIVE_NAME: &str = "i61-native-live";

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TemporaryDirectory(PathBuf);

impl TemporaryDirectory {
    fn create() -> Self {
        loop {
            let nonce = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "issue61-live-adapter-test-{}-{nonce}",
                std::process::id()
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("failed to create isolated directory {path:?}: {error}"),
            }
        }
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("isolated directory cleanup succeeds");
    }
}

#[test]
fn native_launch_contract_is_exact_for_each_dataset() {
    // Given
    let repository = Path::new("/canonical/repository");

    // When
    let wands = NativeLaunchContract::for_dataset(repository, Dataset::Wands);
    let esci = NativeLaunchContract::for_dataset(repository, Dataset::EsciElectronics);

    // Then
    for contract in [&wands, &esci] {
        assert_eq!(contract.image(), NATIVE_IMAGE);
        assert_eq!(contract.container_name(), NATIVE_NAME);
        assert_eq!(contract.cpus(), "3");
        assert_eq!(contract.cpuset_cpus(), "0-2");
        assert_eq!(contract.memory(), "6g");
        assert_eq!(contract.memory_swap(), "6g");
        assert_eq!(contract.port_binding(), (9900, 9900));
        assert_eq!(contract.endpoint(), "http://127.0.0.1:9900");
        assert_eq!(contract.readiness_path(), "/ping");
        assert_eq!(contract.readiness_interval(), Duration::from_secs(1));
        assert_eq!(contract.readiness_timeout(), Duration::from_secs(90));
        assert_eq!(contract.teardown_target(), NATIVE_NAME);
        assert_eq!(contract.teardown_timeout(), Duration::from_secs(30));
        assert_eq!(
            contract.binary_mount(),
            (
                repository.join("target/release/i61_native_server"),
                PathBuf::from("/opt/i61_native_server"),
                true,
            )
        );
    }
    assert_eq!(
        wands.dataset_mount(),
        (
            repository.join("dataset_cache/wands"),
            PathBuf::from("/dataset"),
            true,
        )
    );
    assert_eq!(
        wands.argv(),
        [
            "/opt/i61_native_server",
            "--catalog",
            "/dataset/catalog.jsonl",
            "--dataset",
            "wands",
            "--port",
            "9900",
        ]
    );
    assert_eq!(
        esci.dataset_mount(),
        (
            repository.join("dataset_cache/esci_electronics"),
            PathBuf::from("/dataset"),
            true,
        )
    );
    assert_eq!(
        esci.argv(),
        [
            "/opt/i61_native_server",
            "--catalog",
            "/dataset/esci_electronics_products.jsonl",
            "--dataset",
            "esci_electronics",
            "--port",
            "9900",
        ]
    );
}

#[test]
fn native_cgroup_is_derived_from_the_inspected_pid() {
    // Given
    let fixture = TemporaryDirectory::create();
    let proc_root = fixture.path().join("proc");
    let cgroup_mount = fixture.path().join("sys/fs/cgroup");
    std::fs::create_dir_all(proc_root.join("4242")).expect("fake proc PID directory is created");
    std::fs::create_dir_all(&cgroup_mount).expect("fake cgroup mount is created");
    std::fs::write(
        proc_root.join("4242/cgroup"),
        "0::/system.slice/docker-native.scope\n",
    )
    .expect("fake proc cgroup identity is written");
    let contract =
        NativeLaunchContract::for_dataset(Path::new("/canonical/repository"), Dataset::Wands);

    // When
    let reader = contract
        .cgroup_reader_for_pid(4242, &proc_root, &cgroup_mount)
        .expect("PID cgroup identity resolves");

    // Then
    assert_eq!(
        reader.dir(),
        cgroup_mount.join("system.slice/docker-native.scope")
    );
}
