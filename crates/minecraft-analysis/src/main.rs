use std::fs;
use std::io::{BufWriter, IsTerminal, Write};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use clap::{ArgGroup, Args, Parser, Subcommand, ValueEnum};
use miette::{miette, IntoDiagnostic};
use minecraft_analysis_core::coverage;
use minecraft_analysis_core::manifest_authoring::{self, ManifestSide};
use minecraft_analysis_core::profile::{self, WorldProfile};
use minecraft_analysis_core::registry::{self, VanillaVersion};
use minecraft_analysis_core::rules::{self, SourceProfile};
use minecraft_analysis_core::{nbt, world};

mod nbt_context;
mod nbt_input;
mod nbt_viewer;
mod progress;

#[derive(Debug, Parser)]
#[command(author, version, about)]
struct Cli {
    #[arg(long, global = true)]
    verbose: bool,
    /// Disable interactive progress rendering.
    #[arg(long, global = true)]
    no_progress: bool,
    /// Maximum region files processed concurrently (defaults to available CPUs).
    #[arg(long, global = true, value_name = "N")]
    jobs: Option<NonZeroUsize>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Convert and transactionally publish a world.
    Convert(ConvertInputs),
    /// Explain rule evaluation for one world-global block coordinate.
    Explain {
        #[command(flatten)]
        inputs: ExplainInputs,
        /// World-global block coordinate formatted as `x,y,z`.
        #[arg(long, value_name = "X,Y,Z", allow_hyphen_values = true)]
        location: BlockCoordinate,
        /// World dimension: overworld, nether, end, or an existing DIM... directory.
        #[arg(long, default_value = "overworld")]
        dimension: world::DimensionId,
    },
    /// Inspect raw NBT data from a world.
    Nbt {
        #[command(subcommand)]
        command: NbtCommand,
    },
    /// Analyze and author transformation rules.
    Rules {
        #[command(subcommand)]
        command: RulesCommand,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BlockCoordinate([i32; 3]);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CoordinatePair {
    source: [i32; 3],
    target: [i32; 3],
}

impl FromStr for CoordinatePair {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let parts = value.split(':').collect::<Vec<_>>();
        if parts.len() != 2 {
            return Err("expected exactly one `:` separating source and target coordinates".into());
        }
        let parse = |side: &str, value: &str| {
            value
                .parse::<BlockCoordinate>()
                .map(|coordinate| coordinate.0)
                .map_err(|error| format!("invalid {side} coordinate: {error}"))
        };
        Ok(Self {
            source: parse("source", parts[0])?,
            target: parse("target", parts[1])?,
        })
    }
}

impl FromStr for BlockCoordinate {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.contains(':') {
            return Err(format!(
                "invalid world coordinate {value:?}; expected x,y,z (the old file:x,y,z syntax is no longer supported)"
            ));
        }
        let parts: Vec<_> = value.split(',').collect();
        if parts.len() != 3 {
            return Err(format!(
                "invalid world coordinate {value:?}; expected exactly x,y,z (the old file:x,y,z syntax is no longer supported)"
            ));
        }
        let mut coordinate = [0; 3];
        for (index, part) in parts.into_iter().enumerate() {
            coordinate[index] = part.parse::<i32>().map_err(|_| {
                format!("invalid world coordinate {value:?}; each x,y,z component must be a signed 32-bit integer")
            })?;
        }
        Ok(Self(coordinate))
    }
}

#[derive(Debug, Subcommand)]
enum RulesCommand {
    /// Report source blocks and items not covered by vanilla migration or rules.
    Coverage(CoverageInputs),
    /// Update one rule manifest from a runtime-generated numeric ID map.
    UpdateManifest(UpdateManifestInputs),
    /// Infer an exact rule from one paired source and target world coordinate.
    Infer(InferInputs),
}

#[derive(Clone, Debug, Args)]
struct InferInputs {
    #[arg(long)]
    rules: PathBuf,
    #[arg(long)]
    source_world: PathBuf,
    #[arg(long)]
    target_world: PathBuf,
    #[arg(
        long,
        value_name = "SOURCE_X,SOURCE_Y,SOURCE_Z:TARGET_X,TARGET_Y,TARGET_Z",
        allow_hyphen_values = true
    )]
    coordinate: CoordinatePair,
    #[arg(long, default_value = "overworld")]
    source_dimension: world::DimensionId,
    #[arg(long, default_value = "overworld")]
    target_dimension: world::DimensionId,
    #[arg(long)]
    rule_id: Option<String>,
}

