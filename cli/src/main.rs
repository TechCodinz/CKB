//! Complete CLI with all features

use clap::{Parser, Subcommand, Args};
use ckb_core::{CkbEngine, ScanReport, DriftViolation, Severity};
use anyhow::Result;
use std::path::PathBuf;
use std::time::Instant;
use indicatif::{ProgressBar, ProgressStyle, MultiProgress};
use comfy_table::{Table, Cell, Color, Attribute, ContentArrangement};
use serde_json::{json, to_string_pretty};
use tokio::fs;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
    
    /// Output format (json, table, quiet)
    #[arg(global = true, short, long, default_value = "table")]
    format: String,
    
    /// Enable verbose output
    #[arg(global = true, short, long)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Scan a codebase and build knowledge graph
    Scan(ScanArgs),
    
    /// Check for architectural drift
    Check(CheckArgs),
    
    /// Analyze impact of a change
    Impact(ImpactArgs),
    
    /// List saved snapshots
    Snapshots(SnapshotsArgs),
    
    /// Compare two snapshots
    Compare(CompareArgs),
    
    /// Export graph to various formats
    Export(ExportArgs),
    
    /// Watch mode - continuously monitor for changes
    Watch(WatchArgs),
    
    /// Suggest architectural improvements
    Suggest(SuggestArgs),
    
    /// Generate architectural documentation
    Docs(DocsArgs),
    
    /// Start MCP server for AI integration
    Serve(ServeArgs),
    
    /// Auto-initialize CKB AI rules (.cursorrules & CLAUDE.md) and MCP configs
    Init(InitArgs),

    /// Validate against architecture rules file
    Validate(ValidateArgs),
    
    /// Interactive TUI mode
    Tui,
}

#[derive(Args)]
struct ScanArgs {
    /// Path to codebase
    path: PathBuf,
    
    /// Include patterns (glob)
    #[arg(short, long)]
    include: Vec<String>,
    
    /// Exclude patterns (glob)
    #[arg(short, long)]
    exclude: Vec<String>,
    
    /// Store snapshot
    #[arg(short, long)]
    store: bool,
    
    /// Max depth for analysis
    #[arg(long, default_value = "10")]
    depth: usize,
}

#[derive(Args)]
struct CheckArgs {
    /// Path to codebase
    path: PathBuf,
    
    /// Exit with error if violations found
    #[arg(short, long)]
    strict: bool,
    
    /// Fail on severity level
    #[arg(long, default_value = "warning")]
    fail_on: SeverityLevel,
    
    /// Output file for results
    #[arg(short, long)]
    output: Option<PathBuf>,
    
    /// Format for output (json, sarif, junit)
    #[arg(long, default_value = "human")]
    report_format: String,
}

#[derive(Args)]
struct ImpactArgs {
    /// Path to codebase
    path: PathBuf,
    
    /// File to analyze
    file: PathBuf,
    
    /// Line number
    line: u32,
    
    /// Type of change
    #[arg(short, long, default_value = "modify")]
    change: String,
    
    /// Show detailed impact tree
    #[arg(short, long)]
    tree: bool,
}

#[derive(Args)]
struct SnapshotsArgs {
    /// Path to codebase
    path: PathBuf,
    
    /// Show detailed info
    #[arg(short, long)]
    verbose: bool,
}

#[derive(Args)]
struct CompareArgs {
    /// First snapshot ID
    snapshot1: String,
    
    /// Second snapshot ID
    snapshot2: String,
    
    /// Path to codebase
    #[arg(short, long)]
    path: PathBuf,
}

#[derive(Args)]
struct ExportArgs {
    /// Path to codebase
    path: PathBuf,
    
    /// Output format (dot, json, graphml, mermaid, svg)
    #[arg(short, long, default_value = "dot")]
    format: String,
    
    /// Output file
    #[arg(short, long)]
    output: PathBuf,
}

#[derive(Args)]
struct WatchArgs {
    /// Path to codebase
    path: PathBuf,
    
    /// Interval in seconds
    #[arg(short, long, default_value = "5")]
    interval: u64,
    
    /// Command to run on changes
    #[arg(short, long)]
    exec: Option<String>,
}

#[derive(Args)]
struct SuggestArgs {
    /// Path to codebase
    path: PathBuf,

    /// Number of suggestions
    #[arg(short, long, default_value = "5")]
    count: usize,

    /// Focus area (short is -F: the global -f/--format already owns -f,
    /// and reusing it here made clap panic in debug builds)
    #[arg(short = 'F', long)]
    focus: Option<String>,
}

#[derive(Args)]
struct DocsArgs {
    /// Path to codebase
    path: PathBuf,
    
    /// Output directory
    #[arg(short, long)]
    output: PathBuf,
    
    /// Include diagrams
    #[arg(short, long)]
    diagrams: bool,
}

#[derive(Args)]
struct ServeArgs {
    /// Host address (short is -H: -h is clap's auto --help flag, and
    /// claiming it here made clap panic in debug builds)
    #[arg(short = 'H', long, default_value = "127.0.0.1")]
    host: String,
    
    /// Port
    #[arg(short, long, default_value = "3000")]
    port: u16,

    /// Use standard I/O (Stdio JSON-RPC) transport for MCP protocol
    #[arg(long)]
    stdio: bool,
    
    /// Enable CORS
    #[arg(long)]
    cors: bool,
    
    /// Enable authentication
    #[arg(long)]
    auth: bool,
    
    /// API key (if auth enabled)
    #[arg(long)]
    api_key: Option<String>,
}

#[derive(Args)]
struct InitArgs {
    /// Path to target codebase (defaults to current directory)
    #[arg(default_value = ".")]
    path: PathBuf,

    /// Force overwrite existing rule files (short is -F: the global
    /// -f/--format already owns -f, and reusing it here made clap panic
    /// in debug builds)
    #[arg(short = 'F', long)]
    force: bool,
}

#[derive(Args)]
struct ValidateArgs {
    /// Path to codebase
    path: PathBuf,
    
    /// Architecture rules file
    #[arg(short, long)]
    rules: PathBuf,
}

#[derive(clap::ValueEnum, Clone)]
enum SeverityLevel {
    Info,
    Warning,
    Error,
    Critical,
}

impl From<SeverityLevel> for Severity {
    fn from(level: SeverityLevel) -> Self {
        match level {
            SeverityLevel::Info => Severity::Info,
            SeverityLevel::Warning => Severity::Warning,
            SeverityLevel::Error => Severity::Error,
            SeverityLevel::Critical => Severity::Critical,
        }
    }
}

/// True when the requested output format is machine-readable (JSON, SARIF,
/// JUnit XML). For those formats stdout must carry *only* the payload —
/// human chrome (headers, banners) goes to stderr so the output can be
/// piped straight into `json.tool`, a SARIF uploader, or `JSON.parse`
/// (the VS Code extension's `runCliJson` does exactly that and previously
/// could never succeed against this CLI).
fn is_machine_format(format: &str) -> bool {
    matches!(format, "json" | "sarif" | "junit")
}

