# MinecraftAnalysis

`minecraft-analysis` is a registry-aware, loss-preserving converter for copies of
rule-selected Forge 1.2.5 or Forge 1.7.10 Anvil worlds targeting a
caller-supplied Forge 1.12.2 template world.
The older Python scripts remain in the repository as historical tools; they do
not provide the safety or typed-NBT guarantees of the Rust converter.

For a human-readable tour of the implementation, start with the
[architecture overview](docs/architecture/README.md). The accompanying
[profiles guide](docs/architecture/profiles.md) explains what a world profile
is, how it is detected, and how it differs from a world's registry catalog.

## Dumping standalone NBT

Dump the complete root compound of any standalone Java Edition NBT file as
deterministic, pretty-printed SNBT:

```sh
cargo run -p minecraft-analysis -- nbt dump /path/to/document.nbt
```

Inspect one stored block by its world-global coordinate, optionally selecting a
dimension and supplying source transformation rules for additional registry
context:

```bash
cargo run -p minecraft-analysis -- nbt dump --world /world --location=-1,64,4
cargo run -p minecraft-analysis -- nbt dump --world /world --location=-1,64,4 --dimension nether --rules rules.json
```

World-coordinate output is a stable labeled block record rather than a complete
chunk document. Registry names are resolved from the world's Forge registry,
optional repeatable `--rules` source manifests, and version-appropriate vanilla
defaults. An unknown numeric ID is still dumped and is labeled `unresolved`.
The former region-file `--chunk` and `--local-chunk` modes and dump-specific
`--source-rule` and `--target-rule` options have been removed.

The command writes SNBT to standard output with a trailing newline, making it
suitable for terminal inspection or shell redirection. It automatically reads
uncompressed, gzip-compressed, and zlib-compressed NBT and does not require the
world to match a supported conversion profile. The binary NBT root name is not
part of the emitted SNBT value.

All data is included recursively. In Forge worlds this includes the raw `FML`
registry structures and legacy registry keys; the command does not filter,
interpret, or normalize registry data. A read, decode, or render failure is
reported on standard error without writing a partial document to standard
output.

The former world-specific command `nbt level --world /worlds/example` has been
removed. To inspect the same data, pass the file directly:

```sh
cargo run -p minecraft-analysis -- nbt dump /worlds/example/level.dat
```

## Interactive NBT viewer

Open any standalone compound-rooted Java Edition NBT document in a read-only
terminal tree:

```sh
cargo run -p minecraft-analysis -- nbt view /path/to/document.nbt
```

Open the chunk containing a world-global block and highlight that block
initially. The dimension defaults to the overworld, and optional repeatable
rules enrich source block identities:

```bash
cargo run -p minecraft-analysis -- nbt view --world /world --location 12,64,9 --rules rules.json
```

The viewer automatically detects uncompressed, gzip-compressed, and
zlib-compressed input. Its header shows the source path, detected compression,
and binary root name. Use Up/`k` and Down/`j` to move, Left/`h` to collapse or
move to a parent, Right/`l` to expand or move to the first child, Enter to
toggle a container, and `q` or Escape to exit. Typed arrays appear as bounded
previews rather than one row per element.

The viewer is read-only and has no editing, mouse input, or export. Invalid
input and an unavailable requested block are diagnosed before raw mode or the
alternate screen is entered. Direct region-file selectors are no longer
supported; use `--world`, `--location`, and optional `--dimension` instead.

## Development

The supported development environment is the locked Nix flake:

```sh
nix develop
```

Run the same Rust checks as the independent GitHub Actions workflows from that
shell:

```sh
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
```

The CI workflows install stable Rust directly rather than using Nix. Cargo
continues to own Rust dependency resolution through the checked-in lockfile.
`nix flake check path:.` remains available as an aggregate check of the locked
development environment.