#[derive(Clone, Debug, Args)]
struct UpdateManifestInputs {
    /// Rule document to update in place.
    #[arg(long)]
    rules: PathBuf,
    /// Runtime-generated numeric ID map.
    #[arg(long)]
    id_map: PathBuf,
    /// Manifest within the rule document to update.
    #[arg(long, value_enum)]
    manifest: ManifestSelection,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum ManifestSelection {
    Source,
    Target,
}

impl From<ManifestSelection> for ManifestSide {
    fn from(value: ManifestSelection) -> Self {
        match value {
            ManifestSelection::Source => Self::Source,
            ManifestSelection::Target => Self::Target,
        }
    }
}

#[derive(Debug, Subcommand)]
enum NbtCommand {
    /// Print one standalone NBT document's root compound as SNBT.
    Dump(DumpInputs),
    /// Inspect one standalone NBT document interactively.
    View {
        /// Standalone NBT file, or region file when a selector is supplied.
        file: PathBuf,
        #[command(flatten)]
        selector: nbt_input::ChunkSelectorArgs,
        /// Transformation rule document used to enrich source block identities.
        #[arg(long)]
        rules: Vec<PathBuf>,
    },
}

#[derive(Clone, Debug, Args)]
#[command(group(ArgGroup::new("dump_input").required(true).multiple(false).args(["file", "world"])))]
#[command(group(ArgGroup::new("rule_side").multiple(false).args(["source_rule", "target_rule"])))]
struct DumpInputs {
    /// Standalone NBT file, or region file when a selector is supplied.
    #[arg(conflicts_with_all = ["location", "source_rule", "target_rule"])]
    file: Option<PathBuf>,
    /// Global chunk coordinates in x,z form for positional region-file mode.
    #[arg(long, value_name = "X,Z", conflicts_with_all = ["local_chunk", "world"], requires = "file", allow_hyphen_values = true)]
    chunk: Option<nbt_input::Coordinates>,
    /// Region-local chunk coordinates in x,z form for positional region-file mode.
    #[arg(long, value_name = "X,Z", conflicts_with_all = ["chunk", "world"], requires = "file", allow_hyphen_values = true)]
    local_chunk: Option<nbt_input::Coordinates>,
    /// Java Edition world root containing the selected dimension.
    #[arg(long, requires_all = ["location", "rule_side"])]
    world: Option<PathBuf>,
    /// World-global block coordinate formatted as `x,y,z`.
    #[arg(
        long,
        value_name = "X,Y,Z",
        allow_hyphen_values = true,
        requires = "world"
    )]
    location: Option<BlockCoordinate>,
    /// World dimension: overworld, nether, end, or an existing DIM... directory.
    #[arg(long, default_value = "overworld", requires = "world")]
    dimension: world::DimensionId,
    /// Rule document whose source registry interprets the stored numeric ID.
    #[arg(long, value_name = "FILE", requires = "world")]
    source_rule: Option<PathBuf>,
    /// Rule document whose target registry interprets the stored numeric ID.
    #[arg(long, value_name = "FILE", requires = "world")]
    target_rule: Option<PathBuf>,
}

#[derive(Clone, Debug, Args)]
struct CommonInputs {
    #[arg(long)]
    source: PathBuf,
    #[arg(long)]
    template: PathBuf,
    #[arg(long)]
    output: PathBuf,
    #[arg(long, required = true)]
    rules: Vec<PathBuf>,
    #[arg(long)]
    report: Option<PathBuf>,
}

#[derive(Clone, Debug, Args)]
struct ExplainInputs {
    #[arg(long)]
    source: PathBuf,
    #[arg(long)]
    template: PathBuf,
    #[arg(long, required = true)]
    rules: Vec<PathBuf>,
}

#[derive(Clone, Debug, Args)]
struct ConvertInputs {
    #[arg(long)]
    source: PathBuf,
    #[arg(long)]
    template: PathBuf,
    #[arg(long)]
    output: PathBuf,
    #[arg(long, required = true)]
    rules: Vec<PathBuf>,
}

#[derive(Clone, Debug, Args)]
struct CoverageInputs {
    /// Rule-selected Forge source world to inventory without modification.
    #[arg(long)]
    world: PathBuf,
    /// Transformation rule document; may be supplied more than once.
    #[arg(long, required = true)]
    rules: Vec<PathBuf>,
    /// Write the JSON report to this path instead of standard output.
    #[arg(long)]
    report: Option<PathBuf>,
}

fn main() {
    match run() {
        Ok(0) => {}
        Ok(status) => std::process::exit(status),
        Err(error) => {
            eprintln!("{error:?}");
            std::process::exit(2);
        }
    }
}

