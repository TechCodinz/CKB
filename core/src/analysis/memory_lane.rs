//! V13.2 Memory Lane — bounded, project-adaptive software intelligence.
//!
//! Memory Lane learns from observed project outcomes without silently rewriting
//! CKB core source. It may autonomously adapt project-local strategy rankings,
//! retrieval priorities and risk thresholds. Any proposal to change CKB core is
//! emitted as a guarded improvement proposal that remains PREDICTED until it is
//! validated by tests/compilers and explicitly promoted.

use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub const MEMORY_LANE_VERSION: &str = "ckb-memory-lane-v13.2";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryLaneKind {
    Episodic,
    Semantic,
    Procedural,
    Runtime,
    Preference,
    Reflection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryLaneEvidence {
    Static,
    Runtime,
    History,
    Validation,
    Human,
    Predicted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryLaneEpisode {
    pub id: String,
    pub project_id: String,
    pub kind: MemoryLaneKind,
    pub title: String,
    pub summary: String,
    #[serde(default)]
    pub entities: Vec<String>,
    #[serde(default)]
    pub strategy: Option<String>,
    #[serde(default)]
    pub predicted_score: Option<f64>,
    #[serde(default)]
    pub observed_score: Option<f64>,
    pub evidence: MemoryLaneEvidence,
    pub confidence: f64,
    pub created_at_ms: i64,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StrategyMemory {
    pub strategy: String,
    pub attempts: u64,
    pub successes: u64,
    pub failures: u64,
    pub total_reward: f64,
    pub mean_reward: f64,
    pub confidence: f64,
    pub last_used_at_ms: Option<i64>,
}

impl StrategyMemory {
    fn observe(&mut self, reward: f64, accepted: bool, now_ms: i64) {
        self.attempts = self.attempts.saturating_add(1);
        if accepted { self.successes = self.successes.saturating_add(1); }
        else { self.failures = self.failures.saturating_add(1); }
        self.total_reward += reward.clamp(-1.0, 1.0);
        self.mean_reward = if self.attempts == 0 { 0.0 } else { self.total_reward / self.attempts as f64 };
        self.confidence = (1.0 - (-((self.attempts as f64) / 8.0)).exp()).clamp(0.0, 1.0);
        self.last_used_at_ms = Some(now_ms);
    }

    fn ranking_score(&self) -> f64 {
        let success_rate = if self.attempts == 0 { 0.5 } else { self.successes as f64 / self.attempts as f64 };
        (self.mean_reward * 0.55 + success_rate * 0.35 + self.confidence * 0.10).clamp(-1.0, 1.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdaptiveProjectProfile {
    pub project_id: String,
    pub generation: u64,
    pub risk_threshold: f64,
    pub retrieval_depth_bias: i32,
    pub runtime_evidence_weight: f64,
    pub validation_evidence_weight: f64,
    pub history_evidence_weight: f64,
    pub strategies: BTreeMap<String, StrategyMemory>,
    #[serde(default)]
    pub stable_invariants: BTreeSet<String>,
    #[serde(default)]
    pub rejected_patterns: BTreeSet<String>,
    #[serde(default)]
    pub last_consolidated_at_ms: Option<i64>,
}

impl AdaptiveProjectProfile {
    pub fn new(project_id: impl Into<String>) -> Self {
        Self {
            project_id: project_id.into(), generation: 0,
            risk_threshold: 0.65, retrieval_depth_bias: 0,
            runtime_evidence_weight: 1.0, validation_evidence_weight: 1.0,
            history_evidence_weight: 0.75, strategies: BTreeMap::new(),
            stable_invariants: BTreeSet::new(), rejected_patterns: BTreeSet::new(),
            last_consolidated_at_ms: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningOutcome {
    pub project_id: String,
    pub episode_id: String,
    pub strategy: String,
    pub accepted: bool,
    pub validation_passed: Option<bool>,
    pub runtime_improved: Option<bool>,
    pub rollback_required: bool,
    pub human_approved: Option<bool>,
    pub reward: f64,
    pub observed_at_ms: i64,
    #[serde(default)]
    pub invariant_confirmed: Option<String>,
    #[serde(default)]
    pub rejected_pattern: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImprovementProposal {
    pub id: String,
    pub project_id: String,
    pub target: String,
    pub rationale: String,
    pub evidence_episode_ids: Vec<String>,
    pub expected_reward: f64,
    pub evidence: MemoryLaneEvidence,
    pub requires_guarded_change: bool,
    pub requires_compiler_and_tests: bool,
    pub auto_apply_allowed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryLaneReflection {
    pub project_id: String,
    pub episodes_considered: usize,
    pub calibrated_prediction_error: Option<f64>,
    pub top_strategies: Vec<StrategyMemory>,
    pub weak_strategies: Vec<StrategyMemory>,
    pub proposals: Vec<ImprovementProposal>,
    pub profile_generation: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryLaneEngine {
    pub version: String,
    pub profile: AdaptiveProjectProfile,
    episodes: VecDeque<MemoryLaneEpisode>,
    max_episodes: usize,
}

impl MemoryLaneEngine {
    pub fn new(project_id: impl Into<String>) -> Self {
        Self { version: MEMORY_LANE_VERSION.into(), profile: AdaptiveProjectProfile::new(project_id), episodes: VecDeque::new(), max_episodes: 2_000 }
    }

    pub fn with_capacity(project_id: impl Into<String>, max_episodes: usize) -> Self {
        let mut engine = Self::new(project_id);
        engine.max_episodes = max_episodes.clamp(100, 20_000);
        engine
    }

    pub fn episodes(&self) -> impl Iterator<Item = &MemoryLaneEpisode> { self.episodes.iter() }

    pub fn remember(&mut self, mut episode: MemoryLaneEpisode) -> Result<(), String> {
        if episode.project_id != self.profile.project_id { return Err("memory episode belongs to a different project".into()); }
        if !(0.0..=1.0).contains(&episode.confidence) { return Err("memory confidence must be within 0..=1".into()); }
        if matches!(episode.evidence, MemoryLaneEvidence::Predicted) && episode.observed_score.is_some() {
            return Err("predicted memory cannot contain an observed outcome".into());
        }
        if episode.id.trim().is_empty() { return Err("memory episode id is required".into()); }
        if self.episodes.iter().any(|existing| existing.id == episode.id) { return Ok(()); }
        episode.entities.sort(); episode.entities.dedup();
        self.episodes.push_back(episode);
        while self.episodes.len() > self.max_episodes { self.episodes.pop_front(); }
        Ok(())
    }

    pub fn learn(&mut self, outcome: LearningOutcome) -> Result<(), String> {
        if outcome.project_id != self.profile.project_id { return Err("learning outcome belongs to a different project".into()); }
        if !self.episodes.iter().any(|episode| episode.id == outcome.episode_id) { return Err("learning outcome references an unknown episode".into()); }
        let strategy = self.profile.strategies.entry(outcome.strategy.clone()).or_insert(StrategyMemory {
            strategy: outcome.strategy.clone(), attempts: 0, successes: 0, failures: 0,
            total_reward: 0.0, mean_reward: 0.0, confidence: 0.0, last_used_at_ms: None,
        });
        let mut reward = outcome.reward.clamp(-1.0, 1.0);
        if outcome.validation_passed == Some(true) { reward += 0.12; }
        if outcome.validation_passed == Some(false) { reward -= 0.30; }
        if outcome.runtime_improved == Some(true) { reward += 0.15; }
        if outcome.runtime_improved == Some(false) { reward -= 0.15; }
        if outcome.rollback_required { reward -= 0.35; }
        if outcome.human_approved == Some(true) { reward += 0.08; }
        if outcome.human_approved == Some(false) { reward -= 0.15; }
        reward = reward.clamp(-1.0, 1.0);
        strategy.observe(reward, outcome.accepted && !outcome.rollback_required, outcome.observed_at_ms);
        if let Some(invariant) = outcome.invariant_confirmed { self.profile.stable_invariants.insert(invariant); }
        if let Some(pattern) = outcome.rejected_pattern { self.profile.rejected_patterns.insert(pattern); }
        self.profile.generation = self.profile.generation.saturating_add(1);
        self.adapt_policy();
        Ok(())
    }

    fn adapt_policy(&mut self) {
        let strong: Vec<_> = self.profile.strategies.values().filter(|s| s.attempts >= 3 && s.ranking_score() >= 0.55).collect();
        let weak: Vec<_> = self.profile.strategies.values().filter(|s| s.attempts >= 3 && s.ranking_score() <= 0.05).collect();
        let rollback_pressure: f64 = self.episodes.iter().rev().take(50).filter_map(|e| e.metadata.get("rollback")).filter(|v| *v == "true").count() as f64 / 50.0;
        self.profile.risk_threshold = (0.65 + rollback_pressure * 0.20 + weak.len() as f64 * 0.01 - strong.len() as f64 * 0.005).clamp(0.50, 0.90);
        self.profile.retrieval_depth_bias = if self.profile.stable_invariants.len() > 20 { -1 } else if weak.len() > strong.len() { 1 } else { 0 };
        self.profile.validation_evidence_weight = (1.0 + rollback_pressure * 0.5).clamp(1.0, 1.5);
        self.profile.runtime_evidence_weight = if self.episodes.iter().any(|e| matches!(e.kind, MemoryLaneKind::Runtime)) { 1.15 } else { 1.0 };
    }

    pub fn rank_strategies(&self) -> Vec<StrategyMemory> {
        let mut strategies: Vec<_> = self.profile.strategies.values().cloned().collect();
        strategies.sort_by(|a,b| b.ranking_score().partial_cmp(&a.ranking_score()).unwrap_or(Ordering::Equal).then_with(|| a.strategy.cmp(&b.strategy)));
        strategies
    }

    pub fn consolidate(&mut self, now_ms: i64) -> MemoryLaneReflection {
        self.profile.last_consolidated_at_ms = Some(now_ms);
        let errors: Vec<f64> = self.episodes.iter().filter_map(|episode| Some((episode.predicted_score?, episode.observed_score?))).map(|(p,o)| (p-o).abs()).collect();
        let calibrated_prediction_error = if errors.is_empty() { None } else { Some(errors.iter().sum::<f64>() / errors.len() as f64) };
        let ranked = self.rank_strategies();
        let top_strategies = ranked.iter().filter(|s| s.attempts >= 3 && s.ranking_score() > 0.45).take(5).cloned().collect();
        let weak_strategies: Vec<_> = ranked.iter().rev().filter(|s| s.attempts >= 3 && s.ranking_score() < 0.15).take(5).cloned().collect();
        let mut proposals = Vec::new();
        for weak in &weak_strategies {
            let evidence_episode_ids = self.episodes.iter().filter(|e| e.strategy.as_deref() == Some(weak.strategy.as_str())).rev().take(12).map(|e| e.id.clone()).collect();
            proposals.push(ImprovementProposal {
                id: format!("proposal:{}:{}", self.profile.project_id, weak.strategy),
                project_id: self.profile.project_id.clone(),
                target: format!("strategy:{}", weak.strategy),
                rationale: format!("Project-local evidence ranks strategy '{}' below the safe adaptation threshold after {} attempts.", weak.strategy, weak.attempts),
                evidence_episode_ids,
                expected_reward: (0.25 - weak.mean_reward).clamp(-1.0, 1.0),
                evidence: MemoryLaneEvidence::Predicted,
                requires_guarded_change: true,
                requires_compiler_and_tests: true,
                auto_apply_allowed: false,
            });
        }
        MemoryLaneReflection { project_id:self.profile.project_id.clone(), episodes_considered:self.episodes.len(), calibrated_prediction_error, top_strategies, weak_strategies, proposals, profile_generation:self.profile.generation }
    }

    pub fn recall(&self, terms: &[String], limit: usize) -> Vec<MemoryLaneEpisode> {
        let terms: Vec<_> = terms.iter().map(|t| t.to_ascii_lowercase()).filter(|t| !t.is_empty()).collect();
        let mut scored: Vec<(f64,&MemoryLaneEpisode)> = self.episodes.iter().map(|episode| {
            let haystack = format!("{} {} {} {}", episode.title, episode.summary, episode.entities.join(" "), episode.strategy.clone().unwrap_or_default()).to_ascii_lowercase();
            let matches = terms.iter().filter(|term| haystack.contains(term.as_str())).count() as f64;
            let evidence_bonus = match episode.evidence { MemoryLaneEvidence::Validation => 1.0, MemoryLaneEvidence::Runtime => 0.9, MemoryLaneEvidence::Human => 0.85, MemoryLaneEvidence::Static => 0.75, MemoryLaneEvidence::History => 0.65, MemoryLaneEvidence::Predicted => 0.25 };
            (matches * 4.0 + episode.confidence * 2.0 + evidence_bonus, episode)
        }).filter(|(score,_)| terms.is_empty() || *score > 2.0).collect();
        scored.sort_by(|a,b| b.0.partial_cmp(&a.0).unwrap_or(Ordering::Equal).then_with(|| b.1.created_at_ms.cmp(&a.1.created_at_ms)));
        scored.into_iter().take(limit.clamp(1,250)).map(|(_,episode)| episode.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn episode(id:&str,strategy:&str)->MemoryLaneEpisode { MemoryLaneEpisode { id:id.into(), project_id:"p".into(), kind:MemoryLaneKind::Procedural, title:"change".into(), summary:"observed guarded change".into(), entities:vec!["src/a.rs".into()], strategy:Some(strategy.into()), predicted_score:Some(0.8), observed_score:None, evidence:MemoryLaneEvidence::Predicted, confidence:0.7, created_at_ms:1, metadata:BTreeMap::new() } }

    #[test] fn learns_project_local_strategy_without_core_auto_apply() {
        let mut lane=MemoryLaneEngine::new("p");
        for i in 0..4 { let id=format!("e{i}"); lane.remember(episode(&id,"risky")).unwrap(); lane.learn(LearningOutcome { project_id:"p".into(), episode_id:id, strategy:"risky".into(), accepted:false, validation_passed:Some(false), runtime_improved:None, rollback_required:true, human_approved:Some(false), reward:-0.4, observed_at_ms:i, invariant_confirmed:None, rejected_pattern:Some("unsafe-cross-layer-edit".into()) }).unwrap(); }
        let reflection=lane.consolidate(20);
        assert!(!reflection.proposals.is_empty());
        assert!(reflection.proposals.iter().all(|proposal| !proposal.auto_apply_allowed && proposal.requires_guarded_change && proposal.requires_compiler_and_tests));
        assert!(lane.profile.risk_threshold >= 0.65);
    }

    #[test] fn predicted_episode_cannot_fake_observed_truth() {
        let mut lane=MemoryLaneEngine::new("p"); let mut e=episode("x","safe"); e.observed_score=Some(1.0); assert!(lane.remember(e).is_err());
    }
}