Install [zizmor](https://docs.zizmor.sh/installation/) and audit the GitHub
Actions workflows with the same enforced thresholds as CI:

```sh
zizmor --min-severity=medium --min-confidence=medium .github/workflows/
```

After the workflows have completed successfully on `master`, configure branch
protection to require the `Format`, `Check`, `Clippy`, `Test`, and `Zizmor`
status checks.

## Inputs and template creation

Always work from backups. The converter requires:

- a Forge 1.2.5 or Forge 1.7.10 Anvil source world;
- a separate world freshly created by the exact Forge 1.12.2 modpack that will
  load the result;
- a nonexistent output path on the same filesystem as its parent;
- one or more JSON rule documents. Schema 2 declares `source_profile`; existing
  schema-1 documents remain implicitly Forge 1.7.10.

Start the target modpack once, create and save a minimal world, exit cleanly,
and use that world as `--template`. Its persisted Forge registry snapshot is the
authority for target numeric assignments. The converter never loads mod jars.

## Dry run and explanation

```sh
cargo run -p minecraft-analysis -- dry-run \
  --source /worlds/source-1.7.10 \
  --template /worlds/template-1.12.2 \
  --output /worlds/converted \
  --rules rules/modpack.json \
  --report preflight.json
```

Dry-run performs full read-only discovery, registry resolution, rule coverage,
fingerprinting, and space estimation. It may write the requested report but
never creates the world output.

To inspect rule matching at one world-global block coordinate (the dimension
defaults to `overworld`):

```sh
cargo run -p minecraft-analysis -- explain \
  --source /worlds/source-1.7.10 \
  --template /worlds/template-1.12.2 \
  --rules rules/modpack.json \
  --location '12,64,-3' \
  --dimension nether
```

`--dimension` accepts `overworld`, `nether`, `end`, or an existing safe
`DIM...` modded-dimension directory. Horizontal coordinates use Euclidean
division, so negative positions select the mathematically containing chunk and
region. Explain reads only that region's selected chunk and reports the terrain
block, its block entity, and direct or recursively nested inventory items owned
by that block entity. It does not scan unrelated regions, entities, or
standalone NBT files.

## Rule coverage authoring

Before choosing a target template, inventory a rule-selected Forge source world for
blocks and items that still need transformation rules:

```sh
cargo run -p minecraft-analysis -- rules coverage \
  --world /worlds/source-1.7.10 \
  --rules rules/modpack.json \
  --report coverage.json
```

The source and rule documents are read-only. Verified vanilla assignments for
the selected profile have built-in migration coverage and do not need explicit rules.
Non-vanilla blocks and items require a selected rule even when their registry
name could otherwise pass through unchanged.

The JSON report groups exact uncovered signatures and includes numeric identity,
metadata or damage, item count, complete typed SNBT, rule rejection traces, raw
observation counts, and up to the first five distinct world/NBT locations in
canonical order. Multiple observations of the same location count separately.
For blocks, a colocated block entity and its SNBT are included when present.
Associated block-entity SNBT omits the top-level lowercase `x`, `y`, and `z`
fields so otherwise identical blocks group together; their coordinates remain
available in each structured report location. Nested or differently cased
coordinate-like fields remain part of the signature.

Coverage exits with status 0 when complete, 1 after successfully reporting
uncovered objects, and 2 for invalid inputs or execution failures. Standard item
locations and custom nested paths declared by selected rules are scanned. Since
arbitrary mods can store items elsewhere, every report warns that undeclared
mod-specific paths cannot be proven covered.

Use the report to extend the rules and repeat coverage until it exits 0. Then
run the target-aware `dry-run` to validate target identities and conversion
policy; source-only coverage does not replace that step.

## Conversion

Use the same inputs with `convert`. A dry run is recommended when a complete
read-only compatibility report is useful, but conversion does not require or
repeat a preflight pass:

```sh
cargo run -p minecraft-analysis -- convert \
  --source /worlds/source-1.7.10 \
  --template /worlds/template-1.12.2 \
  --output /worlds/converted \
  --rules rules/modpack.json
```

### Region concurrency

Analysis-only commands and conversion process independent region files in
parallel by default. During conversion, the same worker reads, transforms, and
writes its assigned region before reporting success.
The worker count comes from the CPU parallelism available
to the process, including operating-system affinity or container limits. Chunks
inside each region remain sequential, and standalone NBT files and final
publication are not added to the region worker pool.

Use the global `--jobs` option to override the worker count. One job preserves
sequential region processing:

```console
cargo run -p minecraft-analysis -- --jobs 1 dry-run \
  --source /worlds/source \
  --template /worlds/forge-1.12.2-template \
  --output /worlds/converted \
  --rules rules.json
```

Each active conversion worker can retain complete source and output region
buffers plus one decoded chunk. Lower `--jobs` values reduce peak memory and may
also perform better on storage that handles concurrent reads poorly; higher
values do not guarantee linear speedup.

The converter writes into a clearly named sibling staging directory. Each
transformed NBT or region file is checked against the target catalogs while it
is transformed, then encoded, flushed, and committed through a temporary
sibling. Publication occurs only after every work unit succeeds, by one
same-filesystem rename. Source and template files are opened read-only and
remain unchanged. Conversion emits no report document; use `dry-run` for a
complete compatibility report, `rules coverage` for rule completeness, and
`explain` for location-specific rule traces.

## Rule authoring

See [`examples/rules/example.json`](examples/rules/example.json). Documents carry
`schema_version`, a stable `rule_set`, a schema-2 `source_profile`, optional imports/manifests, and uniquely
identified rules. Rules can target blocks, items, entities, and block entities;
match exact/masked/ranged metadata and typed NBT; patch values without passing
through lossy JSON representations; and declare mod-specific nested item paths.

Loss is opt-in. Deletion, air replacement, dropped items, NBT discard, clamping,
and unrelated substitution require an explicit selected rule. Missing mappings
fail the active conversion work unit by default. Registry contradictions require an explicit world or
manifest selection, except that authoritative Forge 1.2.5 vanilla assignments
cannot be replaced. Forge 1.2.5 manifests must separately declare every modpack
block and item numeric assignment.

## Reports

Analysis reports are JSON containing the resolved source profile, input audit metadata, rule-set IDs,
registry mappings, disposition counts, object locations, selected rules and full
decision traces, warnings, file dispositions, and outcome. `unresolved` objects
in a dry run predict conversion failure. Locations retain file, dimension, chunk, block coordinate,
and NBT path where applicable.

Parallel analysis contributions are appended as workers complete. Report schemas,
records, counts, diagnostics, and decisions are equivalent across valid worker
schedules, but top-level report array order and raw JSON bytes may differ.
Consumers should compare those collections by their logical record keys rather
than relying on array position or hashing the complete serialized report.

Direct conversion emits no report. When a fatal conversion error occurs, the
CLI emits an actionable diagnostic and does not publish the output world.

## Interrupted runs and recovery

For output `/worlds/converted`, staging is named
`/worlds/.converted.minecraft-analysis-staging`. An interruption can leave that
directory or `*.minecraft-analysis-tmp` siblings. They are intentionally never
deleted automatically. Inspect them, retain anything needed for diagnosis, then
remove the exact staging directory explicitly before retrying from untouched
inputs. A final output path is never treated as resumable and must not exist.

## Validation boundary

Successful conversion means all mutation-time registry checks, encoding,
writing, flushing, staging commits, and final publication succeeded. It does
not reopen output files or prove that arbitrary mod code accepts historical NBT semantics. Test a
converted copy with the exact caller-provided Forge 1.12.2 installation and
modpack before adopting it. Game and mod artifacts are never downloaded or
bundled by this project.
