# CI hygiene

The repository's supported CI path is Rust-only.

- `.github/workflows/rust-ci.yml` is the canonical quality gate.
- Legacy CMake CI has been removed.
- Dependabot tracks Cargo and GitHub Actions dependencies.
- The intended required status check for `main` is `Rust CI / quality-gate`.

This note exists to make the repository-level CI contract explicit while branch protection is configured in GitHub settings.
