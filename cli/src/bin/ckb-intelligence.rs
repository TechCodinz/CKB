//! Fast JSON intelligence facade for IDE extensions and agents.
//!
//! `ckb-intelligence` deliberately emits JSON only. It scans the real local
//! workspace with CKB Core, then runs bounded graph intelligence without any
//! remote filesystem assumption or synthetic fallback.
//!
//! Unchanged workspaces restore the last persisted CKB graph after a cheap
//! source metadata fingerprint. This makes repeated model-memory queries and
//! IDE reopen hydration fast without pretending a stale graph is current.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use ckb_core::{
    ArchitectureMemoryEngine, CkbEngine, DeepActivityAnalyzer, PatchTransaction,
    PatchTransactionEngine, ScanReport, ValidationCommand,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Instant, UNIX_EPOCH};

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
    /// Return a compact workspace intelligence bundle in one graph hydration.
    Bundle {
        path: PathBuf,
        #[arg(long, default_value = "architecture hotspots dependencies runtime change risk")]
        query: String,
        #[arg(long, default_value_t = 3)]
        depth: usize,
        #[arg(long, default_value_t = 32)]
        limit: usize,
    },
    /// Prepare and validate a unified diff on an isolated Git branch/worktree.
    PreparePatch {
        path: PathBuf,
        patch_file: PathBuf,
        validation_file: PathBuf,
        state_file: PathBuf,
        #[arg(long, default_value = "HEAD")]
        baseline: String,
    },
    /// Explicitly commit a validated isolated transaction; never merges or pushes.
    CommitPatch {
        state_file: PathBuf,
        /// Must exactly match the stagedTreeId returned by prepare-patch.
        #[arg(long)]
        confirm_staged_tree: String,
        #[arg(long)]
        message: String,
    },
    /// Rescan a committed or rolled-back isolated transaction for before/after evidence.
    RescanPatch {
        state_file: PathBuf,
        #[arg(
            long,
            default_value = "post-change architecture runtime regression risk"
        )]
        query: String,
        #[arg(long, default_value_t = 3)]
        depth: usize,
        #[arg(long, default_value_t = 32)]
        limit: usize,
    },
    /// Create a validated rollback commit inside the isolated transaction branch.
    RollbackPatch {
        state_file: PathBuf,
        /// Must exactly match the isolated commit being rolled back.
        #[arg(long)]
        confirm_committed_sha: String,
    },
    /// Remove a transaction worktree, optionally deleting its isolated branch.
    CleanupPatch {
        state_file: PathBuf,
        #[arg(long, default_value_t = false)]
        delete_branch: bool,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CacheMeta {
    workspace: String,
    fingerprint: String,
    report: ScanReport,
}

fn source_extensions() -> HashSet<&'static str> {
    ["ts", "tsx", "js", "jsx", "mjs", "py", "go", "rs", "java"]
        .into_iter()
        .collect()
}

fn skip_dir(name: &str) -> bool {
    matches!(
        name,
        ".git" | "node_modules" | "target" | "dist" | "build" | ".next" |
        "coverage" | "vendor" | "__pycache__" | ".turbo" | ".yarn" | "out"
    )
}

fn fingerprint_entries(root: &Path, dir: &Path, extensions: &HashSet<&str>, rows: &mut Vec<String>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            let name = entry.file_name();
            if !skip_dir(&name.to_string_lossy()) {
                fingerprint_entries(root, &path, extensions, rows)?;
            }
            continue;
        }
        if !file_type.is_file() { continue; }

        let file_name = entry.file_name().to_string_lossy().to_string();
        let extension = path.extension().and_then(|value| value.to_str()).unwrap_or("").to_ascii_lowercase();
        let is_source = extensions.contains(extension.as_str());
        let is_manifest = matches!(file_name.as_str(), "package.json" | "Cargo.toml" | "go.mod" | "pyproject.toml");
        if !is_source && !is_manifest { continue; }

        let metadata = entry.metadata()?;
        let modified = metadata.modified().ok()
            .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
            .map(|value| value.as_nanos())
            .unwrap_or(0);
        let relative = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
        rows.push(format!("{}|{}|{}", relative, metadata.len(), modified));
    }
    Ok(())
}