#[allow(clippy::too_many_lines)]
fn run() -> miette::Result<i32> {
    let cli = Cli::parse();
    let execution = cli.jobs.map_or_else(
        minecraft_analysis_core::work::ExecutionConfig::default,
        |jobs| {
            minecraft_analysis_core::work::ExecutionConfig::new(jobs.get())
                .expect("NonZeroUsize is valid")
        },
    );
    let mut progress = progress::Controller::new(progress::enabled(
        cli.no_progress,
        std::io::stderr().is_terminal(),
    ));
    let filter = if cli.verbose { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_writer(progress.log_writer())
        .init();
    let result = match cli.command {
        Command::Convert(inputs) => {
            let reporter = progress.reporter();
            let prepared = prepare_input_paths(
                &inputs.source,
                &inputs.template,
                &inputs.output,
                &inputs.rules,
            )?;
            let region_total = count_regions(&prepared.safe.source)?;
            minecraft_analysis_core::pipeline::convert_with_progress_config(
                &prepared.safe,
                &prepared.source_catalog,
                &prepared.target_catalog,
                &prepared.loaded,
                region_total,
                &reporter,
                execution,
            )
            .map_err(|error| miette!(error.to_string()))?;
            Ok(0)
        }
        Command::Explain {
            inputs,
            location,
            dimension,
        } => {
            let reporter = progress.reporter();
            let prepared = prepare_explanation(&inputs)?;
            let matching = minecraft_analysis_core::explanation::at_coordinate_with_progress(
                &prepared.source,
                &prepared.source_catalog,
                &prepared.target_catalog,
                &prepared.loaded,
                &dimension,
                location.0,
                &reporter,
            )
            .map_err(|error| miette!(error.to_string()))?;
            emit_explanation(prepared.loaded.source_profile, &matching)?;
            Ok(0)
        }
        Command::Nbt { command } => match command {
            NbtCommand::Dump(inputs) => {
                if let (Some(file), None) = (&inputs.file, &inputs.world) {
                    dump_nbt(
                        file,
                        nbt_input::ChunkSelectorArgs {
                            chunk: inputs.chunk,
                            local_chunk: inputs.local_chunk,
                        },
                    )?;
                } else {
                    dump_world_block(&inputs)?;
                }
                Ok(0)
            }
            NbtCommand::View {
                file,
                selector,
                rules,
            } => {
                view_nbt(&file, selector, &rules)?;
                Ok(0)
            }
        },
        Command::Rules { command } => match command {
            RulesCommand::Coverage(inputs) => {
                let reporter = progress.reporter();
                run_coverage(&inputs, &reporter, execution)
            }
            RulesCommand::UpdateManifest(inputs) => {
                let summary = manifest_authoring::update_manifest(
                    &inputs.rules,
                    &inputs.id_map,
                    inputs.manifest.into(),
                )
                .map_err(|error| miette!(error.to_string()))?;
                println!(
                    "{}",
                    serde_json::to_string_pretty(&summary).into_diagnostic()?
                );
                Ok(0)
            }
            RulesCommand::Infer(inputs) => {
                run_inference(&inputs)?;
                Ok(0)
            }
        },
    };
    progress.finish();
    result
}

fn run_inference(inputs: &InferInputs) -> miette::Result<()> {
    let source_world = canonical_world(&inputs.source_world, "source")?;
    let target_world = canonical_world(&inputs.target_world, "target")?;
    let (source_catalog, target_catalog, loaded) = prepare_catalogs(
        &source_world,
        &target_world,
        std::slice::from_ref(&inputs.rules),
    )?;
    let source = load_observation(
        &source_world,
        &inputs.source_dimension,
        inputs.coordinate.source,
        &source_catalog,
        "source",
    )?;
    let target = load_observation(
        &target_world,
        &inputs.target_dimension,
        inputs.coordinate.target,
        &target_catalog,
        "target",
    )?;
    let inferred = minecraft_analysis_core::rule_inference::infer(
        &source,
        &target,
        inputs.rule_id.as_deref(),
        &loaded,
    )
    .map_err(|error| miette!(error.to_string()))?;
    let mut output = serde_json::to_vec_pretty(&inferred).into_diagnostic()?;
    output.push(b'\n');
    let stdout = std::io::stdout();
    let mut writer = BufWriter::new(stdout.lock());
    writer.write_all(&output).into_diagnostic()?;
    writer.flush().into_diagnostic()?;
    Ok(())
}

fn canonical_world(path: &Path, side: &str) -> miette::Result<PathBuf> {
    let world = fs::canonicalize(path)
        .map_err(|error| miette!("cannot resolve {side} world {}: {error}", path.display()))?;
    if !world.is_dir() {
        return Err(miette!(
            "{side} world {} is not a directory",
            world.display()
        ));
    }
    Ok(world)
}

fn load_observation(
    world: &Path,
    dimension: &world::DimensionId,
    coordinate: [i32; 3],
    catalog: &registry::RegistryCatalog,
    side: &str,
) -> miette::Result<minecraft_analysis_core::rule_inference::BlockObservation> {
    let loaded = nbt_input::load_world_chunk(world, dimension, coordinate).map_err(|error| {
        miette!(
            "cannot load {side} observation at ({},{},{}): {error}",
            coordinate[0],
            coordinate[1],
            coordinate[2]
        )
    })?;
    let nbt_input::Source::RegionChunk { selection, .. } = loaded.source else {
        unreachable!()
    };
    let index = minecraft_analysis_core::chunk_blocks::BlockIndex::build(
        &loaded.document,
        [selection.global.x, selection.global.z],
        catalog,
    )
    .map_err(|error| {
        miette!(
            "cannot index {side} observation at ({},{},{}): {error}",
            coordinate[0],
            coordinate[1],
            coordinate[2]
        )
    })?;
    let record = index.get(coordinate).ok_or_else(|| {
        miette!(
            "no stored block is indexed for {side} coordinate ({},{},{})",
            coordinate[0],
            coordinate[1],
            coordinate[2]
        )
    })?;
    let identity = record.identity.as_ref().ok_or_else(|| {
        miette!(
            "{side} registry does not resolve numeric block ID {} at coordinate ({},{},{})",
            record.id,
            coordinate[0],
            coordinate[1],
            coordinate[2]
        )
    })?;
    Ok(minecraft_analysis_core::rule_inference::BlockObservation {
        name: identity.name.clone(),
        metadata: record.metadata,
        block_entity: record.block_entity.cloned(),
    })
}

fn emit_explanation(
    profile: SourceProfile,
    matching: &[minecraft_analysis_core::report::ObjectRecord],
) -> miette::Result<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "source_profile": profile.as_str(),
            "objects": matching,
        }))
        .into_diagnostic()?
    );
    Ok(())
}

