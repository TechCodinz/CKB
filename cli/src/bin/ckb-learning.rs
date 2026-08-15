use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use ckb_core::{
    FileDeltaKind, IncrementalArchitectureEngine, LanguageParser, RepositoryAnalysisState,
    VerifiedFileDelta,
};
use serde_json::json;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(name = "ckb-learning", about = "CKB V13 exact incremental architecture learning")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Parse a repository once and persist normalized parsed evidence (no source text).
    Bootstrap {
        #[arg(default_value = ".")]
        root: PathBuf,
        #[arg(long, default_value = ".ckb/parsed-evidence-v1.json")]
        state: PathBuf,
    },
    /// Reparse only verified changed files and rebuild exact cross-file relationships.
    Apply {
        #[arg(default_value = ".")]
        root: PathBuf,
        #[arg(long, default_value = ".ckb/parsed-evidence-v1.json")]
        state: PathBuf,
        #[arg(long = "add")]
        added: Vec<PathBuf>,
        #[arg(long = "modify")]
        modified: Vec<PathBuf>,
        #[arg(long = "delete")]
        deleted: Vec<PathBuf>,
        #[arg(long, default_value = "local-verified-change")]
        source: String,
    },
    /// Inspect persisted parsed-evidence state without reading repository source.
    Status {
        #[arg(long, default_value = ".ckb/parsed-evidence-v1.json")]
        state: PathBuf,
    },
}

fn supported(path: &Path) -> bool {
    matches!(path.extension().and_then(|value| value.to_str()).unwrap_or(""), "ts" | "tsx" | "js" | "jsx" | "mjs" | "py" | "go" | "rs" | "java")
}

fn discover(root: &Path) -> Result<Vec<PathBuf>> {
    let mut output = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        for entry in std::fs::read_dir(&directory).with_context(|| format!("read {}", directory.display()))? {
            let entry = entry?;
            let path = entry.path();
            let name = path.file_name().and_then(|value| value.to_str()).unwrap_or("");
            if path.is_dir() {
                if !matches!(name, ".git" | ".ckb" | "node_modules" | "target" | "dist" | "build" | ".next" | "vendor" | "coverage" | ".turbo" | ".yarn") {
                    stack.push(path);
                }
            } else if supported(&path) {
                output.push(path);
            }
        }
    }
    output.sort();
    Ok(output)
}

fn relative_identity(root: &Path, path: &Path) -> Result<String> {
    let root = root.canonicalize().with_context(|| format!("canonicalize root {}", root.display()))?;
    let absolute = if path.is_absolute() { path.to_path_buf() } else { root.join(path) };
    let absolute = absolute.canonicalize().with_context(|| format!("canonicalize {}", absolute.display()))?;
    let relative = absolute.strip_prefix(&root)
        .map_err(|_| anyhow!("{} is outside repository root {}", absolute.display(), root.display()))?;
    let value = relative.to_string_lossy().replace('\\', "/");
    if value.is_empty() { return Err(anyhow!("repository-relative source path is empty")); }
    Ok(value)
}

fn relative_deleted_identity(root: &Path, path: &Path) -> Result<String> {
    // Deleted files cannot be canonicalized because they no longer exist.
    // Canonicalize only the root and lexically reject parent traversal.
    let root = root.canonicalize().with_context(|| format!("canonicalize root {}", root.display()))?;
    if path.is_absolute() {
        return path.strip_prefix(&root)
            .map(|value| value.to_string_lossy().replace('\\', "/"))
            .map_err(|_| anyhow!("deleted path is outside repository root"));
    }
    let mut parts = Vec::<String>::new();
    for component in path.components() {
        match component {
            std::path::Component::Normal(value) => parts.push(value.to_string_lossy().to_string()),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => return Err(anyhow!("deleted path must not traverse outside repository root")),
            _ => return Err(anyhow!("deleted path must be repository-relative")),
        }
    }
    if parts.is_empty() { return Err(anyhow!("deleted path is empty")); }
    Ok(parts.join("/"))
}

fn read_state(path: &Path) -> Result<RepositoryAnalysisState> {
    let bytes = std::fs::read(path).with_context(|| format!("read incremental state {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("decode incremental state {}", path.display()))
}

fn write_state(path: &Path, state: &RepositoryAnalysisState) -> Result<()> {
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent)?; }
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, serde_json::to_vec_pretty(state)?)?;
    std::fs::rename(&temp, path)?;
    Ok(())
}