fn source_fingerprint(root: &Path) -> Result<String> {
    let mut rows = Vec::new();
    fingerprint_entries(root, root, &source_extensions(), &mut rows)?;
    rows.sort_unstable();
    let mut context = md5::Context::new();
    for row in rows {
        context.consume(row.as_bytes());
        context.consume(b"\n");
    }
    Ok(format!("{:x}", context.compute()))
}

fn cache_root(workspace: &Path) -> PathBuf {
    let identity = workspace.to_string_lossy().replace('\\', "/").to_ascii_lowercase();
    let hash = format!("{:x}", md5::compute(identity.as_bytes()));
    let base = std::env::var_os("CKB_IDE_CACHE_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("LOCALAPPDATA").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("ckb").join("ide-intelligence").join(hash)
}

fn parser_concurrency() -> usize {
    if let Ok(value) = std::env::var("CKB_IDE_PARSE_CONCURRENCY") {
        if let Ok(parsed) = value.parse::<usize>() {
            return parsed.clamp(1, 128);
        }
    }
    let logical = std::thread::available_parallelism().map(|value| value.get()).unwrap_or(4);
    // Parsing is CPU-heavy with short I/O phases. Two tasks per logical CPU is
    // a useful ceiling for IDE responsiveness, but cap it so very large hosts
    // do not turn a workspace scan into an allocation/open-file storm.
    (logical.saturating_mul(2)).clamp(4, 32)
}

fn read_transaction(path: &Path) -> Result<PatchTransaction> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("Cannot read transaction state {}", path.display()))?;
    serde_json::from_slice(&bytes)
        .with_context(|| format!("Invalid transaction state {}", path.display()))
}

fn write_transaction(path: &Path, transaction: &PatchTransaction) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("transaction.json");
    let temporary = parent.join(format!(".{name}.{}.tmp", std::process::id()));
    std::fs::write(&temporary, serde_json::to_vec_pretty(transaction)?)?;
    std::fs::rename(&temporary, path)?;
    Ok(())
}

