## Why

The project has no pull-request automation to enforce Rust quality checks before changes merge and no automated dependency-update process. A standard GitHub-native CI baseline will provide focused feedback for each gate, continuously validate workflow security, and keep Rust and Actions dependencies current.

## What Changes

- Add independent GitHub Actions workflows for Rust formatting, compilation checks, Clippy, and workspace tests on pull requests and default-branch pushes.
- Install the standard Rust toolchain and required components directly on GitHub-hosted runners; CI will not depend on Nix.
- Require Cargo dependency resolution to remain locked during CI validation.
- Add an independent zizmor workflow that statically analyzes all GitHub Actions workflows.
- Add Dependabot configuration for Cargo and GitHub Actions dependencies.
- Apply concurrency cancellation, least-privilege permissions, and immutable action pinning as baseline workflow hygiene.
- Document commands contributors can use to reproduce each Rust check locally.

## Capabilities

### New Capabilities

- `continuous-integration`: Defines independent automated Rust quality and workflow-security gates, their execution conditions, dependency maintenance, and failure reporting.

### Modified Capabilities

None.

## Impact

- Adds separate workflow files and `.github/dependabot.yml` under `.github/`.
- Updates contributor-facing development documentation.
- Uses GitHub-hosted Linux runners, standard Rust installation tooling, the checked-in `Cargo.lock`, and zizmor; the existing Nix development environment remains available but is not part of GitHub Actions.
- Creates automated pull requests for Cargo and GitHub Actions dependency updates; no runtime APIs or application behavior change.
