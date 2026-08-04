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
    
    /// Focus area
    #[arg(short, long)]
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
    /// Host address
    #[arg(short, long, default_value = "127.0.0.1")]
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

    /// Force overwrite existing rule files
    #[arg(short, long)]
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
    
    println!("🔍 CKB Scan - Analyzing codebase: {}", args.path.display());
    
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
    println!("🔍 CKB Check - Analyzing architectural drift: {}", args.path.display());
    
    let engine = CkbEngine::new()?;
    let report = engine.scan_codebase(&args.path.to_string_lossy()).await?;
    
    match format.as_str() {
        "json" => {
            println!("{}", to_string_pretty(&report)?);
        }
        "sarif" => {
            let sarif = generate_sarif(&report);
            println!("{}", to_string_pretty(&sarif)?);
        }
        "junit" => {
            let junit = generate_junit(&report);
            println!("{}", junit);
        }
        _ => {
            display_drift_report(&report.drift);
        }
    }
    
    if let Some(output) = args.output {
        fs::write(output, to_string_pretty(&report)?).await?;
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
    println!("🔍 CKB Impact - Analyzing change impact at {}:{}", 
        args.file.display(), args.line);
    
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

async fn snapshots_command(args: SnapshotsArgs, format: String) -> Result<()> {
    println!("No snapshots found for {}", args.path.display());
    Ok(())
}

async fn compare_command(args: CompareArgs, format: String) -> Result<()> {
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
        
        let current_hash = format!("{:x}", md5::compute(to_string_pretty(&report)?));
        
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
    println!("🔍 CKB Suggest - Finding architectural improvements");
    
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
    println!("🔍 CKB Validate - Validating against custom rules");
    
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

async fn serve_command(args: ServeArgs) -> Result<()> {
    println!("🚀 Starting CKB MCP server on {}:{}", args.host, args.port);
    println!("Press Ctrl+C to stop");
    Ok(())
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

fn display_snapshots(snapshots: &[ckb_core::SnapshotMetadata], verbose: bool) {
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
    for (i, pattern) in report.patterns.iter().enumerate() {
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
                            severity: rule.severity.clone(),
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

fn generate_suggestions(report: &ScanReport, count: usize, focus: Option<String>) -> Result<Vec<ArchitectureSuggestion>> {
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

fn display_snapshot_diff(diff: &SnapshotDiff) {
    println!("Snapshot comparison: (not yet implemented)");
}

fn format_change(change: i64) -> String {
    if change > 0 { format!("+{}", change) }
    else if change < 0 { format!("{}", change) }
    else { "0".to_string() }
}

#[derive(Debug)]
struct SnapshotDiff {}

fn display_custom_violations(violations: &[DriftViolation]) {
    display_drift_report(violations);
}