/// Print a human-facing header line: stdout for human formats, stderr
/// when a machine format owns stdout.
fn print_header(machine: bool, line: String) {
    if machine {
        eprintln!("{}", line);
    } else {
        println!("{}", line);
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    
    // Setup logging
    if cli.verbose {
        tracing_subscriber::fmt::init();
    }
    
    match cli.command {
        Commands::Scan(args) => scan_command(args, cli.format).await?,
        Commands::Check(args) => check_command(args, cli.format).await?,
        Commands::Impact(args) => impact_command(args, cli.format).await?,
        Commands::Snapshots(args) => snapshots_command(args, cli.format).await?,
        Commands::Compare(args) => compare_command(args, cli.format).await?,
        Commands::Export(args) => export_command(args).await?,
        Commands::Watch(args) => watch_command(args).await?,
        Commands::Suggest(args) => suggest_command(args, cli.format).await?,
        Commands::Docs(args) => docs_command(args).await?,
        Commands::Serve(args) => serve_command(args).await?,
        Commands::Init(args) => init_command(args).await?,
        Commands::Validate(args) => validate_command(args, cli.format).await?,
        Commands::Tui => tui_command().await?,
    }
    
    Ok(())
}

async fn init_command(args: InitArgs) -> Result<()> {
    println!("🚀 Initializing CKB for AI tools at: {}", args.path.display());

    let engine = CkbEngine::new()?;
    let path_str = args.path.to_string_lossy();
    let report = engine.scan_codebase(&path_str).await?;

    let rules_content = engine.generate_ai_rules().await?;

    let cursorrules_path = args.path.join(".cursorrules");
    let claudemd_path = args.path.join("CLAUDE.md");

    tokio::fs::write(&cursorrules_path, &rules_content).await?;
    tokio::fs::write(&claudemd_path, &rules_content).await?;

    println!("✅ Generated .cursorrules and CLAUDE.md guidelines based on {} scanned nodes.", report.nodes);
    println!("🎉 CKB is ready! To launch standard MCP server, run: ckb serve --stdio");

    Ok(())
}

async fn scan_command(args: ScanArgs, format: String) -> Result<()> {
    let start = Instant::now();

    print_header(
        is_machine_format(&format),
        format!("🔍 CKB Scan - Analyzing codebase: {}", args.path.display()),
    );
    
    let multi = MultiProgress::new();
    let scan_pb = multi.add(ProgressBar::new_spinner());
    scan_pb.set_style(ProgressStyle::default_spinner()
        .template("{spinner:.green} {msg}")?
        .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]));
    scan_pb.set_message("Discovering files...");
    
    let engine = CkbEngine::new()?;
    // Configure scan options...
    
    scan_pb.set_message("Parsing files and building graph...");
    
    let report = engine.scan_codebase(&args.path.to_string_lossy()).await?;
    
    scan_pb.finish_with_message("✅ Scan complete!");
    
    let duration = start.elapsed();
    
    match format.as_str() {
        "json" => {
            let output = json!({
                "duration_ms": duration.as_millis(),
                "report": report,
            });
            println!("{}", to_string_pretty(&output)?);
        }
        _ => {
            display_scan_report(&report, duration);
        }
    }
    
    Ok(())
}

async fn check_command(args: CheckArgs, format: String) -> Result<()> {
    // `--report-format` is the check-specific format selector (it's what
    // the README's headline `ckb check --strict --report-format sarif`
    // example uses). It was previously defined but never read — the
    // command silently switched on the global `--format` instead. An
    // explicit non-default `--report-format` now wins; otherwise the
    // global format applies, exactly as before.
    let report_format = if args.report_format != "human" {
        args.report_format.clone()
    } else {
        format.clone()
    };
    let machine = is_machine_format(&report_format);
    print_header(
        machine,
        format!(
            "🔍 CKB Check - Analyzing architectural drift: {}",
            args.path.display()
        ),
    );

    let engine = CkbEngine::new()?;
    let report = engine.scan_codebase(&args.path.to_string_lossy()).await?;

    // Render the payload once so stdout and --output never disagree.
    let payload: Option<String> = match report_format.as_str() {
        "json" => Some(to_string_pretty(&report)?),
        "sarif" => Some(to_string_pretty(&generate_sarif(&report))?),
        "junit" => Some(generate_junit(&report)),
        _ => None,
    };

    match &payload {
        Some(payload) => println!("{}", payload),
        None => display_drift_report(&report.drift),
    }

    if let Some(output) = &args.output {
        // The report file carries the payload in the selected report
        // format. For the human/table format there is no file rendering,
        // so the file keeps the JSON report it has always carried (the
        // ckb-scan GitHub Action parses `.drift` out of it).
        let file_contents = match &payload {
            Some(payload) => payload.clone(),
            None => to_string_pretty(&report)?,
        };
        fs::write(output, file_contents).await?;
    }

    // Previously `--strict` and `--fail-on` were parsed but never actually
    // used anywhere in this function — `ckb check` always exited 0 no matter
    // how many (or how severe) violations were found, which makes it
    // useless as a CI gate despite being built for exactly that (it's the
    // subcommand the GitHub Action below calls). Now it actually fails the
    // process when `--strict` is set and any violation meets or exceeds
    // `--fail-on`'s severity.
    let threshold: Severity = args.fail_on.into();
    let blocking_violations = report.drift.iter().filter(|v| v.severity >= threshold).count();

    if args.strict && blocking_violations > 0 {
        eprintln!(
            "\n❌ CKB check failed: {} violation(s) at or above '{:?}' severity (--strict is set).",
            blocking_violations, threshold
        );
        std::process::exit(1);
    }
    
    Ok(())
}

async fn impact_command(args: ImpactArgs, format: String) -> Result<()> {
    print_header(
        is_machine_format(&format),
        format!(
            "🔍 CKB Impact - Analyzing change impact at {}:{}",
            args.file.display(),
            args.line
        ),
    );
    
    let engine = CkbEngine::new()?;
    engine.scan_codebase(&args.path.to_string_lossy()).await?;
    
    let change_type = match args.change.as_str() {
        "add" => ckb_core::ChangeType::Add,
        "modify" => ckb_core::ChangeType::Modify,
        "delete" => ckb_core::ChangeType::Delete,
        "rename" => ckb_core::ChangeType::Rename,
        _ => ckb_core::ChangeType::Modify,
    };
    
    let impact = engine.analyze_impact(
        &args.file.to_string_lossy(), 
        args.line, 
        change_type
    ).await?;
    
    match format.as_str() {
        "json" => {
            println!("{}", to_string_pretty(&impact)?);
        }
        _ => {
            display_impact_report(&impact, args.tree);
        }
    }
    
    Ok(())
}