async fn load_graph(path: &PathBuf) -> Result<(CkbEngine, ScanReport, u128, bool)> {
    let started = Instant::now();
    let canonical = std::fs::canonicalize(path)
        .with_context(|| format!("Cannot resolve workspace path {}", path.display()))?;
    let canonical_string = canonical.to_string_lossy().to_string();
    let fingerprint = source_fingerprint(&canonical)?;
    let cache = cache_root(&canonical);
    let graph_store = cache.join("graph");
    let metadata_file = cache.join("metadata.json");
    std::fs::create_dir_all(&cache)?;

    let engine = CkbEngine::new_with_storage_path(&graph_store.to_string_lossy())?;
    let cached = std::fs::read(&metadata_file)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<CacheMeta>(&bytes).ok())
        .filter(|meta| meta.workspace == canonical_string && meta.fingerprint == fingerprint);

    if let Some(meta) = cached {
        if engine.restore_latest_architecture_snapshot().await? {
            return Ok((engine, meta.report, started.elapsed().as_millis(), true));
        }
    }

    let report = engine.scan_codebase_bounded(&canonical_string, parser_concurrency()).await?;
    let meta = CacheMeta {
        workspace: canonical_string,
        fingerprint,
        report: report.clone(),
    };
    let temp_file = metadata_file.with_extension("json.tmp");
    std::fs::write(&temp_file, serde_json::to_vec(&meta)?)?;
    std::fs::rename(temp_file, metadata_file)?;
    Ok((engine, report, started.elapsed().as_millis(), false))
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Activity { path } => {
            let (engine, scan, wall_ms, cache_hit) = load_graph(&path).await?;
            let graph = engine.architecture_graph_snapshot().await;
            let activity = DeepActivityAnalyzer::analyze(&graph)?;
            println!("{}", serde_json::to_string(&json!({
                "kind": "static+runtime-when-observed",
                "source": "ckb-core-local-workspace",
                "scan": scan,
                "scanWallMs": wall_ms,
                "cacheHit": cache_hit,
                "parseConcurrency": parser_concurrency(),
                "activity": activity,
                "evidencePolicy": "static-runtime-predicted-separated",
                "synthetic": false
            }))?);
        }
        Command::Memory { path, query, depth, limit } => {
            let (engine, scan, wall_ms, cache_hit) = load_graph(&path).await?;
            let graph = engine.architecture_graph_snapshot().await;
            let memory = ArchitectureMemoryEngine::query(&graph, &query, depth, limit)?;
            println!("{}", serde_json::to_string(&json!({
                "kind": "architecture-memory",
                "source": "ckb-core-local-workspace",
                "snapshotId": scan.snapshot_id,
                "scanWallMs": wall_ms,
                "cacheHit": cache_hit,
                "parseConcurrency": parser_concurrency(),
                "memory": memory,
                "synthetic": false
            }))?);
        }
        Command::Dna { path } => {
            let (engine, scan, wall_ms, cache_hit) = load_graph(&path).await?;
            let graph = engine.architecture_graph_snapshot().await;
            let dna = ArchitectureMemoryEngine::code_dna(&graph)?;
            println!("{}", serde_json::to_string(&json!({
                "kind": "code-dna",
                "source": "ckb-core-local-workspace",
                "snapshotId": scan.snapshot_id,
                "scanWallMs": wall_ms,
                "cacheHit": cache_hit,
                "parseConcurrency": parser_concurrency(),
                "dna": dna,
                "synthetic": false
            }))?);
        }
        Command::Bundle { path, query, depth, limit } => {
            let (engine, scan, wall_ms, cache_hit) = load_graph(&path).await?;
            let graph = engine.architecture_graph_snapshot().await;
            let activity = DeepActivityAnalyzer::analyze(&graph)?;
            let dna = ArchitectureMemoryEngine::code_dna(&graph)?;
            let memory = ArchitectureMemoryEngine::query(&graph, &query, depth, limit)?;
            println!("{}", serde_json::to_string(&json!({
                "version": "ckb-ide-intelligence-v3",
                "kind": "architecture-intelligence-bundle",
                "source": "ckb-core-local-workspace",
                "scan": scan,
                "scanWallMs": wall_ms,
                "cacheHit": cache_hit,
                "parseConcurrency": parser_concurrency(),
                "activity": activity,
                "dna": dna,
                "memory": memory,
                "evidencePolicy": "static-runtime-predicted-separated",
                "synthetic": false
            }))?);
        }
        Command::PreparePatch {
            path,
            patch_file,
            validation_file,
            state_file,
            baseline,
        } => {
            let patch = std::fs::read_to_string(&patch_file)
                .with_context(|| format!("Cannot read patch {}", patch_file.display()))?;
            let validation_bytes = std::fs::read(&validation_file).with_context(|| {
                format!("Cannot read validation plan {}", validation_file.display())
            })?;
            let validations: Vec<ValidationCommand> = serde_json::from_slice(&validation_bytes)
                .with_context(|| {
                    format!(
                        "Validation plan {} must be a JSON array of label/program/args objects",
                        validation_file.display()
                    )
                })?;
            let transaction =
                PatchTransactionEngine::prepare(&path, &baseline, &patch, &validations)?;
            write_transaction(&state_file, &transaction)?;
            println!("{}", serde_json::to_string(&json!({
                "kind": "patch-transaction",
                "operation": "prepare-and-validate",
                "transaction": transaction,
                "stateFile": state_file,
                "confirmationRequired": true,
                "mutationApplied": false,
                "activeCheckoutModified": false,
                "synthetic": false
            }))?);
        }
        Command::CommitPatch {
            state_file,
            confirm_staged_tree,
            message,
        } => {
            let mut transaction = read_transaction(&state_file)?;
            if confirm_staged_tree != transaction.staged_tree_id {
                anyhow::bail!(
                    "confirmation does not match the validated staged tree; no commit was created"
                );
            }
            let committed_sha = PatchTransactionEngine::commit(&mut transaction, &message)?;
            write_transaction(&state_file, &transaction)?;
            println!("{}", serde_json::to_string(&json!({
                "kind": "patch-transaction",
                "operation": "commit-isolated-branch",
                "transactionId": transaction.transaction_id,
                "branchName": transaction.branch_name,
                "committedSha": committed_sha,
                "merged": false,
                "pushed": false,
                "activeCheckoutModified": false,
                "synthetic": false
            }))?);
        }
        Command::RescanPatch {
            state_file,
            query,
            depth,
            limit,
        } => {
            let transaction = read_transaction(&state_file)?;
            if transaction.committed_sha.is_none() {
                anyhow::bail!("transaction has no isolated commit to rescan");
            }
            let worktree = PathBuf::from(&transaction.worktree_path);
            let (engine, scan, wall_ms, cache_hit) = load_graph(&worktree).await?;
            let graph = engine.architecture_graph_snapshot().await;
            let activity = DeepActivityAnalyzer::analyze(&graph)?;
            let dna = ArchitectureMemoryEngine::code_dna(&graph)?;
            let memory = ArchitectureMemoryEngine::query(&graph, &query, depth, limit)?;
            let validations = PatchTransactionEngine::revalidate(&transaction)?;
            let validation_passed = validations.iter().all(|result| result.success);
            println!(
                "{}",
                serde_json::to_string(&json!({
                    "version": "ckb-patch-rescan-v1",
                    "kind": "patch-transaction-rescan",
                    "transactionId": transaction.transaction_id,
                    "baselineCommit": transaction.baseline_commit,
                    "committedSha": transaction.committed_sha,
                    "rollbackCommittedSha": transaction.rollback_committed_sha,
                    "transactionState": transaction.state,
                    "scan": scan,
                    "scanWallMs": wall_ms,
                    "cacheHit": cache_hit,
                    "activity": activity,
                    "dna": dna,
                    "memory": memory,
                    "validations": validations,
                    "validationPassed": validation_passed,
                    "evidencePolicy": "static-runtime-predicted-separated",
                    "activeCheckoutModified": false,
                    "synthetic": false
                }))?
            );
        }
        Command::RollbackPatch {
            state_file,
            confirm_committed_sha,
        } => {
            let mut transaction = read_transaction(&state_file)?;
            if transaction.committed_sha.as_deref() != Some(confirm_committed_sha.as_str()) {
                anyhow::bail!(
                    "rollback confirmation does not match the isolated commit; no rollback was created"
                );
            }
            let rollback_sha = PatchTransactionEngine::rollback(&mut transaction)?;
            write_transaction(&state_file, &transaction)?;
            println!(
                "{}",
                serde_json::to_string(&json!({
                    "kind": "patch-transaction",
                    "operation": "rollback-isolated-branch",
                    "transactionId": transaction.transaction_id,
                    "committedSha": transaction.committed_sha,
                    "rollbackStagedTreeId": transaction.rollback_staged_tree_id,
                    "rollbackCommittedSha": rollback_sha,
                    "rollbackValidations": transaction.rollback_validations,
                    "merged": false,
                    "pushed": false,
                    "activeCheckoutModified": false,
                    "synthetic": false
                }))?
            );
        }
        Command::CleanupPatch {
            state_file,
            delete_branch,
        } => {
            let transaction = read_transaction(&state_file)?;
            PatchTransactionEngine::cleanup(&transaction, delete_branch)?;
            println!("{}", serde_json::to_string(&json!({
                "kind": "patch-transaction",
                "operation": "cleanup",
                "transactionId": transaction.transaction_id,
                "worktreeRemoved": true,
                "branchDeleted": delete_branch,
                "synthetic": false
            }))?);
        }
    }

    Ok(())
}
