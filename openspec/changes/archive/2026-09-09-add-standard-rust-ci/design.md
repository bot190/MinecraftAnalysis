## Context

See `proposal.md` for motivation. The Rust workspace declares Rust 1.85 as its minimum supported version and has no existing GitHub Actions or Dependabot configuration. The repository also provides a locked Nix development environment, but the requested CI architecture intentionally uses the normal Rust installation and command-line tools directly on GitHub-hosted Linux runners.

## Goals / Non-Goals

**Goals:**

- Give formatting, compilation, Clippy, tests, and workflow security separate workflow files and stable status checks.
- Use standard Rust toolchain installation and direct Cargo/rustfmt commands in CI.
- Keep Cargo resolution locked and cache build inputs without making correctness depend on a cache.
- Continuously scan workflow definitions with zizmor.
- Keep Cargo crates and pinned GitHub Actions current through Dependabot pull requests.

**Non-Goals:**

- Running Nix or validating the Nix flake in GitHub Actions.
- Cross-platform testing, code coverage thresholds, release packaging, deployment, benchmarks, or nightly Rust testing.
- Automatic merging of dependency updates.
- Adding dependency license or vulnerability policy beyond GitHub's existing dependency alerts; tools such as `cargo-deny` can be proposed separately.

## Decisions

### Use one workflow file per check

Add independent workflow files for `format`, `check`, `clippy`, `test`, and `zizmor`. Each listens for pull requests to and pushes on `master`, matching the current remote default branch. Each workflow declares its own concurrency group and cancellation behavior so a new revision supersedes only stale runs of that same workflow and ref.

A job matrix or one workflow containing multiple jobs was rejected because the requested contract is isolation at the GitHub Actions workflow level. Separate workflows make trigger behavior, permissions, logs, and branch-protection results explicit and allow each gate to run or fail independently.

### Install Rust directly and invoke conventional commands

The Rust workflows use a pinned Rust toolchain installer action and a pinned Cargo cache action. Formatting installs `rustfmt` and runs `cargo fmt --all -- --check`. Clippy installs `clippy` and runs `cargo clippy --workspace --all-targets --locked -- -D warnings`. Compilation and tests use `cargo check --workspace --all-targets --locked` and `cargo test --workspace --all-targets --locked` respectively.

Use the stable toolchain rather than Nix so CI follows common Rust project conventions. The workspace's `rust-version = "1.85"` continues to define package compatibility, while stable CI catches regressions against the currently supported stable compiler. A separate MSRV matrix is outside this baseline.

The alternative was to install exactly Rust 1.85. That would test the declared minimum but miss compatibility with evolving stable tooling; testing both belongs in a future toolchain matrix if MSRV enforcement becomes a project requirement.

### Pin actions and let Dependabot update them

Every `uses:` reference is pinned to a full commit SHA and annotated with its human-readable release. Dependabot's `github-actions` ecosystem updater checks the repository root weekly and proposes new pinned revisions. This combines immutable execution with routine upgrades.

Floating major tags were rejected because they allow third-party workflow code to change without a repository commit. Manual-only upgrades were rejected because immutable pins otherwise become stale easily.

### Run zizmor in an independent least-privilege workflow

The zizmor workflow checks out the repository and invokes the official zizmor action, pinned by SHA, against `.github/workflows/`. It runs on the same pull-request and default-branch events as the Rust workflows and enforces findings at the configured baseline severity. Its token permissions remain read-only; emitting a GitHub code-scanning SARIF report is not required for the baseline because that would require additional permissions. Findings remain available in the workflow log and annotations supported by the action.

The official action is preferred over ad hoc package installation because it provides a focused, reproducible Actions integration. A single zizmor workflow avoids recursively adding zizmor steps to every other workflow.

### Configure Dependabot for Cargo and GitHub Actions

Add `.github/dependabot.yml` version 2 with weekly update entries for the `cargo` ecosystem at `/` and the `github-actions` ecosystem at `/`, both targeting `master`. Set explicit pull-request limits and group compatible development dependency updates where supported, while allowing security updates to remain independently actionable. Dependabot updates Cargo manifests and `Cargo.lock`; its pull requests must pass the same independent CI workflows as contributor changes.

Renovate and custom scheduled update scripts were considered, but Dependabot is GitHub-native, covers both requested ecosystems, and requires no additional credentials or hosted service.

### Keep permissions and diagnostics explicit

Each workflow declares `permissions: contents: read`, avoids `pull_request_target`, uses non-privileged pull-request execution, and prints the direct tool output. Each workflow's concurrency key includes its workflow name plus the pull request number or Git ref. This avoids cross-cancelling different checks while removing stale work for updated revisions.

## Risks / Trade-offs

- [Five workflows repeat checkout, toolchain setup, and cache restoration] → Accept modest duplication in exchange for independent status checks; use the same cache implementation and keys so compatible artifacts can be reused safely.
- [Stable Rust can move independently of repository changes] → Keep `rust-version` as the compatibility declaration and consider a pinned `rust-toolchain.toml` or MSRV matrix later if exact compiler reproducibility becomes more important.
- [Caching build outputs between independent workflows can increase storage or cause lock contention] → Cache Cargo registry/git data and target artifacts through a standard Rust cache action, but treat cache misses as normal and never as correctness failures.
- [zizmor can introduce new findings after an analyzer update] → Pin its action and update it through reviewed Dependabot pull requests, so policy changes arrive as visible repository changes.
- [Dependabot may create noisy pull requests] → Use a weekly cadence, sensible open-PR limits, and compatible update groups; do not enable automatic merging.
- [Hard-coding `master` can drift if the default branch is renamed] → Treat a branch rename as a coordinated update to workflow triggers and Dependabot targets.

## Migration Plan

1. Add the five workflow files with pinned actions, direct Rust commands, read-only permissions, and independent concurrency controls.
2. Add Dependabot configuration for Cargo and GitHub Actions.
3. Update development documentation with each local Rust and zizmor command.
4. Validate the workflow files with zizmor and run all four Rust commands in the repository's supported local development environment before merging.
5. After the first successful default-branch run, configure branch protection to require the stable format, check, Clippy, test, and zizmor workflow results.

Rollback consists of removing the workflow and Dependabot files and reverting the documentation update; no application data or runtime migration is involved.
