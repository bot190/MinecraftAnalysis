## 1. Add Independent Rust Workflows

- [x] 1.1 Add `.github/workflows/format.yml` with direct stable Rust/rustfmt installation and `cargo fmt --all -- --check`; verify the workflow has its own pull-request and `master` push triggers, read-only permissions, concurrency cancellation, and immutable action pins.
- [x] 1.2 Add `.github/workflows/check.yml` with direct stable Rust installation and `cargo check --workspace --all-targets --locked`; verify the command succeeds without modifying `Cargo.lock` and the workflow is independent and least-privileged.
- [x] 1.3 Add `.github/workflows/clippy.yml` with direct stable Rust/Clippy installation and `cargo clippy --workspace --all-targets --locked -- -D warnings`; verify the command succeeds and Clippy warnings fail this workflow alone.
- [x] 1.4 Add `.github/workflows/test.yml` with direct stable Rust installation and `cargo test --workspace --all-targets --locked`; verify the command succeeds and the workflow exposes a separate test result.
- [x] 1.5 Configure pinned standard Cargo caching consistently in the check, Clippy, and test workflows; verify a cache miss does not prevent any command from running.

## 2. Validate Workflow Security

- [x] 2.1 Add `.github/workflows/zizmor.yml` using the official zizmor action pinned to a full commit SHA and scan all files under `.github/workflows/`; verify a clean scan succeeds and an enforced test finding causes only the zizmor workflow to fail.
- [x] 2.2 Review every workflow for `contents: read`, absence of `pull_request_target`, full-SHA `uses:` references with release comments, and workflow-specific concurrency keys; verify zizmor accepts the final workflow set at the configured severity.

## 3. Configure Dependency Maintenance

- [x] 3.1 Add `.github/dependabot.yml` version 2 with weekly Cargo updates for `/`, targeting `master`, an explicit open-pull-request limit, and compatible development-update grouping; verify the file passes Dependabot's schema requirements and includes lockfile-managed Rust dependencies.
- [x] 3.2 Add a weekly GitHub Actions updater for `/`, targeting `master` with an explicit open-pull-request limit; verify Dependabot can discover every pinned action reference across the five workflows.
- [x] 3.3 Confirm Dependabot configuration does not enable automatic merging and that its pull requests will trigger all independent pull-request workflows.

## 4. Document and Validate the Baseline

- [x] 4.1 Update the README development section with the exact local format, check, Clippy, test, and zizmor commands; verify every documented command matches its corresponding workflow command.
- [x] 4.2 Run the four Rust validation commands inside the repository's required Nix development environment and verify formatting, compilation, Clippy, and tests pass without changing `Cargo.lock`.
- [x] 4.3 Run zizmor against the completed `.github/workflows/` directory and verify no finding at the enforced severity remains.
- [x] 4.4 Verify the five workflow names produce distinct format, check, Clippy, test, and zizmor statuses suitable for branch protection, and document those required status names for maintainers.
