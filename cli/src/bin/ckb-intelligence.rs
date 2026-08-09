//! Fast JSON intelligence facade for IDE extensions and agents.
//!
//! `ckb-intelligence` deliberately emits JSON only. It scans the real local
//! workspace with CKB Core, then runs bounded graph intelligence without any
//! remote filesystem assumption or synthetic fallback.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use ckb_core::{ArchitectureMemoryEngine, CkbEngine, DeepActivityAnalyzer};
use serde_json::json;
use std::path::PathBuf;
use std::time::Instant;

#[derive(Parser)]
#[command(name = "ckb-intelligence", version, about = "CKB deep architecture intelligence for IDEs and AI agents")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Build a full architecture graph and return deep activity intelligence.
    Activity {
        path: PathBuf,
    },
    /// Retrieve a bounded architecture-memory neighborhood for a query.
    Memory {
        path: PathBuf,
        query: String,
        #[arg(long, default_value_t = 3)]
        depth: usize,
        #[arg(long, default_value_t = 24)]
        limit: usize,
    },
    /// Return deterministic Code DNA derived from the current graph/runtime evidence.
    Dna {
        path: PathBuf,
    },
    /// Return a compact workspace intelligence bundle in one scan.
    Bundle {
        path: PathBuf,
        #[arg(long, default_value = "architecture hotspots dependencies runtime change risk")]
        query: String,
        #[arg(long, default_value_t = 3)]
        depth: usize,
        #[arg(long, default_value_t = 32)]
        limit: usize,
    },
}

async fn scan(path: &PathBuf) -> Result<(CkbEngine, ckb_core::ScanReport, u128)> {
    let started = Instant::now();
    let canonical = std::fs::canonicalize(path)
        .with_context(|| format!("Cannot resolve workspace path {}", path.display()))?;
    let canonical = canonical.to_string_lossy().to_string();
    let engine = CkbEngine::new()?;
    let report = engine.scan_codebase(&canonical).await?;
    Ok((engine, report, started.elapsed().as_millis()))
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Activity { path } => {
            let (engine, scan, wall_ms) = scan(&path).await?;
            let graph = engine.architecture_graph_snapshot().await;
            let activity = DeepActivityAnalyzer::analyze(&graph)?;
            println!("{}", serde_json::to_string(&json!({
                "kind": "static+runtime-when-observed",
                "source": "ckb-core-local-workspace",
                "scan": scan,
                "scanWallMs": wall_ms,
                "activity": activity,
                "evidencePolicy": "static-runtime-predicted-separated",
                "synthetic": false
            }))?);
        }
        Command::Memory { path, query, depth, limit } => {
            let (engine, scan, wall_ms) = scan(&path).await?;
            let graph = engine.architecture_graph_snapshot().await;
            let memory = ArchitectureMemoryEngine::query(&graph, &query, depth, limit)?;
            println!("{}", serde_json::to_string(&json!({
                "kind": "architecture-memory",
                "source": "ckb-core-local-workspace",
                "snapshotId": scan.snapshot_id,
                "scanWallMs": wall_ms,
                "memory": memory,
                "synthetic": false
            }))?);
        }
        Command::Dna { path } => {
            let (engine, scan, wall_ms) = scan(&path).await?;
            let graph = engine.architecture_graph_snapshot().await;
            let dna = ArchitectureMemoryEngine::code_dna(&graph)?;
            println!("{}", serde_json::to_string(&json!({
                "kind": "code-dna",
                "source": "ckb-core-local-workspace",
                "snapshotId": scan.snapshot_id,
                "scanWallMs": wall_ms,
                "dna": dna,
                "synthetic": false
            }))?);
        }
        Command::Bundle { path, query, depth, limit } => {
            let (engine, scan, wall_ms) = scan(&path).await?;
            let graph = engine.architecture_graph_snapshot().await;
            let activity = DeepActivityAnalyzer::analyze(&graph)?;
            let dna = ArchitectureMemoryEngine::code_dna(&graph)?;
            let memory = ArchitectureMemoryEngine::query(&graph, &query, depth, limit)?;
            println!("{}", serde_json::to_string(&json!({
                "version": "ckb-ide-intelligence-v1",
                "kind": "architecture-intelligence-bundle",
                "source": "ckb-core-local-workspace",
                "scan": scan,
                "scanWallMs": wall_ms,
                "activity": activity,
                "dna": dna,
                "memory": memory,
                "evidencePolicy": "static-runtime-predicted-separated",
                "synthetic": false
            }))?);
        }
    }

    Ok(())
}