fn parse_relative(parser: &LanguageParser, root: &Path, path: &Path) -> Result<ckb_core::FileAnalysis> {
    let relative = relative_identity(root, path)?;
    let absolute = root.canonicalize()?.join(&relative);
    let content = std::fs::read_to_string(&absolute).with_context(|| format!("read {}", absolute.display()))?;
    parser.parse_content(&relative, &content).with_context(|| format!("parse {relative}"))
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    match args.command {
        Command::Bootstrap { root, state } => {
            let parser = LanguageParser::new();
            let files = discover(&root)?;
            let mut analyses = Vec::new();
            let mut parse_errors = Vec::new();
            for file in files {
                match parse_relative(&parser, &root, &file) {
                    Ok(analysis) => analyses.push(analysis),
                    Err(error) => parse_errors.push(error.to_string()),
                }
            }
            if analyses.is_empty() { return Err(anyhow!("no supported source files were parsed")); }
            if !parse_errors.is_empty() {
                // A bootstrap state must represent the completed repository
                // scan exactly. Partial parse success is not promoted into the
                // canonical incremental baseline.
                return Err(anyhow!("bootstrap refused partial parsed evidence: {} file(s) failed; first error: {}", parse_errors.len(), parse_errors[0]));
            }
            let state_value = RepositoryAnalysisState::from_completed_scan(analyses)?;
            let graph = IncrementalArchitectureEngine::graph_from_state(&state_value)?;
            write_state(&state, &state_value)?;
            println!("{}", serde_json::to_string_pretty(&json!({
                "status": "bootstrapped",
                "version": state_value.version,
                "stateFile": state,
                "files": state_value.file_count(),
                "nodes": graph.node_count(),
                "edges": graph.edge_count(),
                "sourceTextPersisted": false,
                "evidencePolicy": "full-verified-parse-baseline",
                "synthetic": false
            }))?);
        }
        Command::Apply { root, state, added, modified, deleted, source } => {
            if added.is_empty() && modified.is_empty() && deleted.is_empty() {
                return Err(anyhow!("provide at least one --add, --modify or --delete file"));
            }
            let parser = LanguageParser::new();
            let mut persisted = read_state(&state)?;
            let current = IncrementalArchitectureEngine::graph_from_state(&persisted)?;
            let mut deltas = Vec::new();
            for path in added {
                let analysis = parse_relative(&parser, &root, &path)?;
                let identity = analysis.path.clone();
                deltas.push(VerifiedFileDelta { path: identity, kind: FileDeltaKind::Add, analysis: Some(analysis), source_digest: None, source: source.clone() });
            }
            for path in modified {
                let analysis = parse_relative(&parser, &root, &path)?;
                let identity = analysis.path.clone();
                deltas.push(VerifiedFileDelta { path: identity, kind: FileDeltaKind::Modify, analysis: Some(analysis), source_digest: None, source: source.clone() });
            }
            for path in deleted {
                deltas.push(VerifiedFileDelta { path: relative_deleted_identity(&root, &path)?, kind: FileDeltaKind::Delete, analysis: None, source_digest: None, source: source.clone() });
            }
            let (_next, report) = IncrementalArchitectureEngine::apply_verified_delta(&current, &mut persisted, deltas)?;
            // Persist only after the complete delta and relationship rebuild
            // succeeds. The state file therefore advances atomically.
            write_state(&state, &persisted)?;
            println!("{}", serde_json::to_string_pretty(&json!({
                "status": "learned",
                "stateFile": state,
                "report": report,
                "sourceTextPersisted": false,
                "synthetic": false
            }))?);
        }
        Command::Status { state } => {
            let persisted = read_state(&state)?;
            let graph = IncrementalArchitectureEngine::graph_from_state(&persisted)?;
            println!("{}", serde_json::to_string_pretty(&json!({
                "status": "ready",
                "version": persisted.version,
                "files": persisted.file_count(),
                "nodes": graph.node_count(),
                "edges": graph.edge_count(),
                "sourceTextPersisted": false,
                "synthetic": false
            }))?);
        }
    }
    Ok(())
}