async fn snapshots_command(args: SnapshotsArgs, _format: String) -> Result<()> {
    // Snapshots live in the engine's store (./ckb_data under the working
    // directory the scans ran from). This command previously printed
    // "No snapshots found" unconditionally — the store was never read.
    let engine = CkbEngine::new()?;
    let snapshots = engine.list_snapshots().await?;

    if snapshots.is_empty() {
        println!(
            "No snapshots found for {} (store ./ckb_data holds none; run `ckb scan --store` first)",
            args.path.display()
        );
        return Ok(());
    }

    println!("📸 {} snapshot(s) in store:", snapshots.len());
    for snapshot in &snapshots {
        if args.verbose {
            println!(
                "  {}  {}  {} nodes, {} edges",
                snapshot.id,
                snapshot.timestamp.format("%Y-%m-%d %H:%M:%S UTC"),
                snapshot.node_count,
                snapshot.edge_count
            );
        } else {
            println!("  {}", snapshot.id);
        }
    }
    Ok(())
}

async fn compare_command(args: CompareArgs, format: String) -> Result<()> {
    let machine = is_machine_format(&format);
    print_header(
        machine,
        format!(
            "🔍 CKB Compare - {} → {}",
            args.snapshot1, args.snapshot2
        ),
    );

    // This command previously ignored its arguments and returned Ok(())
    // — exit 0 with zero bytes of output, even for snapshot IDs that do
    // not exist. Snapshots are looked up in the same store scans write
    // to; a missing snapshot is a hard error, never a silent success.
    let engine = CkbEngine::new()?;
    let before = engine
        .load_snapshot(&args.snapshot1)
        .await?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Snapshot not found: {} (looked in ./ckb_data under the current directory; \
                 list stored snapshots with `ckb snapshots <path>`)",
                args.snapshot1
            )
        })?;
    let after = engine
        .load_snapshot(&args.snapshot2)
        .await?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Snapshot not found: {} (looked in ./ckb_data under the current directory; \
                 list stored snapshots with `ckb snapshots <path>`)",
                args.snapshot2
            )
        })?;

    let delta = engine.compare_snapshots(&before, &after)?;

    match format.as_str() {
        "json" => {
            let output = json!({
                "snapshot_before": args.snapshot1,
                "snapshot_after": args.snapshot2,
                "delta": delta,
            });
            println!("{}", to_string_pretty(&output)?);
        }
        _ => display_snapshot_delta(&delta),
    }

    Ok(())
}

async fn export_command(args: ExportArgs) -> Result<()> {
    println!("🔍 CKB Export - Exporting graph to {} format", args.format);
    
    let engine = CkbEngine::new()?;
    let report = engine.scan_codebase(&args.path.to_string_lossy()).await?;
    
    match args.format.as_str() {
        "dot" => {
            let dot = generate_dot(&report);
            fs::write(&args.output, dot).await?;
        }
        "json" => {
            fs::write(&args.output, to_string_pretty(&report)?).await?;
        }
        "graphml" => {
            let graphml = generate_graphml(&report);
            fs::write(&args.output, graphml).await?;
        }
        "mermaid" => {
            let mermaid = generate_mermaid(&report);
            fs::write(&args.output, mermaid).await?;
        }
        "svg" => {
            println!("SVG export requires GraphViz installed");
        }
        _ => anyhow::bail!("Unsupported format: {}", args.format),
    }
    
    println!("✅ Exported to {}", args.output.display());
    
    Ok(())
}

async fn watch_command(args: WatchArgs) -> Result<()> {
    println!("🔍 CKB Watch - Monitoring {} every {} seconds", 
        args.path.display(), args.interval);
    println!("   Press Ctrl+C to stop.\n");
    
    let mut previous_hash = String::new();
    let mut previous_violation_count: Option<usize> = None;
    
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(args.interval)).await;
        
        let engine = CkbEngine::new()?;
        let report = match engine.scan_codebase(&args.path.to_string_lossy()).await {
            Ok(r) => r,
            Err(e) => {
                eprintln!("⚠️  Scan failed, will retry next interval: {}", e);
                continue;
            }
        };
        
        let current_hash = report_fingerprint(&report);
        
        if current_hash != previous_hash {
            println!("🔄 Change detected at {}", chrono::Local::now().format("%H:%M:%S"));
            
            if report.drift.is_empty() {
                println!("✅ No violations detected ({} files, {} nodes).\n", report.files_processed, report.nodes);
            } else {
                // Previously this printed "Found N violations" then, if N > 3,
                // jumped straight to "...and N-3 more" without ever printing
                // the first 3 — the actual violation details never appeared
                // in watch mode at all. Reusing the same table renderer
                // `ckb check` uses so watch mode is actually useful to read.
                display_drift_report(&report.drift);
            }

            if let Some(prev_count) = previous_violation_count {
                let current_count = report.drift.len();
                if current_count > prev_count {
                    println!("   📈 {} new violation(s) since last change.\n", current_count - prev_count);
                } else if current_count < prev_count {
                    println!("   📉 {} violation(s) resolved since last change.\n", prev_count - current_count);
                }
            }
            previous_violation_count = Some(report.drift.len());
            
            if let Some(cmd) = &args.exec {
                println!("▶️  Executing: {}", cmd);
                match tokio::process::Command::new("sh").arg("-c").arg(cmd).output().await {
                    Ok(output) => {
                        if !output.status.success() {
                            eprintln!("   ⚠️  Command exited with status {}", output.status);
                        }
                    }
                    Err(e) => eprintln!("   ⚠️  Failed to run command: {}", e),
                }
            }
            
            previous_hash = current_hash;
        }
    }
}

async fn suggest_command(args: SuggestArgs, format: String) -> Result<()> {
    print_header(
        is_machine_format(&format),
        "🔍 CKB Suggest - Finding architectural improvements".to_string(),
    );
    
    let engine = CkbEngine::new()?;
    let report = engine.scan_codebase(&args.path.to_string_lossy()).await?;
    
    let suggestions = generate_suggestions(&report, args.count, args.focus)?;
    
    match format.as_str() {
        "json" => {
            println!("{}", to_string_pretty(&suggestions)?);
        }
        _ => {
            display_suggestions(&suggestions);
        }
    }
    
    Ok(())
}

async fn docs_command(args: DocsArgs) -> Result<()> {
    println!("🔍 CKB Docs - Generating architecture documentation");
    
    let engine = CkbEngine::new()?;
    let report = engine.scan_codebase(&args.path.to_string_lossy()).await?;
    
    fs::create_dir_all(&args.output).await?;
    
    let docs = generate_architecture_docs(&report, args.diagrams)?;
    fs::write(args.output.join("ARCHITECTURE.md"), docs).await?;
    
    if args.diagrams {
        let mermaid = generate_mermaid(&report);
        fs::write(args.output.join("diagram.mmd"), mermaid).await?;
    }
    
    println!("✅ Documentation generated in {}", args.output.display());
    
    Ok(())
}