fn run_coverage(
    inputs: &CoverageInputs,
    progress: &dyn minecraft_analysis_core::progress::ProgressObserver,
    execution: minecraft_analysis_core::work::ExecutionConfig,
) -> miette::Result<i32> {
    let world = fs::canonicalize(&inputs.world).map_err(|error| {
        miette!(
            "cannot resolve coverage world {}: {error}",
            inputs.world.display()
        )
    })?;
    let loaded = rules::load_many(&inputs.rules).map_err(|error| miette!(error.to_string()))?;
    let mut source_catalog = prepare_source(&world, loaded.source_profile)?;
    apply_manifests(&loaded, &mut source_catalog, None)?;
    let mut vanilla_catalog = registry::RegistryCatalog::default();
    match loaded.source_profile {
        SourceProfile::Forge1_2_5 => {
            vanilla_catalog = registry::RegistryCatalog::forge_1_2_5();
        }
        SourceProfile::Forge1_7_10 => {
            vanilla_catalog.add_vanilla_fallbacks(VanillaVersion::Minecraft1_7_10);
        }
    }
    let region_total = count_regions(&world)?;
    let report = coverage::analyze_source_with_progress_config(
        &world,
        &source_catalog,
        &vanilla_catalog,
        &loaded,
        region_total,
        progress,
        execution,
    )
    .map_err(|error| miette!(error.to_string()))?;
    let complete = report.complete;
    if let Some(path) = &inputs.report {
        let file = fs::File::create(path).map_err(|error| {
            miette!("cannot create coverage report {}: {error}", path.display())
        })?;
        let mut writer = BufWriter::new(file);
        report.write_json_pretty(&mut writer).into_diagnostic()?;
        writer
            .flush()
            .map_err(|error| miette!("cannot flush coverage report {}: {error}", path.display()))?;
    } else {
        let stdout = std::io::stdout();
        let mut writer = BufWriter::new(stdout.lock());
        report.write_json_pretty(&mut writer).into_diagnostic()?;
        writeln!(writer).into_diagnostic()?;
        writer.flush().into_diagnostic()?;
    }
    Ok(i32::from(!complete))
}

fn apply_manifests(
    loaded: &rules::LoadedRules,
    source_catalog: &mut registry::RegistryCatalog,
    mut target_catalog: Option<&mut registry::RegistryCatalog>,
) -> miette::Result<()> {
    for (_, document) in &loaded.documents {
        rules::apply_manifest(source_catalog, &document.source_manifest, "source rules")
            .map_err(|error| miette!(error.to_string()))?;
        if let Some(target_catalog) = target_catalog.as_deref_mut() {
            rules::apply_manifest(target_catalog, &document.target_manifest, "target rules")
                .map_err(|error| miette!(error.to_string()))?;
        }
    }
    Ok(())
}

fn prepare_source(
    world: &Path,
    selected: SourceProfile,
) -> miette::Result<registry::RegistryCatalog> {
    match selected {
        SourceProfile::Forge1_2_5 => {
            profile::detect_expected(world, WorldProfile::Forge1_2_5)
                .map_err(|error| miette!(error.to_string()))?;
            Ok(registry::RegistryCatalog::forge_1_2_5())
        }
        SourceProfile::Forge1_7_10 => {
            let source = profile::detect_expected(world, WorldProfile::Forge1_7_10)
                .map_err(|error| miette!(error.to_string()))?;
            let mut catalog = registry::extract_forge_1_7_10(&source.level_dat)
                .map_err(|error| miette!(error.to_string()))?;
            catalog.add_vanilla_fallbacks(VanillaVersion::Minecraft1_7_10);
            Ok(catalog)
        }
    }
}

