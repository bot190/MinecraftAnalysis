## 1. Mutation-Time Safety and Diagnostics

- [x] 1.1 Audit terrain, ordinary item, nested item, entity-held item, block-entity inventory, and standalone inventory conversion paths so every emitted block and item is checked against the target catalog before encoding; add focused missing-target and numeric-range tests and verify them with `nix develop -c cargo test -p minecraft-analysis-core`.
- [x] 1.2 Propagate normalized file, dimension, chunk, block or typed NBT path, source identity or numeric representation, and applicable rule-chain context through direct conversion failures; verify focused core and CLI tests show actionable errors without consulting a migration report and refuse publication.

## 2. Report-Free Direct Conversion

- [x] 2.1 Separate direct-conversion CLI inputs from analysis report inputs, remove `--report` and JSON output from `convert`, and verify CLI parsing and success tests prove `dry-run`, `rules coverage`, and `explain` retain their machine-readable output contracts.
- [x] 2.2 Remove chunk and standalone source assessment, `ReportContribution` creation, disposition records, validation-finding transfer, and report merging from direct conversion while retaining those facilities for analysis-only commands; verify focused core tests prove conversion decisions and staged bytes remain equivalent.
- [x] 2.3 Simplify region worker results and coordinator reduction so successful conversion transfers only state needed for staged-file commit, and verify sequential and parallel tests retain bounded scheduling, deterministic primary failures, equivalent staged bytes, and atomic publication.
- [x] 2.4 Remove conversion report initialization, finalization, serialization, failure-report handling, and now-unused conversion-only report APIs without removing dry-run report support; verify `nix develop -c cargo test --workspace` compiles and passes.

## 3. Verification-Free Staging

- [x] 3.1 Remove post-write reopen and semantic verification from region, standalone NBT, and merged `level.dat` work units so successful encode, write, and flush make a temporary file eligible for coordinator commit; verify failure-injection tests still prevent publication on codec, write, flush, and commit errors.
- [x] 3.2 Remove opaque destination digest rereads while ensuring classification and copying use the same captured source bytes; verify tests cover byte-preserving opaque copy, strict malformed NBT failure, and source-capture consistency.
- [x] 3.3 Delete the production verification module and obsolete verification progress/error plumbing when no runtime callers remain, while retaining or adding NBT and region round-trip, compression, extended-ID, metadata-limit, oversized-chunk, and end-to-end output tests; verify `nix develop -c cargo test -p minecraft-analysis-core` passes.

## 4. Progress and Documentation

- [x] 4.1 Update conversion progress so region completion means transformation plus temporary output writing and publication is the only subsequent conversion activity; verify interactive-controller and CLI tests contain no conversion verification or report-generation lifecycle.
- [x] 4.2 Update the README and architecture documentation to remove conversion report and local verification guarantees, explain the dry-run/coverage/explain/convert responsibility split, and document retained staging, error, and publication safety; verify documented commands and behavior agree with the delta specifications.
- [x] 4.3 Update resource-bound documentation to remove conversion report spool, report-transfer, verified-path, and reopen state while preserving analysis report and active-worker bounds; verify the documented bounds match implementation constants and worker behavior.

## 5. Regression and Performance Validation

- [x] 5.1 Add end-to-end CLI regression coverage proving successful direct conversion emits no report document, rejects the removed report option, publishes valid expected fixtures, and leaves dry-run, coverage, and explain output unchanged; verify the focused integration tests pass inside `nix develop`.
- [x] 5.2 Add sequential and multi-worker regression coverage for successful publication, canonical failure selection, diagnostic staging, opaque pass-through, and no commit after the failure boundary; verify the relevant core and CLI suites pass inside `nix develop`.
- [x] 5.3 Run `nix develop -c cargo test --workspace`, `nix develop -c cargo clippy --workspace --all-targets -- -D warnings`, and `nix develop -c cargo fmt --all -- --check`, then record any representative release-build timing comparison available for conversion before and after removal.
- [x] 5.4 Run `openspec validate remove-conversion-report-and-verification --strict` and confirm every changed capability, breaking CLI behavior, retained analysis contract, and removed verification guarantee is represented coherently before implementation is considered complete.
