//! Evidence-backed Git patch transactions.
//!
//! A transaction never edits the caller's current checkout. It resolves an
//! exact baseline commit, creates a dedicated Git worktree and branch, checks
//! and stages the supplied unified diff, then runs validation commands without
//! invoking a shell. Committing the isolated branch is a separate operation so
//! a Cloud/API layer can require explicit human confirmation first.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use uuid::Uuid;

const MAX_PATCH_BYTES: usize = 4 * 1024 * 1024;
const MAX_CAPTURE_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PatchTransactionState {
    Validated,
    ValidationFailed,
    Committed,
    RolledBack,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ValidationCommand {
    /// Executable name or path. It is executed directly; no shell is used.
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub label: String,
}

impl ValidationCommand {
    pub fn new(label: impl Into<String>, program: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            label: label.into(),
            program: program.into(),
            args,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ValidationResult {
    pub label: String,
    pub program: String,
    pub args: Vec<String>,
    pub success: bool,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchTransaction {
    pub transaction_id: String,
    pub repository_path: String,
    pub baseline_commit: String,
    pub branch_name: String,
    pub worktree_path: String,
    pub patch_object_id: String,
    pub staged_tree_id: String,
    pub changed_files: Vec<String>,
    pub validations: Vec<ValidationResult>,
    pub state: PatchTransactionState,
    pub committed_sha: Option<String>,
    #[serde(default)]
    pub rollback_staged_tree_id: Option<String>,
    #[serde(default)]
    pub rollback_validations: Vec<ValidationResult>,
    #[serde(default)]
    pub rollback_committed_sha: Option<String>,
    pub mutation_scope: String,
    pub synthetic: bool,
}

pub struct PatchTransactionEngine;

impl PatchTransactionEngine {
    /// Prepare and validate a patch on an isolated branch/worktree.
    ///
    /// The returned branch is intentionally left uncommitted. Call `commit`
    /// only after the product layer has recorded explicit user confirmation.
    pub fn prepare(
        repository_path: impl AsRef<Path>,
        baseline_ref: &str,
        patch: &str,
        validation_commands: &[ValidationCommand],
    ) -> anyhow::Result<PatchTransaction> {
        if validation_commands.is_empty() || validation_commands.len() > 16 {
            return Err(anyhow::anyhow!(
                "patch transactions require 1 to 16 validation commands"
            ));
        }
        let repository = repository_path
            .as_ref()
            .canonicalize()
            .map_err(|error| anyhow::anyhow!("repository path is unavailable: {error}"))?;
        Self::validate_repository(&repository)?;
        Self::validate_ref(baseline_ref)?;
        let declared_paths = Self::validate_patch(patch)?;

        let baseline_commit = Self::git_text(
            &repository,
            &[
                "rev-parse",
                "--verify",
                &format!("{baseline_ref}^{{commit}}"),
            ],
        )?;
        let transaction_id = Uuid::new_v4().simple().to_string();
        let branch_name = format!("ckb/transaction/{}", &transaction_id[..16]);
        let worktree = std::env::temp_dir()
            .join("ckb-patch-transactions")
            .join(&transaction_id);
        let parent = worktree
            .parent()
            .ok_or_else(|| anyhow::anyhow!("invalid worktree path"))?;
        std::fs::create_dir_all(parent)?;

        let worktree_text = worktree.to_string_lossy().into_owned();
        let add_result = Self::git_output(
            &repository,
            &[
                "worktree",
                "add",
                "--detach",
                &worktree_text,
                &baseline_commit,
            ],
        )?;
        if !add_result.status.success() {
            return Err(Self::command_error("git worktree add", &add_result));
        }

        let prepared = (|| -> anyhow::Result<PatchTransaction> {
            let branch_result = Self::git_output(&worktree, &["switch", "-c", &branch_name])?;
            if !branch_result.status.success() {
                return Err(Self::command_error("git switch -c", &branch_result));
            }

            let check = Self::git_with_input(
                &worktree,
                &["apply", "--check", "--whitespace=error-all", "-"],
                patch.as_bytes(),
            )?;
            if !check.status.success() {
                return Err(Self::command_error("git apply --check", &check));
            }

            let apply = Self::git_with_input(
                &worktree,
                &["apply", "--index", "--whitespace=fix", "-"],
                patch.as_bytes(),
            )?;
            if !apply.status.success() {
                return Err(Self::command_error("git apply --index", &apply));
            }

            let changed_files = Self::git_text(
                &worktree,
                &["diff", "--cached", "--name-only", "--diff-filter=ACMRD"],
            )?
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
            if changed_files.is_empty() {
                return Err(anyhow::anyhow!("patch produced no staged source changes"));
            }

            let changed_set = changed_files.iter().cloned().collect::<BTreeSet<_>>();
            if !declared_paths.is_subset(&changed_set) {
                return Err(anyhow::anyhow!(
                    "staged change set does not contain every path declared by the patch"
                ));
            }

            let patch_object_id = Self::git_hash_object(&worktree, patch.as_bytes())?;
            let staged_tree_id = Self::git_text(&worktree, &["write-tree"])?;
            let validations = validation_commands
                .iter()
                .map(|command| Self::run_validation(&worktree, command))
                .collect::<anyhow::Result<Vec<_>>>()?;
            let all_passed = validations.iter().all(|result| result.success);

            Ok(PatchTransaction {
                transaction_id,
                repository_path: repository.to_string_lossy().into_owned(),
                baseline_commit,
                branch_name: branch_name.clone(),
                worktree_path: worktree_text,
                patch_object_id,
                staged_tree_id,
                changed_files,
                validations,
                state: if all_passed {
                    PatchTransactionState::Validated
                } else {
                    PatchTransactionState::ValidationFailed
                },
                committed_sha: None,
                rollback_staged_tree_id: None,
                rollback_validations: Vec::new(),
                rollback_committed_sha: None,
                mutation_scope: "isolated-git-worktree-and-branch".to_string(),
                synthetic: false,
            })
        })();

        if prepared.is_err() {
            let _ = Self::remove_worktree(&repository, &worktree);
            let _ = Self::git_output(&repository, &["branch", "-D", &branch_name]);
        }
        prepared
    }

    /// Commit a previously validated isolated transaction.
    ///
    /// This does not merge, push, or modify the user's original checkout.
    pub fn commit(transaction: &mut PatchTransaction, message: &str) -> anyhow::Result<String> {
        if transaction.state != PatchTransactionState::Validated {
            return Err(anyhow::anyhow!(
                "only a fully validated transaction can be committed"
            ));
        }
        let message = message.trim();
        if message.is_empty() || message.len() > 500 {
            return Err(anyhow::anyhow!(
                "commit message must contain 1 to 500 characters"
            ));
        }

        let worktree = Path::new(&transaction.worktree_path);
        let current_tree = Self::git_text(worktree, &["write-tree"])?;
        if current_tree != transaction.staged_tree_id {
            return Err(anyhow::anyhow!(
                "staged tree changed after validation; prepare a new transaction"
            ));
        }

        let output = Self::git_output(
            worktree,
            &[
                "-c",
                "user.name=CKB Patch Transaction",
                "-c",
                "user.email=patch-transaction@ckb.invalid",
                "commit",
                "--no-gpg-sign",
                "-m",
                message,
            ],
        )?;
        if !output.status.success() {
            return Err(Self::command_error("git commit", &output));
        }
        let sha = Self::git_text(worktree, &["rev-parse", "HEAD"])?;
        transaction.state = PatchTransactionState::Committed;
        transaction.committed_sha = Some(sha.clone());
        Ok(sha)
    }

    /// Revert a committed transaction inside its isolated worktree, validate
    /// the reverted tree, and only then create the rollback commit.
    ///
    /// The caller's active checkout, remote refs and production branches are
    /// never modified. A failed rollback validation is discarded in the
    /// isolated worktree and no rollback commit is created.
    pub fn rollback(transaction: &mut PatchTransaction) -> anyhow::Result<String> {
        if transaction.state != PatchTransactionState::Committed {
            return Err(anyhow::anyhow!(
                "only a committed isolated transaction can be rolled back"
            ));
        }
        let committed_sha = transaction
            .committed_sha
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("committed transaction is missing its commit sha"))?;
        let worktree = Path::new(&transaction.worktree_path);
        let head = Self::git_text(worktree, &["rev-parse", "HEAD"])?;
        if head != committed_sha {
            return Err(anyhow::anyhow!(
                "isolated branch head changed after commit; rollback refused"
            ));
        }

        let revert = Self::git_output(worktree, &["revert", "--no-commit", committed_sha])?;
        if !revert.status.success() {
            let _ = Self::git_output(worktree, &["revert", "--abort"]);
            return Err(Self::command_error("git revert --no-commit", &revert));
        }

        let rollback = (|| -> anyhow::Result<(String, Vec<ValidationResult>)> {
            let staged_tree_id = Self::git_text(worktree, &["write-tree"])?;
            let validations = transaction
                .validations
                .iter()
                .map(|previous| ValidationCommand {
                    label: format!("rollback: {}", previous.label),
                    program: previous.program.clone(),
                    args: previous.args.clone(),
                })
                .map(|command| Self::run_validation(worktree, &command))
                .collect::<anyhow::Result<Vec<_>>>()?;
            if validations.iter().any(|result| !result.success) {
                return Err(anyhow::anyhow!(
                    "rollback validation failed; no rollback commit was created"
                ));
            }

            let message = format!("revert: CKB transaction {}", transaction.transaction_id);
            let commit = Self::git_output(
                worktree,
                &[
                    "-c",
                    "user.name=CKB Patch Transaction",
                    "-c",
                    "user.email=patch-transaction@ckb.invalid",
                    "commit",
                    "--no-gpg-sign",
                    "-m",
                    &message,
                ],
            )?;
            if !commit.status.success() {
                return Err(Self::command_error("git commit rollback", &commit));
            }
            Ok((staged_tree_id, validations))
        })();

        let (staged_tree_id, validations) = match rollback {
            Ok(value) => value,
            Err(error) => {
                let _ = Self::git_output(worktree, &["reset", "--hard", "HEAD"]);
                return Err(error);
            }
        };
        let sha = Self::git_text(worktree, &["rev-parse", "HEAD"])?;
        transaction.rollback_staged_tree_id = Some(staged_tree_id);
        transaction.rollback_validations = validations;
        transaction.rollback_committed_sha = Some(sha.clone());
        transaction.state = PatchTransactionState::RolledBack;
        Ok(sha)
    }

    /// Re-run the original validation commands against the current isolated
    /// worktree to produce post-commit or post-rollback evidence.
    pub fn revalidate(transaction: &PatchTransaction) -> anyhow::Result<Vec<ValidationResult>> {
        if !matches!(
            transaction.state,
            PatchTransactionState::Committed | PatchTransactionState::RolledBack
        ) {
            return Err(anyhow::anyhow!(
                "only committed or rolled-back transactions can be rescanned"
            ));
        }
        let worktree = Path::new(&transaction.worktree_path);
        transaction
            .validations
            .iter()
            .map(|previous| ValidationCommand {
                label: format!("post-change: {}", previous.label),
                program: previous.program.clone(),
                args: previous.args.clone(),
            })
            .map(|command| Self::run_validation(worktree, &command))
            .collect()
    }

    /// Remove the isolated worktree. The branch is kept by default so a
    /// confirmed commit remains inspectable and can be pushed or merged.
    pub fn cleanup(transaction: &PatchTransaction, delete_branch: bool) -> anyhow::Result<()> {
        let repository = Path::new(&transaction.repository_path);
        let worktree = Path::new(&transaction.worktree_path);
        Self::remove_worktree(repository, worktree)?;
        if delete_branch {
            let output = Self::git_output(repository, &["branch", "-D", &transaction.branch_name])?;
            if !output.status.success() {
                return Err(Self::command_error("git branch -D", &output));
            }
        }
        Ok(())
    }

    fn validate_repository(repository: &Path) -> anyhow::Result<()> {
        let output = Self::git_output(repository, &["rev-parse", "--is-inside-work-tree"])?;
        if !output.status.success() || String::from_utf8_lossy(&output.stdout).trim() != "true" {
            return Err(anyhow::anyhow!(
                "patch transactions require a Git working tree"
            ));
        }
        Ok(())
    }

    fn validate_ref(reference: &str) -> anyhow::Result<()> {
        if reference.is_empty()
            || reference.len() > 300
            || reference.starts_with('-')
            || reference.contains('\0')
            || reference.chars().any(char::is_whitespace)
        {
            return Err(anyhow::anyhow!("invalid baseline Git reference"));
        }
        Ok(())
    }

    fn validate_patch(patch: &str) -> anyhow::Result<BTreeSet<String>> {
        if patch.is_empty() || patch.len() > MAX_PATCH_BYTES {
            return Err(anyhow::anyhow!(
                "patch must contain 1 to {MAX_PATCH_BYTES} bytes"
            ));
        }
        if patch.contains("GIT binary patch") || patch.as_bytes().contains(&0) {
            return Err(anyhow::anyhow!(
                "binary patches are not accepted by source transactions"
            ));
        }

        let mut paths = BTreeSet::new();
        for line in patch.lines() {
            let Some(raw) = line.strip_prefix("+++ ") else {
                continue;
            };
            let raw = raw.split('\t').next().unwrap_or(raw).trim();
            if raw == "/dev/null" {
                continue;
            }
            if raw.starts_with('"') {
                return Err(anyhow::anyhow!("quoted patch paths are not supported"));
            }
            let path = raw.strip_prefix("b/").unwrap_or(raw).replace('\\', "/");
            let candidate = Path::new(&path);
            if candidate.is_absolute()
                || candidate
                    .components()
                    .any(|part| matches!(part, std::path::Component::ParentDir))
                || path == ".git"
                || path.starts_with(".git/")
                || path.is_empty()
            {
                return Err(anyhow::anyhow!("unsafe patch path: {path}"));
            }
            paths.insert(path);
        }
        if paths.is_empty() {
            return Err(anyhow::anyhow!("patch does not declare a source target"));
        }
        Ok(paths)
    }

    fn run_validation(
        worktree: &Path,
        validation: &ValidationCommand,
    ) -> anyhow::Result<ValidationResult> {
        if validation.program.trim().is_empty() || validation.label.trim().is_empty() {
            return Err(anyhow::anyhow!(
                "validation label and executable are required"
            ));
        }
        let output = Command::new(&validation.program)
            .args(&validation.args)
            .current_dir(worktree)
            .stdin(Stdio::null())
            .output()
            .map_err(|error| {
                anyhow::anyhow!(
                    "failed to execute validation '{}': {error}",
                    validation.label
                )
            })?;
        Ok(ValidationResult {
            label: validation.label.clone(),
            program: validation.program.clone(),
            args: validation.args.clone(),
            success: output.status.success(),
            exit_code: output.status.code(),
            stdout: Self::bounded_text(&output.stdout),
            stderr: Self::bounded_text(&output.stderr),
        })
    }

    fn git_text(cwd: &Path, args: &[&str]) -> anyhow::Result<String> {
        let output = Self::git_output(cwd, args)?;
        if !output.status.success() {
            return Err(Self::command_error(
                &format!("git {}", args.join(" ")),
                &output,
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    fn git_output(cwd: &Path, args: &[&str]) -> anyhow::Result<Output> {
        Command::new("git")
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .output()
            .map_err(|error| anyhow::anyhow!("failed to execute git: {error}"))
    }

    fn git_with_input(cwd: &Path, args: &[&str], input: &[u8]) -> anyhow::Result<Output> {
        let mut child = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| anyhow::anyhow!("failed to execute git: {error}"))?;
        child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("git stdin unavailable"))?
            .write_all(input)?;
        Ok(child.wait_with_output()?)
    }

    fn git_hash_object(cwd: &Path, input: &[u8]) -> anyhow::Result<String> {
        let output = Self::git_with_input(cwd, &["hash-object", "--stdin"], input)?;
        if !output.status.success() {
            return Err(Self::command_error("git hash-object", &output));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    fn remove_worktree(repository: &Path, worktree: &Path) -> anyhow::Result<()> {
        if !worktree.exists() {
            return Ok(());
        }
        let worktree_text = worktree.to_string_lossy().into_owned();
        let output = Self::git_output(
            repository,
            &["worktree", "remove", "--force", &worktree_text],
        )?;
        if !output.status.success() {
            return Err(Self::command_error("git worktree remove", &output));
        }
        Ok(())
    }

    fn bounded_text(bytes: &[u8]) -> String {
        let start = bytes.len().saturating_sub(MAX_CAPTURE_BYTES);
        String::from_utf8_lossy(&bytes[start..]).into_owned()
    }

    fn command_error(command: &str, output: &Output) -> anyhow::Error {
        anyhow::anyhow!(
            "{command} failed (exit {:?}): {}",
            output.status.code(),
            Self::bounded_text(&output.stderr).trim()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(cwd: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn repository() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        git(root.path(), &["init", "-q"]);
        std::fs::write(root.path().join("main.rs"), "fn value() -> i32 { 1 }\n").unwrap();
        git(root.path(), &["add", "main.rs"]);
        git(
            root.path(),
            &[
                "-c",
                "user.name=CKB Test",
                "-c",
                "user.email=test@ckb.invalid",
                "commit",
                "-q",
                "-m",
                "baseline",
            ],
        );
        root
    }

    #[test]
    fn prepares_validates_and_commits_only_the_isolated_branch() {
        let repository = repository();
        let patch = "diff --git a/main.rs b/main.rs\nindex 4d2a9f4..8c2a01e 100644\n--- a/main.rs\n+++ b/main.rs\n@@ -1 +1 @@\n-fn value() -> i32 { 1 }\n+fn value() -> i32 { 2 }\n";
        let validation = ValidationCommand::new(
            "content-check",
            "git",
            vec!["diff".into(), "--cached".into(), "--check".into()],
        );
        let mut transaction =
            PatchTransactionEngine::prepare(repository.path(), "HEAD", patch, &[validation])
                .unwrap();

        assert_eq!(transaction.state, PatchTransactionState::Validated);
        assert_eq!(transaction.changed_files, vec!["main.rs"]);
        assert_eq!(
            std::fs::read_to_string(repository.path().join("main.rs")).unwrap(),
            "fn value() -> i32 { 1 }\n",
        );
        assert_eq!(
            std::fs::read_to_string(Path::new(&transaction.worktree_path).join("main.rs")).unwrap(),
            "fn value() -> i32 { 2 }\n",
        );

        let committed =
            PatchTransactionEngine::commit(&mut transaction, "fix: update value").unwrap();
        assert_eq!(transaction.state, PatchTransactionState::Committed);
        assert_eq!(
            transaction.committed_sha.as_deref(),
            Some(committed.as_str())
        );
        PatchTransactionEngine::cleanup(&transaction, true).unwrap();
    }

    #[test]
    fn rollback_is_validated_and_confined_to_the_isolated_branch() {
        let repository = repository();
        let patch = "diff --git a/main.rs b/main.rs\n--- a/main.rs\n+++ b/main.rs\n@@ -1 +1 @@\n-fn value() -> i32 { 1 }\n+fn value() -> i32 { 9 }\n";
        let validation = ValidationCommand::new(
            "repository-check",
            "git",
            vec!["diff".into(), "--cached".into(), "--check".into()],
        );
        let mut transaction =
            PatchTransactionEngine::prepare(repository.path(), "HEAD", patch, &[validation])
                .unwrap();
        PatchTransactionEngine::commit(&mut transaction, "feat: isolated value").unwrap();
        let rollback = PatchTransactionEngine::rollback(&mut transaction).unwrap();

        assert_eq!(transaction.state, PatchTransactionState::RolledBack);
        assert_eq!(
            transaction.rollback_committed_sha.as_deref(),
            Some(rollback.as_str())
        );
        assert!(transaction
            .rollback_validations
            .iter()
            .all(|item| item.success));
        assert!(PatchTransactionEngine::revalidate(&transaction)
            .unwrap()
            .iter()
            .all(|item| item.success));
        assert_eq!(
            std::fs::read_to_string(repository.path().join("main.rs")).unwrap(),
            "fn value() -> i32 { 1 }\n",
        );
        assert_eq!(
            std::fs::read_to_string(Path::new(&transaction.worktree_path).join("main.rs")).unwrap(),
            "fn value() -> i32 { 1 }\n",
        );
        PatchTransactionEngine::cleanup(&transaction, true).unwrap();
    }

    #[test]
    fn failed_validation_cannot_be_committed() {
        let repository = repository();
        let patch = "diff --git a/main.rs b/main.rs\n--- a/main.rs\n+++ b/main.rs\n@@ -1 +1 @@\n-fn value() -> i32 { 1 }\n+fn value() -> i32 { 3 }\n";
        let validation = ValidationCommand::new(
            "intentional-failure",
            "git",
            vec!["rev-parse".into(), "--verify".into(), "missing-ref".into()],
        );
        let mut transaction =
            PatchTransactionEngine::prepare(repository.path(), "HEAD", patch, &[validation])
                .unwrap();
        assert_eq!(transaction.state, PatchTransactionState::ValidationFailed);
        assert!(PatchTransactionEngine::commit(&mut transaction, "must not commit").is_err());
        PatchTransactionEngine::cleanup(&transaction, true).unwrap();
    }

    #[test]
    fn rejects_parent_directory_patch_paths() {
        let repository = repository();
        let patch = "diff --git a/../secret b/../secret\n--- a/../secret\n+++ b/../secret\n@@ -1 +1 @@\n-a\n+b\n";
        let validation = ValidationCommand::new(
            "unreachable-validation",
            "git",
            vec!["diff".into(), "--check".into()],
        );
        let error =
            PatchTransactionEngine::prepare(repository.path(), "HEAD", patch, &[validation])
                .unwrap_err();
        assert!(error.to_string().contains("unsafe patch path"));
    }
}
