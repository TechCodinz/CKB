//! Git-history evidence for V13.1 Deep Software Causality.
//!
//! History is ingested only from the repository's own Git object database.
//! Every generated fact remains `HISTORY`; it is never upgraded to STATIC or
//! RUNTIME truth. The extractor is intentionally best-effort so non-Git source
//! trees can still build a valid causality bundle.

use super::deep_causality::*;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct GitHistoryIngestReport {
    pub git_repository_observed: bool,
    pub commits_ingested: usize,
    pub changed_file_observations: usize,
    pub historical_owners: usize,
}

#[derive(Debug, Clone)]
struct CommitObservation {
    sha: String,
    author_name: String,
    author_email: String,
    timestamp_ms: i64,
    changed_files: Vec<String>,
}

pub fn enrich_with_git_history(
    engine: &mut DeepCausalityEngine,
    root: impl AsRef<Path>,
    repository: &str,
    max_commits: usize,
) -> GitHistoryIngestReport {
    let root = root.as_ref();
    if max_commits == 0 {
        return GitHistoryIngestReport::default();
    }

    let inside = Command::new("git")
        .arg("-C").arg(root)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "true")
        .unwrap_or(false);
    if !inside {
        return GitHistoryIngestReport::default();
    }

    let format = "%x1e%H%x1f%an%x1f%ae%x1f%at";
    let output = match Command::new("git")
        .arg("-C").arg(root)
        .arg("log")
        .arg(format!("-n{}", max_commits))
        .arg(format!("--format={format}"))
        .arg("--numstat")
        .arg("--no-renames")
        .output()
    {
        Ok(v) if v.status.success() => v,
        _ => return GitHistoryIngestReport { git_repository_observed: true, ..Default::default() },
    };

    let text = String::from_utf8_lossy(&output.stdout);
    let observations: Vec<CommitObservation> = text
        .split('\x1e')
        .filter_map(parse_commit_record)
        .collect();

    let mut ownership_counts: HashMap<(String, String), usize> = HashMap::new();
    let mut changed_file_observations = 0usize;
    let mut prior_commit: Option<String> = None;

    for observation in &observations {
        let commit_id = format!("repo:{repository}::commit:{}", observation.sha);
        let owner_id = owner_id(&observation.author_email, &observation.author_name);
        let mut commit_attributes = BTreeMap::new();
        commit_attributes.insert("git.sha".into(), observation.sha.clone());
        commit_attributes.insert("git.author_name".into(), observation.author_name.clone());
        commit_attributes.insert("git.author_email".into(), observation.author_email.clone());
        engine.upsert_entity(CausalEntity {
            id: commit_id.clone(),
            kind: CausalEntityKind::Commit,
            name: observation.sha.chars().take(12).collect(),
            repository: Some(repository.to_string()),
            path: None,
            attributes: commit_attributes,
        });
        engine.upsert_entity(CausalEntity {
            id: owner_id.clone(),
            kind: CausalEntityKind::Owner,
            name: observation.author_name.clone(),
            repository: None,
            path: None,
            attributes: BTreeMap::from([("email".into(), observation.author_email.clone())]),
        });
        let _ = engine.add_fact(history_fact(
            &commit_id,
            &owner_id,
            CausalRelationKind::AuthoredBy,
            observation.timestamp_ms,
            BTreeMap::new(),
        ));

        if let Some(older_commit) = prior_commit.as_ref() {
            // git log is newest -> oldest. The previously ingested commit is
            // therefore newer than the current one and supersedes it.
            let _ = engine.add_fact(history_fact(
                older_commit,
                &commit_id,
                CausalRelationKind::Supersedes,
                observation.timestamp_ms,
                BTreeMap::new(),
            ));
        }
        prior_commit = Some(commit_id.clone());

        for path in &observation.changed_files {
            changed_file_observations += 1;
            let file_id = format!("repo:{repository}::file:{}", normalize_path(path));
            engine.upsert_entity(CausalEntity {
                id: file_id.clone(),
                kind: CausalEntityKind::File,
                name: path.rsplit('/').next().unwrap_or(path).to_string(),
                repository: Some(repository.to_string()),
                path: Some(normalize_path(path)),
                attributes: BTreeMap::new(),
            });
            let _ = engine.add_fact(history_fact(
                &commit_id,
                &file_id,
                CausalRelationKind::Changes,
                observation.timestamp_ms,
                BTreeMap::from([("git.sha".into(), observation.sha.clone())]),
            ));
            *ownership_counts.entry((owner_id.clone(), file_id)).or_default() += 1;
        }
    }

    for ((owner, file), count) in &ownership_counts {
        let confidence = (*count as f32 / 5.0).clamp(0.2, 1.0);
        let _ = engine.add_fact(CausalFact {
            from: owner.clone(),
            to: file.clone(),
            relation: CausalRelationKind::Owns,
            evidence: CausalEvidenceClass::History,
            confidence,
            condition: None,
            timestamp_ms: None,
            metadata: BTreeMap::from([
                ("basis".into(), "git_commit_contributions".into()),
                ("commit_count".into(), count.to_string()),
            ]),
        });
    }

    GitHistoryIngestReport {
        git_repository_observed: true,
        commits_ingested: observations.len(),
        changed_file_observations,
        historical_owners: ownership_counts.keys().map(|(owner, _)| owner).collect::<std::collections::HashSet<_>>().len(),
    }
}