fn dump_nbt(path: &Path, selector: nbt_input::ChunkSelectorArgs) -> miette::Result<()> {
    let loaded = nbt_input::load(path, selector)?;
    let snbt = nbt::to_snbt(&loaded.document).map_err(|error| match &loaded.source {
        nbt_input::Source::Standalone { path, .. } => {
            miette!("cannot render NBT file {} as SNBT: {error}", path.display())
        }
        nbt_input::Source::RegionChunk {
            path, selection, ..
        } => miette!(
            "cannot render {} as SNBT: {error}",
            nbt_input::selection_context(path, *selection)
        ),
    })?;
    let stdout = std::io::stdout();
    let mut writer = BufWriter::new(stdout.lock());
    writeln!(writer, "{snbt}").map_err(|error| {
        miette!(
            "cannot write SNBT for {} to standard output: {error}",
            path.display()
        )
    })?;
    writer.flush().map_err(|error| {
        miette!(
            "cannot flush SNBT for {} to standard output: {error}",
            path.display()
        )
    })?;
    Ok(())
}

fn dump_world_block(inputs: &DumpInputs) -> miette::Result<()> {
    let requested_world = inputs.world.as_deref().expect("clap requires world mode");
    let coordinate = inputs.location.expect("clap requires location").0;
    let world = fs::canonicalize(requested_world).map_err(|error| {
        miette!(
            "cannot resolve dump world {}: {error}",
            requested_world.display()
        )
    })?;
    if !world.is_dir() {
        return Err(miette!("dump world {} is not a directory", world.display()));
    }
    let (rule_path, side) = inputs
        .source_rule
        .as_ref()
        .map(|path| (path, "source"))
        .or_else(|| inputs.target_rule.as_ref().map(|path| (path, "target")))
        .expect("clap requires one rule side");
    let catalog = dump_catalog(&world, rule_path, side)?;
    let loaded = nbt_input::load_world_chunk(&world, &inputs.dimension, coordinate)?;
    let nbt_input::Source::RegionChunk { selection, .. } = loaded.source else {
        unreachable!("world loader always returns a region chunk")
    };
    let index = minecraft_analysis_core::chunk_blocks::BlockIndex::build(
        &loaded.document,
        [selection.global.x, selection.global.z],
        &catalog,
    )
    .map_err(|error| {
        coordinate_error(
            inputs,
            &world,
            selection,
            coordinate,
            format!("block storage is unavailable: {error}"),
        )
    })?;
    let record = index.get(coordinate).ok_or_else(|| {
        coordinate_error(
            inputs,
            &world,
            selection,
            coordinate,
            "no stored block is indexed at the coordinate",
        )
    })?;
    let identity = record.identity.as_ref().ok_or_else(|| {
        coordinate_error(
            inputs,
            &world,
            selection,
            coordinate,
            format!(
                "{side} registry does not resolve numeric block ID {}",
                record.id
            ),
        )
    })?;
    let entity = record.block_entity.map_or_else(
        || Ok("none".to_owned()),
        |value| {
            nbt::value_to_snbt(value).map_err(|error| {
                coordinate_error(
                    inputs,
                    &world,
                    selection,
                    coordinate,
                    format!("cannot render associated block entity as SNBT: {error}"),
                )
            })
        },
    )?;
    let light =
        |value: Option<u8>| value.map_or_else(|| "unavailable".into(), |value| value.to_string());
    let output = format!(
        "coordinate: {},{},{}\ndimension: {}\nglobal chunk: {},{}\nregion: {},{}\nlocal chunk: {},{}\nnumeric ID: {}\nregistry name: {}\nmetadata: {}\nblock light: {}\nsky light: {}\nsection Y: {}\nsection index: {}\nblock entity:\n{}\n",
        coordinate[0], coordinate[1], coordinate[2], nbt_input::dimension_label(&inputs.dimension),
        selection.global.x, selection.global.z, selection.region.x, selection.region.z,
        selection.local.x, selection.local.z, record.id, identity.name, record.metadata,
        light(record.block_light), light(record.sky_light), record.section_y, record.section_index, entity
    );
    let stdout = std::io::stdout();
    let mut writer = BufWriter::new(stdout.lock());
    writer.write_all(output.as_bytes()).into_diagnostic()?;
    writer.flush().into_diagnostic()?;
    Ok(())
}

fn dump_catalog(
    world: &Path,
    rule_path: &Path,
    side: &str,
) -> miette::Result<registry::RegistryCatalog> {
    let loaded = rules::load_many(&[rule_path.to_owned()])
        .map_err(|error| miette!("cannot load {side} rule {}: {error}", rule_path.display()))?;
    let mut catalog = if side == "source" {
        prepare_source(world, loaded.source_profile).map_err(|error| {
            miette!(
                "cannot build source catalog for world {}: {error}",
                world.display()
            )
        })?
    } else {
        registry::RegistryCatalog::default()
    };
    for (path, document) in &loaded.documents {
        let manifest = if side == "source" {
            &document.source_manifest
        } else {
            &document.target_manifest
        };
        rules::apply_manifest(&mut catalog, manifest, &format!("{side} rules")).map_err(
            |error| {
                miette!(
                    "cannot apply {side} manifest from {}: {error}",
                    path.display()
                )
            },
        )?;
    }
    if side == "target" {
        catalog.add_vanilla_fallbacks(VanillaVersion::Minecraft1_12_2);
    }
    Ok(catalog)
}

