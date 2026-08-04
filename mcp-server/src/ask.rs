//! Natural-language codebase Q&A.
//!
//! **Scope note, read before assuming this is more than it is:** this is a
//! keyword-overlap retrieval MVP, not real embeddings-based semantic search.
//! It scores graph nodes and violations by literal token overlap with the
//! question, takes the top matches, and asks Claude to answer using only
//! that context. That's good enough for "which service handles X" or "why
//! does Y depend on Z" style questions where the relevant names roughly
//! match the question's wording, but it will miss real semantic matches
//! that don't share vocabulary (e.g. asking about "auth" when the code says
//! "login" everywhere and never the word "auth"). A real version needs an
//! embeddings pipeline + vector store — this is intentionally not that, so
//! it's honest about being a first pass rather than pretending to be full
//! semantic search.

use ckb_core::{DriftViolation, Node};

const MAX_CONTEXT_NODES: usize = 25;
const MAX_CONTEXT_VIOLATIONS: usize = 10;

const STOPWORDS: &[&str] = &[
    "the", "a", "an", "is", "are", "was", "were", "does", "do", "did", "why", "what", "which",
    "how", "this", "that", "these", "those", "in", "on", "at", "to", "for", "of", "and", "or",
    "it", "its", "with", "from", "by", "as", "be", "has", "have", "had",
];

fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|t| t.len() > 2 && !STOPWORDS.contains(t))
        .map(|t| t.to_string())
        .collect()
}

fn score_overlap(haystack: &str, tokens: &[String]) -> usize {
    let haystack_lower = haystack.to_lowercase();
    tokens.iter().filter(|t| haystack_lower.contains(t.as_str())).count()
}

fn build_context(question: &str, nodes: &[Node], violations: &[DriftViolation]) -> String {
    let tokens = tokenize(question);

    let mut scored_nodes: Vec<(usize, &Node)> = nodes.iter()
        .map(|n| {
            let haystack = format!("{} {}", n.name, n.path.to_string_lossy());
            (score_overlap(&haystack, &tokens), n)
        })
        .filter(|(score, _)| *score > 0)
        .collect();
    scored_nodes.sort_by(|a, b| b.0.cmp(&a.0));
    scored_nodes.truncate(MAX_CONTEXT_NODES);

    let mut scored_violations: Vec<(usize, &DriftViolation)> = violations.iter()
        .map(|v| {
            let haystack = format!("{} {} {} {}", v.from.0, v.to.0, v.boundary, v.message);
            (score_overlap(&haystack, &tokens), v)
        })
        .filter(|(score, _)| *score > 0)
        .collect();
    scored_violations.sort_by(|a, b| b.0.cmp(&a.0));
    scored_violations.truncate(MAX_CONTEXT_VIOLATIONS);

    let mut context = String::new();

    if scored_nodes.is_empty() && scored_violations.is_empty() {
        context.push_str("No nodes or violations in the current scan matched keywords from the question. \
            Answer only if you can do so from general reasoning about the stats below; otherwise say the scan doesn't contain enough relevant information.\n\n");
    }

    context.push_str(&format!("Scan summary: {} nodes total, {} violations total.\n\n", nodes.len(), violations.len()));

    if !scored_nodes.is_empty() {
        context.push_str("Relevant code entities found in the scan:\n");
        for (_, n) in &scored_nodes {
            context.push_str(&format!("- {:?} `{}` at {}:{}\n", n.kind, n.name, n.path.display(), n.line));
        }
        context.push('\n');
    }

    if !scored_violations.is_empty() {
        context.push_str("Relevant architectural violations found in the scan:\n");
        for (_, v) in &scored_violations {
            context.push_str(&format!("- [{:?}/{:?}] {} -> {} ({}): {}\n", v.severity, v.kind, v.from.0, v.to.0, v.boundary, v.message));
        }
        context.push('\n');
    }

    context
}

pub async fn ask_about_codebase(
    question: &str,
    nodes: &[Node],
    violations: &[DriftViolation],
    api_key: &str,
) -> Result<String, String> {
    let context = build_context(question, nodes, violations);

    let system = "You are answering questions about a codebase using only the structural information \
        provided below, which comes from a real static-analysis scan (not from your training data about \
        this specific codebase, since you don't have any). If the provided context doesn't contain enough \
        information to answer confidently, say so plainly rather than guessing or inventing file/function \
        names that weren't given to you. Keep answers to a few sentences unless the question needs a list.";

    let user_content = format!("{}\n\nQuestion: {}", context, question);

    let (answer, _model) = super::explain::call_claude(system, user_content, 700, api_key).await?;
    Ok(answer.trim().to_string())
}