async fn validate_command(args: ValidateArgs, format: String) -> Result<()> {
    print_header(
        is_machine_format(&format),
        "🔍 CKB Validate - Validating against custom rules".to_string(),
    );
    
    let rules_content = fs::read_to_string(&args.rules).await?;
    let rules: Vec<CustomRule> = serde_json::from_str(&rules_content)?;
    
    let engine = CkbEngine::new()?;
    let report = engine.scan_codebase(&args.path.to_string_lossy()).await?;
    
    let violations = validate_rules(&report, &rules)?;
    
    match format.as_str() {
        "json" => {
            println!("{}", to_string_pretty(&violations)?);
        }
        _ => {
            display_custom_violations(&violations);
        }
    }
    
    Ok(())
}

/// Stable content fingerprint of a scan report, for watch-mode change
/// detection. Deliberately excludes everything that changes on every scan
/// even when the code doesn't — `snapshot_id` (a fresh UUID per scan),
/// `duration_ms` (measured wall-clock), and violation `id`s (fresh UUIDs).
/// Hashing the pretty-printed report, as the watch loop previously did,
/// therefore "detected a change" on *every* poll and re-ran `--exec`
/// forever on an untouched codebase.
fn report_fingerprint(report: &ScanReport) -> String {
    let mut drift: Vec<String> = report
        .drift
        .iter()
        .map(|v| {
            format!(
                "{:?}|{}|{}|{}|{:?}|{}",
                v.kind, v.from.0, v.to.0, v.boundary, v.severity, v.message
            )
        })
        .collect();
    drift.sort();

    let mut patterns: Vec<String> = report
        .patterns
        .iter()
        .map(|p| {
            let mut boundaries: Vec<&str> =
                p.boundaries.iter().map(|b| b.name.as_str()).collect();
            boundaries.sort();
            format!("{}|{}", p.name, boundaries.join(","))
        })
        .collect();
    patterns.sort();

    let mut external_dependencies = report.external_dependencies.clone();
    external_dependencies.sort();

    let fingerprint = json!({
        "files_processed": report.files_processed,
        "nodes": report.nodes,
        "edges": report.edges,
        "drift": drift,
        "patterns": patterns,
        "package_identity": report.package_identity,
        "external_dependencies": external_dependencies,
    });
    format!("{:x}", md5::compute(fingerprint.to_string()))
}

async fn serve_command(args: ServeArgs) -> Result<()> {
    // The REST/MCP server implementation lives in the `ckb-mcp-server`
    // crate (binary `ckb-mcp-server`). `ckb serve` delegates to that
    // binary — previously this function printed "Starting CKB MCP
    // server…" and returned immediately without ever binding a port.
    // The child inherits stdio, so `--stdio` JSON-RPC works through this
    // delegation unchanged, and Ctrl+C reaches the real server.
    let server_bin = locate_mcp_server_binary().ok_or_else(|| {
        anyhow::anyhow!(
            "ckb-mcp-server binary not found (looked next to the ckb executable and on PATH). \
             Build/install it with `cargo build --release -p ckb-mcp-server` or \
             `cargo install --path mcp-server`, then re-run `ckb serve`."
        )
    })?;

    let mut cmd = tokio::process::Command::new(&server_bin);
    if args.stdio {
        cmd.arg("--stdio");
    }
    // The server reads its port from the PORT env var.
    cmd.env("PORT", args.port.to_string());

    // The server binds loopback by default and only binds all interfaces
    // when CKB_BIND_ALL=1 is set, so map --host onto that contract and
    // say plainly when a requested host can't be honoured exactly.
    match args.host.as_str() {
        "0.0.0.0" | "::" => {
            cmd.env("CKB_BIND_ALL", "1");
        }
        "127.0.0.1" | "localhost" | "::1" => {}
        other => {
            eprintln!(
                "⚠️  ckb-mcp-server binds either loopback (default) or all interfaces; \
                 --host {other} can't be honoured exactly, binding loopback instead. \
                 Use --host 0.0.0.0 to bind all interfaces."
            );
        }
    }

    if args.cors {
        // "Enable CORS" in the server's terms means an explicit allowed
        // origin; "*" is the only origin the --cors flag can mean. The
        // server itself logs a wide-open warning for it.
        if std::env::var_os("CKB_ALLOWED_ORIGIN").is_none() {
            cmd.env("CKB_ALLOWED_ORIGIN", "*");
        }
        eprintln!("⚠️  CORS enabled for the server process (CKB_ALLOWED_ORIGIN).");
    }

    if let Some(key) = &args.api_key {
        cmd.env("CKB_API_KEY", key);
    }
    if args.auth && args.api_key.is_none() && std::env::var_os("CKB_API_KEY").is_none() {
        anyhow::bail!(
            "serve --auth requires an API key: pass --api-key <key> or set CKB_API_KEY. \
             Without one the server would run unauthenticated."
        );
    }

    eprintln!(
        "🚀 Delegating to {} on {}:{} (Ctrl+C stops the server)",
        server_bin.display(),
        args.host,
        args.port
    );

    let status = cmd
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "Failed to launch ckb-mcp-server at {}: {}",
                server_bin.display(),
                e
            )
        })?;

    if !status.success() {
        anyhow::bail!("ckb-mcp-server exited with status {}", status);
    }
    Ok(())
}