fn coordinate_error(
    inputs: &DumpInputs,
    world: &Path,
    selection: nbt_input::ChunkSelection,
    coordinate: [i32; 3],
    detail: impl std::fmt::Display,
) -> miette::Report {
    miette!(
        "cannot dump world {} dimension {} region ({},{}) chunk ({},{}) local ({},{}) coordinate ({},{},{}): {detail}",
        world.display(), nbt_input::dimension_label(&inputs.dimension), selection.region.x,
        selection.region.z, selection.global.x, selection.global.z, selection.local.x,
        selection.local.z, coordinate[0], coordinate[1], coordinate[2]
    )
}

fn view_nbt(
    path: &Path,
    selector: nbt_input::ChunkSelectorArgs,
    rules: &[PathBuf],
) -> miette::Result<()> {
    let loaded = nbt_input::load(path, selector)?;
    let identity = match &loaded.source {
        nbt_input::Source::RegionChunk { path, .. } => Some(nbt_context::load(path, rules)?),
        nbt_input::Source::Standalone { .. } if !rules.is_empty() => {
            return Err(miette!("--rules requires a selected region chunk"));
        }
        nbt_input::Source::Standalone { .. } => None,
    };
    nbt_viewer::run(&loaded.source, &loaded.document, identity.as_ref())
}

struct PreparedInputs {
    safe: world::SafePaths,
    source_catalog: registry::RegistryCatalog,
    target_catalog: registry::RegistryCatalog,
    loaded: rules::LoadedRules,
}

struct PreparedExplanation {
    source: PathBuf,
    source_catalog: registry::RegistryCatalog,
    target_catalog: registry::RegistryCatalog,
    loaded: rules::LoadedRules,
}

fn count_regions(world: &std::path::Path) -> miette::Result<u64> {
    minecraft_analysis_core::work::count_world_regions(world).map_err(|error| {
        miette!(
            "cannot count region files under {}: {error}",
            world.display()
        )
    })
}

fn prepare_input_paths(
    source: &Path,
    template: &Path,
    output: &Path,
    rules_paths: &[PathBuf],
) -> miette::Result<PreparedInputs> {
    let safe = world::validate_paths(source, template, output)
        .map_err(|error| miette!(error.to_string()))?;
    let (source_catalog, target_catalog, loaded) =
        prepare_catalogs(&safe.source, &safe.template, rules_paths)?;
    Ok(PreparedInputs {
        safe,
        source_catalog,
        target_catalog,
        loaded,
    })
}

fn prepare_explanation(inputs: &ExplainInputs) -> miette::Result<PreparedExplanation> {
    let source = fs::canonicalize(&inputs.source).map_err(|error| {
        miette!(
            "cannot resolve source path {}: {error}",
            inputs.source.display()
        )
    })?;
    let template = fs::canonicalize(&inputs.template).map_err(|error| {
        miette!(
            "cannot resolve template path {}: {error}",
            inputs.template.display()
        )
    })?;
    if source == template {
        return Err(miette!(
            "source and template resolve to the same directory {}",
            source.display()
        ));
    }
    let (source_catalog, target_catalog, loaded) =
        prepare_catalogs(&source, &template, &inputs.rules)?;
    Ok(PreparedExplanation {
        source,
        source_catalog,
        target_catalog,
        loaded,
    })
}

fn prepare_catalogs(
    source: &Path,
    template: &Path,
    rules_paths: &[PathBuf],
) -> miette::Result<(
    registry::RegistryCatalog,
    registry::RegistryCatalog,
    rules::LoadedRules,
)> {
    let loaded = rules::load_many(rules_paths).map_err(|error| miette!(error.to_string()))?;
    let mut source_catalog = prepare_source(source, loaded.source_profile)?;
    let template = profile::detect_expected(template, WorldProfile::Forge1_12_2)
        .map_err(|error| miette!(error.to_string()))?;
    let mut target_catalog = registry::extract_forge_1_12_2(&template.level_dat)
        .map_err(|error| miette!(error.to_string()))?;
    target_catalog.add_vanilla_fallbacks(VanillaVersion::Minecraft1_12_2);
    apply_manifests(&loaded, &mut source_catalog, Some(&mut target_catalog))?;
    Ok((source_catalog, target_catalog, loaded))
}

#[cfg(test)]
mod tests {
    use super::*;
    use minecraft_analysis_core::chunk_blocks::BlockIndex;
    use minecraft_analysis_core::nbt::{Document, List, Tag, Value};
    use minecraft_analysis_core::region::RegionWriter;
    use std::collections::BTreeMap;
    use tempfile::tempdir;

