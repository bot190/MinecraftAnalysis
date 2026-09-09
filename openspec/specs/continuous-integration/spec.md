# Continuous Integration Specification

## Purpose

Defines the automated quality gates that protect the Rust workspace and provide reproducible feedback before changes are merged.

## Requirements

### Requirement: CI validates proposed and integrated changes
The project SHALL run its standard validation suite for every pull request targeting the default branch and every push to the default branch.

#### Scenario: Pull request validation
- **WHEN** a pull request targets the default branch
- **THEN** CI runs the complete standard validation suite against the proposed revision

#### Scenario: Default branch validation
- **WHEN** a commit is pushed to the default branch
- **THEN** CI runs the complete standard validation suite against that commit

### Requirement: CI enforces the Rust quality baseline
The standard validation suite SHALL verify workspace formatting, compile all workspace targets, run Clippy for all workspace targets with warnings treated as errors, and run all workspace tests.

#### Scenario: Valid Rust change
- **WHEN** a revision is formatted, compiles across all workspace targets, produces no Clippy warnings, and passes all workspace tests
- **THEN** every Rust quality check succeeds

#### Scenario: Quality regression
- **WHEN** any required formatting, compilation, lint, or test validation fails
- **THEN** CI reports a failing named check and the revision does not satisfy the standard validation suite

### Requirement: Rust checks use standard Rust tooling
Each Rust validation workflow SHALL install the standard Rust toolchain and only the components it needs on a GitHub-hosted runner, invoke the corresponding Cargo or rustfmt command directly, and SHALL NOT require Nix.

#### Scenario: Rust workflow starts on a clean runner
- **WHEN** CI validates a revision
- **THEN** each Rust workflow installs its required Rust toolchain components and invokes its validation command without entering a Nix environment

### Requirement: CI preserves locked Cargo resolution
Dependency-resolving CI checks SHALL use the checked-in Cargo lockfile and SHALL fail rather than update dependency resolution during validation.

#### Scenario: Stale Cargo lockfile
- **WHEN** a manifest change would require `Cargo.lock` to be updated
- **THEN** the affected CI check fails without modifying the lockfile

### Requirement: CI checks execute independently
Formatting, compilation, Clippy, tests, and zizmor SHALL each be implemented as a separate GitHub Actions workflow with its own named result and diagnostic output.

#### Scenario: Individual check fails
- **WHEN** one quality or workflow-security gate fails
- **THEN** that workflow reports its own failure and the other check workflows can complete independently

### Requirement: GitHub Actions workflows pass zizmor analysis
The project SHALL run zizmor against all repository GitHub Actions workflows for pull requests and default-branch pushes.

#### Scenario: Workflow security issue is introduced
- **WHEN** a workflow change introduces a finding at the configured enforced severity
- **THEN** the independent zizmor workflow fails and reports the affected workflow and finding

#### Scenario: Workflows satisfy the security policy
- **WHEN** zizmor finds no issue at the configured enforced severity
- **THEN** the zizmor workflow succeeds

### Requirement: Dependabot maintains project dependencies
The project SHALL configure Dependabot to check Cargo dependencies and GitHub Actions dependencies on a recurring schedule and open update pull requests against the default branch.

#### Scenario: Cargo dependency update is available
- **WHEN** Dependabot detects an eligible newer Cargo dependency version
- **THEN** it can open a pull request updating the Rust dependency manifests and lockfile

#### Scenario: GitHub Action update is available
- **WHEN** Dependabot detects an eligible newer version of a referenced GitHub Action
- **THEN** it can open a pull request updating the pinned action reference

### Requirement: CI follows safe workflow defaults
The workflow SHALL grant no write permissions, SHALL prevent duplicate in-progress runs for the same pull request or branch, and SHALL allow newer revisions to supersede older in-progress revisions.

#### Scenario: Pull request receives a new commit
- **WHEN** CI is still running for an older revision of the same pull request
- **THEN** the older run is cancelled and validation continues for the newest revision

#### Scenario: Workflow executes untrusted repository content
- **WHEN** CI runs for a pull request
- **THEN** the workflow has read-only access to repository contents and no write-capable token permissions

### Requirement: Contributors can reproduce CI locally
The development documentation SHALL identify the standard Rust commands that reproduce formatting, compilation, Clippy, and test validation locally, and SHALL identify how to run zizmor against the workflows.

#### Scenario: Contributor checks a revision locally
- **WHEN** a contributor follows the documented CI reproduction instructions on a supported system
- **THEN** they can execute each CI gate independently with the documented tool and command