/// Find the `ckb-mcp-server` binary: first as a sibling of the running
/// executable (the layout `cargo build`/`cargo install` produce), then
/// on `PATH`.
fn locate_mcp_server_binary() -> Option<PathBuf> {
    let bin_name = if cfg!(windows) {
        "ckb-mcp-server.exe"
    } else {
        "ckb-mcp-server"
    };
    if let Ok(exe) = std::env::current_exe() {
        let sibling = exe.with_file_name(bin_name);
        if sibling.is_file() {
            return Some(sibling);
        }
    }
    if let Ok(paths) = std::env::var("PATH") {
        for dir in std::env::split_paths(&paths) {
            let candidate = dir.join(bin_name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

async fn tui_command() -> Result<()> {
    println!("🚀 Starting CKB TUI...");
    println!("Interactive TUI coming soon!");
    Ok(())
}

fn display_scan_report(report: &ScanReport, duration: std::time::Duration) {
    println!();
    let mut table = Table::new();
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table.add_row(vec![
        Cell::new("Files Processed").add_attribute(Attribute::Bold),
        Cell::new(report.files_processed),
    ]);
    table.add_row(vec![
        Cell::new("Nodes").add_attribute(Attribute::Bold),
        Cell::new(report.nodes),
    ]);
    table.add_row(vec![
        Cell::new("Edges").add_attribute(Attribute::Bold),
        Cell::new(report.edges),
    ]);
    table.add_row(vec![
        Cell::new("Patterns Detected").add_attribute(Attribute::Bold),
        Cell::new(report.patterns.len()),
    ]);
    table.add_row(vec![
        Cell::new("Violations").add_attribute(Attribute::Bold),
        Cell::new(report.drift.len()).fg(if report.drift.is_empty() { Color::Green } else { Color::Red }),
    ]);
    table.add_row(vec![
        Cell::new("Duration").add_attribute(Attribute::Bold),
        Cell::new(format!("{:.2}s", duration.as_secs_f64())),
    ]);
    println!("{table}");

    if !report.patterns.is_empty() {
        println!("\n📐 Detected Patterns:");
        for p in &report.patterns {
            println!("  • {} (confidence: {:.0}%)", p.name, p.confidence * 100.0);
        }
    }

    if !report.drift.is_empty() {
        println!();
        display_drift_report(&report.drift);
    } else {
        println!("\n✅ No architectural violations found!");
    }
}

fn display_drift_report(violations: &[DriftViolation]) {
    println!("❌ Architectural Violations ({}):", violations.len());
    let mut table = Table::new();
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(vec![
        Cell::new("Severity").add_attribute(Attribute::Bold),
        Cell::new("Type").add_attribute(Attribute::Bold),
        Cell::new("Message").add_attribute(Attribute::Bold),
        Cell::new("Fix").add_attribute(Attribute::Bold),
    ]);

    for v in violations {
        let severity_color = match v.severity {
            Severity::Critical => Color::Red,
            Severity::Error => Color::Red,
            Severity::Warning => Color::Yellow,
            Severity::Info => Color::Cyan,
        };
        table.add_row(vec![
            Cell::new(format!("{:?}", v.severity)).fg(severity_color),
            Cell::new(format!("{:?}", v.kind)),
            Cell::new(&v.message),
            Cell::new(v.suggested_fix.as_deref().unwrap_or("-")),
        ]);
    }
    println!("{table}");
}

fn display_impact_report(impact: &ckb_core::ImpactAnalysis, show_tree: bool) {
    println!("\n📊 Impact Analysis:");
    println!("  Risk Score: {:.0}%", impact.risk_score * 100.0);
    println!("  Estimated Effort: {}", impact.estimated_effort);
    println!("  Direct impacts: {}", impact.direct_impacts.len());
    println!("  Indirect impacts: {}", impact.indirect_impacts.len());

    if !impact.direct_impacts.is_empty() {
        println!("\n  Direct:");
        for node in &impact.direct_impacts {
            println!("    ├─ {}:{} ({:?}, {:.0}%)",
                node.path.display(), node.line, node.impact_kind, node.confidence * 100.0);
        }
    }

    if show_tree && !impact.indirect_impacts.is_empty() {
        println!("\n  Indirect:");
        for node in &impact.indirect_impacts {
            println!("    ├─ {}:{} ({:?})", node.path.display(), node.line, node.impact_kind);
        }
    }
}

#[allow(dead_code)]
fn display_snapshots(snapshots: &[ckb_core::SnapshotMetadata], _verbose: bool) {
    if snapshots.is_empty() {
        println!("No snapshots found.");
        return;
    }
    let mut table = Table::new();
    table.set_header(vec!["ID", "Timestamp", "Nodes", "Edges"]);
    for s in snapshots {
        table.add_row(vec![
            &s.id[..8],
            &s.timestamp.to_string(),
            &s.node_count.to_string(),
            &s.edge_count.to_string(),
        ]);
    }
    println!("{table}");
}

fn display_suggestions(suggestions: &[ArchitectureSuggestion]) {
    if suggestions.is_empty() {
        println!("✅ No suggestions — architecture looks healthy!");
        return;
    }
    for (i, s) in suggestions.iter().enumerate() {
        println!("\n{}. {} (priority: {:.1})", i + 1, s.title, s.priority);
        println!("   {}", s.description);
        for (j, step) in s.steps.iter().enumerate() {
            println!("   {}.{} {}", i + 1, j + 1, step);
        }
    }
}

fn generate_sarif(report: &ScanReport) -> serde_json::Value {
    json!({
        "$schema": "https://raw.githubusercontent.com/oasis-tcs/sarif-spec/master/Schemata/sarif-schema-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": { "driver": { "name": "CKB", "version": "0.1.0" } },
            "results": report.drift.iter().map(|v| json!({
                "ruleId": format!("{:?}", v.kind),
                "level": match v.severity {
                    Severity::Critical | Severity::Error => "error",
                    Severity::Warning => "warning",
                    Severity::Info => "note",
                },
                "message": { "text": v.message },
                "locations": [{
                    "physicalLocation": {
                        "artifactLocation": { "uri": v.from.0 }
                    }
                }]
            })).collect::<Vec<_>>()
        }]
    })
}

fn generate_junit(report: &ScanReport) -> String {
    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    xml.push_str(&format!("<testsuite name=\"CKB\" tests=\"{}\" failures=\"{}\">\n",
        report.drift.len(), report.drift.len()));
    for v in &report.drift {
        xml.push_str(&format!(
            "  <testcase name=\"{:?}\" classname=\"{}\">\n    <failure message=\"{}\" type=\"{:?}\"/>\n  </testcase>\n",
            v.kind, v.boundary, v.message, v.severity
        ));
    }
    xml.push_str("</testsuite>\n");
    xml
}

fn generate_dot(report: &ScanReport) -> String {
    let mut dot = String::from("digraph CKB {\n  rankdir=TB;\n  node [shape=box, style=rounded];\n\n");
    // Add pattern boundaries as subgraphs
    for (i, pattern) in report.patterns.iter().enumerate() {
        dot.push_str(&format!("  subgraph cluster_{} {{\n    label=\"{}\";\n", i, pattern.name));
        for boundary in &pattern.boundaries {
            for node in &boundary.nodes {
                let safe_id = node.0.replace("::", "_").replace("/", "_").replace("\\", "_").replace(".", "_");
                dot.push_str(&format!("    \"{}\" [label=\"{}\"];\n", safe_id, node.0.split("::").last().unwrap_or(&node.0)));
            }
        }
        dot.push_str("  }\n");
    }
    // Add violations as red edges
    for v in &report.drift {
        let from = v.from.0.replace("::", "_").replace("/", "_").replace("\\", "_").replace(".", "_");
        let to = v.to.0.replace("::", "_").replace("/", "_").replace("\\", "_").replace(".", "_");
        dot.push_str(&format!("  \"{}\" -> \"{}\" [color=red, label=\"{:?}\"];\n", from, to, v.kind));
    }
    dot.push_str("}\n");
    dot
}

fn generate_graphml(report: &ScanReport) -> String {
    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<graphml>\n<graph id=\"G\" edgedefault=\"directed\">\n");
    let mut node_ids = std::collections::HashSet::new();
    for v in &report.drift {
        if node_ids.insert(v.from.0.clone()) {
            xml.push_str(&format!("  <node id=\"{}\"/>\n", v.from.0));
        }
        if node_ids.insert(v.to.0.clone()) {
            xml.push_str(&format!("  <node id=\"{}\"/>\n", v.to.0));
        }
        xml.push_str(&format!("  <edge source=\"{}\" target=\"{}\"/>\n", v.from.0, v.to.0));
    }
    xml.push_str("</graph>\n</graphml>\n");
    xml
}

fn generate_mermaid(report: &ScanReport) -> String {
    let mut mmd = String::from("graph TD\n");
    for pattern in report.patterns.iter() {
        mmd.push_str(&format!("  subgraph {}\n", pattern.name.replace(' ', "_")));
        for boundary in &pattern.boundaries {
            for node_id in &boundary.nodes {
                let label = node_id.0.split("::").last().unwrap_or(&node_id.0);
                let safe = node_id.0.replace("::", "_").replace("/", "_").replace("\\", "_").replace(".", "_");
                mmd.push_str(&format!("    {}[\"{}\"]\n", safe, label));
            }
        }
        mmd.push_str("  end\n");
    }
    for v in &report.drift {
        let from = v.from.0.replace("::", "_").replace("/", "_").replace("\\", "_").replace(".", "_");
        let to = v.to.0.replace("::", "_").replace("/", "_").replace("\\", "_").replace(".", "_");
        mmd.push_str(&format!("  {} -->|{:?}| {}\n", from, v.kind, to));
    }
    mmd
}

fn generate_architecture_docs(report: &ScanReport, include_diagrams: bool) -> Result<String> {
    let mut doc = String::from("# Architecture Documentation\n\n");
    doc.push_str(&format!("Generated by CKB on {}\n\n", chrono::Local::now().format("%Y-%m-%d %H:%M")));
    doc.push_str(&format!("## Overview\n- **Files**: {}\n- **Nodes**: {}\n- **Edges**: {}\n\n",
        report.files_processed, report.nodes, report.edges));

    if !report.patterns.is_empty() {
        doc.push_str("## Detected Patterns\n\n");
        for p in &report.patterns {
            doc.push_str(&format!("### {} ({:.0}% confidence)\n{}\n\n", p.name, p.confidence * 100.0, p.description));
            for b in &p.boundaries {
                doc.push_str(&format!("- **{}**: {} nodes\n", b.name, b.nodes.len()));
            }
            doc.push('\n');
        }
    }

    if !report.drift.is_empty() {
        doc.push_str("## Violations\n\n| Severity | Type | Message |\n|---|---|---|\n");
        for v in &report.drift {
            doc.push_str(&format!("| {:?} | {:?} | {} |\n", v.severity, v.kind, v.message));
        }
    }

    if include_diagrams {
        doc.push_str("\n## Dependency Diagram\n\n```mermaid\n");
        doc.push_str(&generate_mermaid(report));
        doc.push_str("```\n");
    }

    Ok(doc)
}

#[derive(Debug, serde::Deserialize)]
struct CustomRule {
    name: String,
    description: String,
    pattern: String,
    severity: Severity,
}

fn validate_rules(report: &ScanReport, rules: &[CustomRule]) -> Result<Vec<DriftViolation>> {
    let mut violations = Vec::new();
    // Match custom rule patterns against the report
    for rule in rules {
        for pattern in &report.patterns {
            for boundary in &pattern.boundaries {
                for node_id in &boundary.nodes {
                    if node_id.0.contains(&rule.pattern) {
                        violations.push(DriftViolation {
                            id: uuid::Uuid::new_v4(),
                            kind: ckb_core::ViolationKind::BoundaryCrossing,
                            from: node_id.clone(),
                            to: node_id.clone(),
                            boundary: rule.name.clone(),
                            message: format!("{}: {}", rule.name, rule.description),
                            severity: rule.severity,
                            suggested_fix: Some(format!("Review node matching pattern '{}'", rule.pattern)),
                        });
                    }
                }
            }
        }
    }
    Ok(violations)
}

#[derive(Debug, serde::Serialize)]
struct ArchitectureSuggestion {
    title: String,
    description: String,
    priority: f32,
    steps: Vec<String>,
    example: Option<String>,
}

fn generate_suggestions(report: &ScanReport, count: usize, _focus: Option<String>) -> Result<Vec<ArchitectureSuggestion>> {
    let mut suggestions = Vec::new();

    // Generate suggestions based on violations
    let circular = report.drift.iter().filter(|v| matches!(v.kind, ckb_core::ViolationKind::CircularDependency)).count();
    if circular > 0 {
        suggestions.push(ArchitectureSuggestion {
            title: "Break Circular Dependencies".to_string(),
            description: format!("{} circular dependencies found. Extract shared interfaces.", circular),
            priority: 9.0,
            steps: vec![
                "Identify the shared concepts creating the cycle".to_string(),
                "Extract an interface/trait into a separate module".to_string(),
                "Have both modules depend on the interface instead of each other".to_string(),
            ],
            example: None,
        });
    }

    let god_objects = report.drift.iter().filter(|v| matches!(v.kind, ckb_core::ViolationKind::GodObject)).count();
    if god_objects > 0 {
        suggestions.push(ArchitectureSuggestion {
            title: "Split God Objects".to_string(),
            description: format!("{} modules have too many dependencies. Consider splitting.", god_objects),
            priority: 7.0,
            steps: vec![
                "Identify cohesive groups of functionality within the module".to_string(),
                "Extract each group into its own module".to_string(),
                "Update imports to reference the new modules".to_string(),
            ],
            example: None,
        });
    }

    if report.patterns.is_empty() {
        suggestions.push(ArchitectureSuggestion {
            title: "Establish Architectural Patterns".to_string(),
            description: "No clear architecture detected. Consider organizing code into layers.".to_string(),
            priority: 8.0,
            steps: vec![
                "Choose an architecture (layered, hexagonal, etc.)".to_string(),
                "Create directory structure matching boundaries".to_string(),
                "Move files to appropriate directories".to_string(),
                "Add a .ckb/rules.json file to enforce boundaries".to_string(),
            ],
            example: None,
        });
    }

    suggestions.sort_by(|a, b| b.priority.partial_cmp(&a.priority).unwrap_or(std::cmp::Ordering::Equal));
    suggestions.truncate(count);
    Ok(suggestions)
}

fn format_change(change: i64) -> String {
    if change > 0 { format!("+{}", change) }
    else if change < 0 { format!("{}", change) }
    else { "0".to_string() }
}

fn signed_delta(before: usize, after: usize) -> String {
    format_change(after as i64 - before as i64)
}

/// Human rendering of a snapshot delta. The JSON form (`--format json`)
/// carries the full added/removed lists; here the lists are capped so a
/// large refactor doesn't flood the terminal.
fn display_snapshot_delta(delta: &ckb_core::SnapshotDelta) {
    const CAP: usize = 20;

    let mut table = Table::new();
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(vec![
        Cell::new("Metric").add_attribute(Attribute::Bold),
        Cell::new("Before").add_attribute(Attribute::Bold),
        Cell::new("After").add_attribute(Attribute::Bold),
        Cell::new("Delta").add_attribute(Attribute::Bold),
    ]);
    for (label, before, after) in [
        ("Files", delta.files_before, delta.files_after),
        ("Nodes", delta.nodes_before, delta.nodes_after),
        ("Edges", delta.edges_before, delta.edges_after),
        ("Violations", delta.violations_before, delta.violations_after),
    ] {
        table.add_row(vec![
            Cell::new(label),
            Cell::new(before),
            Cell::new(after),
            Cell::new(signed_delta(before, after)),
        ]);
    }
    println!("{table}");

    fn print_list(title: &str, items: &[String]) {
        if items.is_empty() {
            return;
        }
        println!("\n{title} ({})", items.len());
        for item in items.iter().take(CAP) {
            println!("  • {item}");
        }
        if items.len() > CAP {
            println!("  … and {} more (use --format json for the full list)", items.len() - CAP);
        }
    }

    print_list("📄 Files added", &delta.files_added);
    print_list("📄 Files removed", &delta.files_removed);
    print_list("🧩 Nodes added", &delta.nodes_added);
    print_list("🧩 Nodes removed", &delta.nodes_removed);

    if !delta.edges_added.is_empty() {
        println!("\n🔗 Edges added ({})", delta.edges_added.len());
        for edge in delta.edges_added.iter().take(CAP) {
            println!("  • {} → {} ({})", edge.from, edge.to, edge.kind);
        }
    }
    if !delta.edges_removed.is_empty() {
        println!("\n🔗 Edges removed ({})", delta.edges_removed.len());
        for edge in delta.edges_removed.iter().take(CAP) {
            println!("  • {} → {} ({})", edge.from, edge.to, edge.kind);
        }
    }

    if !delta.violations_resolved.is_empty() {
        println!("\n✅ Violations resolved ({})", delta.violations_resolved.len());
        for v in &delta.violations_resolved {
            println!("  • [{}] {} → {}: {}", v.kind, v.from, v.to, v.message);
        }
    }
    if !delta.violations_introduced.is_empty() {
        println!("\n❌ Violations introduced ({})", delta.violations_introduced.len());
        for v in &delta.violations_introduced {
            println!("  • [{}] {} → {}: {}", v.kind, v.from, v.to, v.message);
        }
    }
    if delta.violations_resolved.is_empty() && delta.violations_introduced.is_empty() {
        println!("\n✅ No violation changes between snapshots.");
    }
}

#[allow(dead_code)]
fn display_custom_violations(violations: &[DriftViolation]) {
    display_drift_report(violations);
}

#[cfg(test)]
mod cli_regression_tests {
    use super::*;
    use clap::{CommandFactory, error::ErrorKind};

    /// Regression test for the clap short-flag collisions that made
    /// `serve`, `suggest`, and `init` panic (exit 101) in debug builds:
    /// ServeArgs.host claimed `-h` (clap's --help), and SuggestArgs.focus
    /// / InitArgs.force claimed `-f` (the global --format). Building the
    /// full command tree runs clap's own consistency assertions — under
    /// `cargo test` (a debug build) any reintroduced collision panics
    /// here before it can ship.
    #[test]
    fn cli_definition_has_no_flag_conflicts() {
        Cli::command().debug_assert();
    }

    /// Every subcommand must render `--help` (an Err with kind
    /// DisplayHelp) instead of panicking on its flag definitions.
    #[test]
    fn every_subcommand_help_renders() {
        for sub in [
            "scan", "check", "impact", "snapshots", "compare", "export", "watch", "suggest",
            "docs", "serve", "init", "validate",
        ] {
            let err = match Cli::try_parse_from(["ckb", sub, "--help"]) {
                Ok(_) => panic!("--help should short-circuit parsing"),
                Err(e) => e,
            };
            assert_eq!(
                err.kind(),
                ErrorKind::DisplayHelp,
                "subcommand {sub} --help did not render help"
            );
        }
    }

    /// Trivial invocations of the three formerly-panicking subcommands
    /// must parse cleanly, with the renamed shorts landing in the fields
    /// they belong to (long flags unchanged).
    #[test]
    fn formerly_colliding_subcommands_parse() {
        match Cli::try_parse_from(["ckb", "serve", "-H", "0.0.0.0", "--port", "3999", "--stdio"])
            .expect("serve parses")
            .command
        {
            Commands::Serve(args) => {
                assert_eq!(args.host, "0.0.0.0");
                assert_eq!(args.port, 3999);
                assert!(args.stdio);
            }
            _ => panic!("expected Serve"),
        }
        match Cli::try_parse_from(["ckb", "suggest", ".", "-F", "api", "--format", "json"])
            .expect("suggest parses")
            .command
        {
            Commands::Suggest(args) => assert_eq!(args.focus.as_deref(), Some("api")),
            _ => panic!("expected Suggest"),
        }
        // The global --format must still be available alongside subcommand flags.
        let cli = Cli::try_parse_from(["ckb", "init", ".", "-F"]).expect("init parses");
        match cli.command {
            Commands::Init(args) => assert!(args.force),
            _ => panic!("expected Init"),
        }
        let cli = Cli::try_parse_from(["ckb", "--format", "json", "serve", "--help"]);
        assert!(cli.is_err(), "--format + serve --help still renders help");
    }

    fn bare_report(snapshot_id: &str, duration_ms: f64) -> ScanReport {
        ScanReport {
            files_processed: 2,
            nodes: 3,
            edges: 2,
            patterns: Vec::new(),
            drift: Vec::new(),
            snapshot_id: snapshot_id.to_string(),
            duration_ms,
            package_identity: None,
            external_dependencies: Vec::new(),
        }
    }

    /// Watch mode must not "detect a change" when only the per-scan
    /// noise (fresh snapshot UUID, measured duration) differs.
    #[test]
    fn watch_fingerprint_ignores_per_scan_noise() {
        let a = bare_report("11111111-1111-1111-1111-111111111111", 12.5);
        let b = bare_report("22222222-2222-2222-2222-222222222222", 987.25);
        assert_eq!(report_fingerprint(&a), report_fingerprint(&b));
    }

    fn write_source(root: &std::path::Path, rel: &str, content: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ckb-cli-test-{tag}-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The a↔b cycle half of the compare/rules fixture pair. `cycle`
    /// toggles b.ts importing a.ts (the violation) vs. being
    /// self-contained (the fixed version).
    fn write_cycle_project(root: &std::path::Path, cycle: bool) {
        write_source(root, "src/core/util.ts",
            "export function formatId(prefix: string, n: number): string { return prefix + \"-\" + n; }\n");
        write_source(root, "src/core/a.ts",
            "import { stepB } from \"./b\";\nexport function stepA(): string { return stepB(); }\n");
        if cycle {
            write_source(root, "src/core/b.ts",
                "import { stepA } from \"./a\";\nexport function stepB(): string { return stepA(); }\n");
        } else {
            write_source(root, "src/core/b.ts",
                "export function stepB(): string { return \"b\"; }\n");
        }
        write_source(root, "src/db/store.ts",
            "import { formatId } from \"../core/util\";\nexport function readAll(): string { return formatId(\"rec\", 1); }\n");
        write_source(root, "src/ui/panel.ts",
            "import { readAll } from \"../db/store\";\nimport { formatId } from \"../core/util\";\nexport function renderPanel(): string { return formatId(\"panel\", readAll().length); }\n");
    }

    /// `ckb compare` used to be an empty stub: exit 0, zero bytes out,
    /// even for snapshot IDs that don't exist. This exercises the exact
    /// engine calls the fixed command makes (store → load → compare)
    /// over two real scans of two different roots — the root-relative
    /// identity normalization is what lets fixture-a vs fixture-b diff
    /// as one project instead of "everything changed".
    #[tokio::test]
    async fn compare_reports_the_real_delta_between_two_scans() {
        let root_a = temp_dir("cmp-a");
        let root_b = temp_dir("cmp-b");
        let storage = temp_dir("cmp-store");
        write_cycle_project(&root_a, true);
        write_cycle_project(&root_b, false);
        write_source(&root_b, "src/service/report.ts",
            "import { formatId } from \"../core/util\";\nexport function buildReport(): string { return formatId(\"report\", 1); }\n");

        let engine = CkbEngine::new_with_storage_path(&storage.to_string_lossy()).unwrap();
        let report_a = engine.scan_codebase(&root_a.to_string_lossy()).await.unwrap();
        // The engine accumulates into one in-memory graph across scans
        // (long-running hosts reset between projects); a fresh CLI
        // process per scan is the equivalent, so reset here.
        engine.reset_architecture_graph().await;
        let report_b = engine.scan_codebase(&root_b.to_string_lossy()).await.unwrap();
        assert_eq!(report_a.drift.len(), 1, "fixture A has exactly the cycle violation");

        let graph_a = engine.load_snapshot(&report_a.snapshot_id).await.unwrap()
            .expect("scan A must store a loadable snapshot");
        let graph_b = engine.load_snapshot(&report_b.snapshot_id).await.unwrap()
            .expect("scan B must store a loadable snapshot");
        assert!(engine.load_snapshot("00000000-0000-0000-0000-000000000000").await.unwrap().is_none(),
            "unknown snapshot IDs must come back as None so the CLI can fail loudly");

        let delta = engine.compare_snapshots(&graph_a, &graph_b).unwrap();
        assert_eq!((delta.files_before, delta.files_after), (5, 6));
        assert_eq!(delta.files_added, vec!["service/report.ts".to_string()]);
        assert!(delta.files_removed.is_empty());
        assert_eq!(delta.nodes_after - delta.nodes_before, 2, "new file node + its symbol node");
        assert!(delta.edges_removed.iter().any(|e| e.kind == "Import"
            && e.from == "core/b.ts::file" && e.to == "core/a.ts::file"),
            "the removed b→a import edge must be itemised: {:?}", delta.edges_removed);
        assert!(delta.edges_added.iter().any(|e| e.kind == "Import"
            && e.from == "service/report.ts::file" && e.to == "core/util.ts::file"),
            "the added report→util import edge must be itemised: {:?}", delta.edges_added);
        assert_eq!((delta.violations_before, delta.violations_after), (1, 0));
        assert_eq!(delta.violations_resolved.len(), 1);
        assert_eq!(delta.violations_resolved[0].kind, "CircularDependency");
        assert!(delta.violations_introduced.is_empty());
    }

    /// `.ckb/rules.json` used to be dead configuration: `ckb suggest`
    /// told users to add it, nothing loaded it, and scans silently
    /// enforced nothing. A valid file must now produce a
    /// ForbiddenDependency for the scoped import — and only that import
    /// (service→db stays legal, no file = no enforcement, malformed
    /// file = loud failure).
    #[tokio::test]
    async fn project_rules_are_enforced_by_scan() {
        let storage = temp_dir("rules-store");
        let engine = CkbEngine::new_with_storage_path(&storage.to_string_lossy()).unwrap();

        // No rules file: only the cycle is reported.
        let root_plain = temp_dir("rules-plain");
        write_cycle_project(&root_plain, true);
        let plain = engine.scan_codebase(&root_plain.to_string_lossy()).await.unwrap();
        assert_eq!(plain.drift.len(), 1);
        assert!(plain.drift.iter().all(|v| v.kind != ckb_core::ViolationKind::ForbiddenDependency));

        // With the rules file: the ui→db import is flagged, scoped by
        // `from` so nothing else changes.
        let root_ruled = temp_dir("rules-ruled");
        write_cycle_project(&root_ruled, true);
        write_source(&root_ruled, ".ckb/rules.json", r#"[
          {
            "name": "ui-must-not-import-db",
            "description": "UI must go through the service layer",
            "pattern": "src/db/",
            "from": "src/ui/",
            "to": "src/db/",
            "severity": "Error"
          }
        ]"#);
        engine.reset_architecture_graph().await;
        let ruled = engine.scan_codebase(&root_ruled.to_string_lossy()).await.unwrap();
        assert_eq!(ruled.drift.len(), 2, "cycle + the rules violation: {:?}", ruled.drift);
        let violation = ruled.drift.iter()
            .find(|v| v.kind == ckb_core::ViolationKind::ForbiddenDependency)
            .expect("ui→db import must be flagged");
        assert_eq!(violation.boundary, "ui-must-not-import-db");
        assert_eq!(violation.severity, Severity::Error);
        assert!(violation.from.0.ends_with("src/ui/panel.ts::file"), "from: {}", violation.from.0);
        assert!(violation.to.0.ends_with("src/db/store.ts::file"), "to: {}", violation.to.0);

        // A malformed rules file must fail the scan, not silently pass.
        let root_bad = temp_dir("rules-bad");
        write_cycle_project(&root_bad, true);
        write_source(&root_bad, ".ckb/rules.json", "{ not json");
        engine.reset_architecture_graph().await;
        assert!(engine.scan_codebase(&root_bad.to_string_lossy()).await.is_err(),
            "malformed .ckb/rules.json must fail loudly");
    }

    /// …but a real content change (a new violation) must change it.
    #[test]
    fn watch_fingerprint_detects_real_change() {
        let a = bare_report("same", 1.0);
        let mut b = bare_report("same", 1.0);
        b.drift.push(DriftViolation {
            id: uuid::Uuid::new_v4(),
            kind: ckb_core::ViolationKind::GodObject,
            from: ckb_core::NodeId("src/a.ts".to_string()),
            to: ckb_core::NodeId("src/b.ts".to_string()),
            boundary: "core".to_string(),
            message: "Module has too many dependencies".to_string(),
            severity: Severity::Warning,
            suggested_fix: None,
        });
        assert_ne!(report_fingerprint(&a), report_fingerprint(&b));
    }
}