    #[test]
    fn explain_selector_parses_coordinates_and_dimensions() {
        let parsed = Cli::try_parse_from([
            "minecraft-analysis",
            "explain",
            "--source",
            "s",
            "--template",
            "t",
            "--rules",
            "r",
            "--location",
            "-513,64,17",
            "--dimension",
            "DIM42_test",
        ])
        .unwrap();
        let Command::Explain {
            location,
            dimension,
            ..
        } = parsed.command
        else {
            panic!()
        };
        assert_eq!(location, BlockCoordinate([-513, 64, 17]));
        assert_eq!(dimension, world::DimensionId::Modded("DIM42_test".into()));

        for invalid in ["region/r.0.0.mca:1,2,3", "1,2", "1,2,nope", "1,2,3,4"] {
            assert!(invalid.parse::<BlockCoordinate>().is_err(), "{invalid}");
        }
    }

    #[test]
    fn nbt_view_accepts_repeated_optional_rules() {
        let cli = Cli::try_parse_from([
            "minecraft-analysis",
            "nbt",
            "view",
            "r.0.0.mca",
            "--chunk",
            "0,0",
            "--rules",
            "first.json",
            "--rules",
            "second.json",
        ])
        .unwrap();
        let Command::Nbt {
            command: NbtCommand::View { rules, .. },
        } = cli.command
        else {
            panic!("expected nbt view")
        };
        assert_eq!(
            rules,
            [PathBuf::from("first.json"), PathBuf::from("second.json")]
        );
    }

    #[test]
    fn nbt_dump_world_mode_enforces_complete_exclusive_inputs() {
        let valid = [
            vec!["--world", "w", "--location", "1,2,3", "--source-rule", "r"],
            vec![
                "--world",
                "w",
                "--location",
                "-1,2,-3",
                "--dimension",
                "nether",
                "--target-rule",
                "r",
            ],
        ];
        for trailing in valid {
            let mut args = vec!["minecraft-analysis", "nbt", "dump"];
            args.extend(trailing);
            assert!(Cli::try_parse_from(args).is_ok());
        }
        let invalid = [
            vec![],
            vec!["--world", "w", "--source-rule", "r"],
            vec!["--location", "1,2,3", "--source-rule", "r"],
            vec!["--world", "w", "--location", "1,2,3"],
            vec![
                "--world",
                "w",
                "--location",
                "1,2,3",
                "--source-rule",
                "a",
                "--target-rule",
                "b",
            ],
            vec![
                "file.dat",
                "--world",
                "w",
                "--location",
                "1,2,3",
                "--source-rule",
                "r",
            ],
            vec![
                "--world",
                "w",
                "--location",
                "1,2,3",
                "--source-rule",
                "r",
                "--chunk",
                "0,0",
            ],
        ];
        for trailing in invalid {
            let mut args = vec!["minecraft-analysis", "nbt", "dump"];
            args.extend(trailing);
            assert!(
                Cli::try_parse_from(args.clone()).is_err(),
                "accepted {args:?}"
            );
        }
        assert!(Cli::try_parse_from(["minecraft-analysis", "nbt", "dump", "file.dat"]).is_ok());
    }

    #[test]
    fn rules_update_manifest_requires_and_parses_manifest_side() {
        let cli = Cli::try_parse_from([
            "minecraft-analysis",
            "rules",
            "update-manifest",
            "--rules",
            "rules.json",
            "--id-map",
            "idmap.txt",
            "--manifest",
            "source",
        ])
        .unwrap();
        let Command::Rules {
            command: RulesCommand::UpdateManifest(inputs),
        } = cli.command
        else {
            panic!("expected rules update-manifest")
        };
        assert_eq!(inputs.rules, PathBuf::from("rules.json"));
        assert_eq!(inputs.id_map, PathBuf::from("idmap.txt"));
        assert_eq!(inputs.manifest, ManifestSelection::Source);

        for trailing in [vec![], vec!["--manifest", "neither"]] {
            let mut args = vec![
                "minecraft-analysis",
                "rules",
                "update-manifest",
                "--rules",
                "rules.json",
                "--id-map",
                "idmap.txt",
            ];
            args.extend(trailing);
            assert!(Cli::try_parse_from(args).is_err());
        }
    }

