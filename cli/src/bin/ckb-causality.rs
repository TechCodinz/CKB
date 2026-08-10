use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use ckb_core::analysis::{
    build_federated_deep_causality, build_workspace_deep_causality,
    ingest_runtime_resource_observations, ApiContract, ArchitectureRule,
    ChangeOperation, DeepCausalityEngine, FederatedWorkspaceMember,
    RuntimeResourceObservation,
};
use serde::de::DeserializeOwned;
use serde_json::json;
use std::{fs, path::Path};

#[derive(Parser)]
#[command(name = "ckb-causality", version, about = "CKB V13.1 Deep Software Causality")]
struct Cli {
    /// Existing DeepCausalityEngine JSON bundle. Required for query/mutation
    /// commands, but not for `build` or `federate`.
    #[arg(long, global = true)]
    bundle: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Build {
        workspace: String,
        #[arg(long)] repository: Option<String>,
        #[arg(long, default_value = ".ckb/deep-causality.json")] output: String,
    },
    /// Build one organization-wide evidence bundle from a JSON array of
    /// FederatedWorkspaceMember { root, repository } objects.
    Federate {
        members: String,
        #[arg(long, default_value = ".ckb/deep-causality-federated.json")] output: String,
    },
    /// Merge observed profiler/APM resource samples into an existing bundle.
    EnrichRuntime {
        observations: String,
        /// Output bundle. Defaults to overwriting --bundle.
        #[arg(long)] output: Option<String>,
    },
    DataFlow { source: String, sink: String, #[arg(long, default_value_t = 24)] depth: usize },
    Taint { #[arg(long, value_delimiter = ',')] sources: Vec<String>, #[arg(long, value_delimiter = ',')] sinks: Vec<String>, #[arg(long, default_value_t = 24)] depth: usize },
    Reachable { source: String, sink: String, #[arg(long, value_delimiter = ',')] conditions: Vec<String>, #[arg(long, default_value_t = 24)] depth: usize },
    Constraints { #[arg(long, value_delimiter = ',')] constraints: Vec<String> },
    Concurrency,
    SchemaImpact { entity: String, #[arg(long, default_value_t = 12)] depth: usize },
    InfraImpact { entity: String, #[arg(long, default_value_t = 12)] depth: usize },
    ConfigImpact { entity: String, #[arg(long, default_value_t = 12)] depth: usize },
    DistributedFlow { source: String, sink: String, #[arg(long, default_value_t = 32)] depth: usize },
    ContractDiff { before: String, after: String },
    Tests { #[arg(long, value_delimiter = ',')] changed: Vec<String>, #[arg(long, default_value_t = 12)] depth: usize },
    Policy { rules: String },
    DriftForecast { #[arg(long, value_delimiter = ',')] edge_counts: Vec<usize>, #[arg(long, default_value_t = 5)] horizon: usize },
    Simulate { operations: String, #[arg(long, default_value_t = 12)] depth: usize },
    Hotspots,
    FailurePropagation { source: String, #[arg(long, default_value_t = 12)] depth: usize },
    TemporalDiff { older: String },
    CrossRepo { source: String, sink: String, #[arg(long, default_value_t = 32)] depth: usize },
    Ownership,
    Quality,
    Summary,
}

fn read_json<T: DeserializeOwned>(path: &str) -> Result<T> {
    let text = fs::read_to_string(path).with_context(|| format!("read {path}"))?;
    serde_json::from_str(&text).with_context(|| format!("parse JSON {path}"))
}

fn required_bundle(path: Option<&str>) -> Result<DeepCausalityEngine> {
    let path = path.context("--bundle is required for this causality query; run `ckb-causality build <workspace>` first")?;
    read_json(path)
}

fn write_bundle(path: &Path, engine: &DeepCausalityEngine) -> Result<()> {
    if let Some(parent) = path.parent() { fs::create_dir_all(parent)?; }
    fs::write(path, serde_json::to_vec_pretty(engine)?)?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        Command::Build { workspace, repository, output } => {
            let repo = repository.clone().unwrap_or_else(|| {
                Path::new(workspace).file_name().and_then(|v| v.to_str()).unwrap_or("workspace").to_string()
            });
            let (engine, report) = build_workspace_deep_causality(workspace, repo).await?;
            let output_path = Path::new(workspace).join(output);
            write_bundle(&output_path, &engine)?;
            println!("{}", serde_json::to_string_pretty(&json!({
                "bundle": output_path.to_string_lossy(),
                "report": report,
            }))?);
            return Ok(());
        }
        Command::Federate { members, output } => {
            let members: Vec<FederatedWorkspaceMember> = read_json(members)?;
            let (engine, report) = build_federated_deep_causality(&members).await?;
            let output_path = Path::new(output);
            write_bundle(output_path, &engine)?;
            println!("{}", serde_json::to_string_pretty(&json!({
                "bundle": output_path.to_string_lossy(),
                "report": report,
            }))?);
            return Ok(());
        }
        Command::EnrichRuntime { observations, output } => {
            let bundle_path = cli.bundle.as_deref().context("--bundle is required for enrich-runtime")?;
            let mut engine: DeepCausalityEngine = read_json(bundle_path)?;
            let observations: Vec<RuntimeResourceObservation> = read_json(observations)?;
            let report = ingest_runtime_resource_observations(&mut engine, &observations);
            let output_path = output.as_deref().unwrap_or(bundle_path);
            write_bundle(Path::new(output_path), &engine)?;
            println!("{}", serde_json::to_string_pretty(&json!({
                "bundle": output_path,
                "report": report,
                "hotspots": engine.runtime_hotspots(),
            }))?);
            return Ok(());
        }
        _ => {}
    }

    let engine = required_bundle(cli.bundle.as_deref())?;
    let result = match cli.command {
        Command::Build { .. } | Command::Federate { .. } | Command::EnrichRuntime { .. } => unreachable!(),
        Command::DataFlow { source, sink, depth } => serde_json::to_value(engine.data_flow_path(&source, &sink, depth))?,
        Command::Taint { sources, sinks, depth } => serde_json::to_value(engine.taint_paths_v2(&sources, &sinks, depth))?,
        Command::Reachable { source, sink, conditions, depth } => serde_json::to_value(engine.reachable_under(&source, &sink, &conditions, depth))?,
        Command::Constraints { constraints } => json!({ "satisfiable": engine.constraints_satisfiable_v2(&constraints), "constraints": constraints }),
        Command::Concurrency => serde_json::to_value(engine.concurrency_hazards())?,
        Command::SchemaImpact { entity, depth } => serde_json::to_value(engine.schema_impact(&entity, depth))?,
        Command::InfraImpact { entity, depth } => serde_json::to_value(engine.infrastructure_impact(&entity, depth))?,
        Command::ConfigImpact { entity, depth } => serde_json::to_value(engine.config_dependents(&entity, depth))?,
        Command::DistributedFlow { source, sink, depth } => serde_json::to_value(engine.distributed_flow(&source, &sink, depth))?,
        Command::ContractDiff { before, after } => {
            let before: ApiContract = read_json(&before)?;
            let after: ApiContract = read_json(&after)?;
            serde_json::to_value(engine.compare_contracts(&before, &after))?
        }
        Command::Tests { changed, depth } => serde_json::to_value(engine.tests_for_change(&changed, depth))?,
        Command::Policy { rules } => {
            let rules: Vec<ArchitectureRule> = read_json(&rules)?;
            serde_json::to_value(engine.enforce_rules(&rules))?
        }
        Command::DriftForecast { edge_counts, horizon } => json!({
            "evidence":"predicted",
            "values":engine.forecast_drift(&edge_counts, horizon),
        }),
        Command::Simulate { operations, depth } => {
            let operations: Vec<ChangeOperation> = read_json(&operations)?;
            serde_json::to_value(engine.simulate_change(&operations, depth))?
        }
        Command::Hotspots => serde_json::to_value(engine.runtime_hotspots())?,
        Command::FailurePropagation { source, depth } => serde_json::to_value(engine.failure_propagation_v2(&source, depth))?,
        Command::TemporalDiff { older } => {
            let older: DeepCausalityEngine = read_json(&older)?;
            let (added, removed) = engine.temporal_diff(&older);
            json!({ "added": added, "removed": removed })
        }
        Command::CrossRepo { source, sink, depth } => serde_json::to_value(engine.cross_repo_path(&source, &sink, depth))?,
        Command::Ownership => serde_json::to_value(engine.ownership_risks())?,
        Command::Quality => serde_json::to_value(engine.quality_metrics())?,
        Command::Summary => json!({
            "entities": engine.entities().count(),
            "facts": engine.facts().len(),
            "quality": engine.quality_metrics(),
            "concurrency_hazards": engine.concurrency_hazards().len(),
            "runtime_hotspots": engine.runtime_hotspots().len(),
            "ownership_risks": engine.ownership_risks().len(),
        }),
    };
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