fn parse_commit_record(record: &str) -> Option<CommitObservation> {
    let mut lines = record.lines();
    let header = lines.next()?.trim();
    if header.is_empty() { return None; }
    let mut parts = header.split('\x1f');
    let sha = parts.next()?.trim().to_string();
    let author_name = parts.next().unwrap_or("unknown").trim().to_string();
    let author_email = parts.next().unwrap_or("").trim().to_ascii_lowercase();
    let timestamp_ms = parts.next().and_then(|v| v.trim().parse::<i64>().ok()).unwrap_or(0).saturating_mul(1000);
    if sha.is_empty() { return None; }

    let changed_files = lines.filter_map(|line| {
        let mut columns = line.split('\t');
        let _added = columns.next()?;
        let _deleted = columns.next()?;
        let path = columns.next()?.trim();
        if path.is_empty() { None } else { Some(normalize_path(path)) }
    }).collect();

    Some(CommitObservation { sha, author_name, author_email, timestamp_ms, changed_files })
}

fn owner_id(email: &str, name: &str) -> String {
    let stable = if email.trim().is_empty() { name.trim().to_ascii_lowercase() } else { email.trim().to_ascii_lowercase() };
    format!("owner:git:{}", stable.replace([' ', '/', '\\', ':'], "_"))
}

fn normalize_path(path: &str) -> String { path.replace('\\', "/").trim_start_matches("./").to_string() }

fn history_fact(from: &str, to: &str, relation: CausalRelationKind, timestamp_ms: i64, metadata: BTreeMap<String, String>) -> CausalFact {
    CausalFact {
        from: from.into(),
        to: to.into(),
        relation,
        evidence: CausalEvidenceClass::History,
        confidence: 1.0,
        condition: None,
        timestamp_ms: if timestamp_ms > 0 { Some(timestamp_ms) } else { None },
        metadata,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_git_numstat_record() {
        let record = "abc123\x1fAda Dev\x1fada@example.com\x1f1700000000\n10\t2\tsrc/api.rs\n-\t-\tassets/logo.png\n";
        let parsed = parse_commit_record(record).unwrap();
        assert_eq!(parsed.sha, "abc123");
        assert_eq!(parsed.changed_files, vec!["src/api.rs", "assets/logo.png"]);
        assert_eq!(parsed.timestamp_ms, 1_700_000_000_000);
    }

    #[test]
    fn owner_ids_are_stable() {
        assert_eq!(owner_id("ADA@EXAMPLE.COM", "Ada"), "owner:git:ada@example.com");
    }
}