    #[test]
    fn rules_infer_parses_exactly_one_ordered_coordinate_pair() {
        for coordinate in ["1,2,3:4,5,6", "-1,-2,-3:-4,-5,-6"] {
            let parsed = Cli::try_parse_from([
                "minecraft-analysis",
                "rules",
                "infer",
                "--rules",
                "r.json",
                "--source-world",
                "source",
                "--target-world",
                "target",
                "--coordinate",
                coordinate,
            ])
            .unwrap();
            let Command::Rules {
                command: RulesCommand::Infer(inputs),
            } = parsed.command
            else {
                panic!()
            };
            assert_eq!(inputs.source_dimension, world::DimensionId::Overworld);
            assert_eq!(inputs.target_dimension, world::DimensionId::Overworld);
        }
        let equals = Cli::try_parse_from([
            "minecraft-analysis",
            "rules",
            "infer",
            "--rules",
            "r.json",
            "--source-world",
            "source",
            "--target-world",
            "target",
            "--coordinate=-1,2,3:4,5,6",
        ])
        .unwrap();
        assert!(matches!(
            equals.command,
            Command::Rules {
                command: RulesCommand::Infer(_)
            }
        ));

        for coordinates in [
            vec![],
            vec!["1,2,3"],
            vec!["1,2:3,4,5"],
            vec!["1,2,3:4,5,6:7,8,9"],
            vec!["2147483648,2,3:4,5,6"],
        ] {
            let mut args = vec![
                "minecraft-analysis",
                "rules",
                "infer",
                "--rules",
                "r.json",
                "--source-world",
                "source",
                "--target-world",
                "target",
            ];
            for coordinate in coordinates {
                args.extend(["--coordinate", coordinate]);
            }
            assert!(Cli::try_parse_from(args).is_err());
        }
        assert!(Cli::try_parse_from([
            "minecraft-analysis",
            "rules",
            "infer",
            "--rules",
            "r.json",
            "--source-world",
            "source",
            "--target-world",
            "target",
            "--coordinate",
            "1,2,3:4,5,6",
            "--coordinate",
            "7,8,9:10,11,12",
        ])
        .is_err());
    }

    #[test]
    fn region_view_inputs_prepare_enriched_coordinate_index_without_terminal() {
        let temp = tempdir().unwrap();
        let region_path = temp.path().join("world/region/r.0.0.mca");
        fs::create_dir_all(region_path.parent().unwrap()).unwrap();
        let pair = Value::Compound(BTreeMap::from([
            ("K".into(), Value::String("\u{1}mod:machine".into())),
            ("V".into(), Value::Int(300)),
        ]));
        let level = Document {
            root_name: String::new(),
            root: BTreeMap::from([(
                "FML".into(),
                Value::Compound(BTreeMap::from([(
                    "ItemData".into(),
                    Value::List(List {
                        element_tag: Tag::Compound,
                        values: vec![pair],
                    }),
                )])),
            )]),
        };
        fs::write(
            temp.path().join("world/level.dat"),
            nbt::encode(&level, nbt::Compression::Gzip).unwrap(),
        )
        .unwrap();
        let mut blocks = vec![0; 4096];
        blocks[0] = 44;
        let mut add = vec![0; 2048];
        add[0] = 1;
        let section = Value::Compound(BTreeMap::from([
            ("Y".into(), Value::Byte(0)),
            ("Blocks".into(), Value::ByteArray(blocks)),
            ("Add".into(), Value::ByteArray(add)),
            ("Data".into(), Value::ByteArray(vec![2; 2048])),
            ("BlockLight".into(), Value::ByteArray(vec![7; 2048])),
            ("SkyLight".into(), Value::ByteArray(vec![-1; 2048])),
        ]));
        let tile = Value::Compound(BTreeMap::from([
            ("id".into(), Value::String("mod:tile".into())),
            ("x".into(), Value::Int(0)),
            ("y".into(), Value::Int(0)),
            ("z".into(), Value::Int(0)),
            ("opaque".into(), Value::LongArray(vec![1, 2])),
        ]));
        let chunk = Document {
            root_name: String::new(),
            root: BTreeMap::from([(
                "Level".into(),
                Value::Compound(BTreeMap::from([
                    (
                        "Sections".into(),
                        Value::List(List {
                            element_tag: Tag::Compound,
                            values: vec![section],
                        }),
                    ),
                    (
                        "TileEntities".into(),
                        Value::List(List {
                            element_tag: Tag::Compound,
                            values: vec![tile],
                        }),
                    ),
                ])),
            )]),
        };
        let mut writer = RegionWriter::new().unwrap();
        writer
            .write_chunk(0, 0, &nbt::encode_uncompressed(&chunk).unwrap(), 1)
            .unwrap();
        fs::write(&region_path, writer.finish().unwrap()).unwrap();

        let loaded = nbt_input::load(
            &region_path,
            nbt_input::ChunkSelectorArgs {
                chunk: Some(nbt_input::Coordinates { x: 0, z: 0 }),
                local_chunk: None,
            },
        )
        .unwrap();
        let identity = nbt_context::load(&region_path, &[]).unwrap();
        let index = BlockIndex::build(&loaded.document, [0, 0], &identity.catalog).unwrap();
        let record = index.get([0, 0, 0]).unwrap();
        assert_eq!(record.identity.as_ref().unwrap().name, "mod:machine");
        assert_eq!(
            (record.metadata, record.block_light, record.sky_light),
            (2, Some(7), Some(15))
        );
        assert!(record.block_entity.is_some());
    }
}
