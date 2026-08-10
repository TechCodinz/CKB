//! Persistence and causal observation bridge for V13.2 Memory Lane.

use crate::analysis::deep_causality::{CausalEvidenceClass, CausalEntity, DeepCausalityEngine};
use crate::analysis::deep_causality_bundle::memory_lane::{MemoryLaneEngine, MemoryLaneEpisode, MemoryLaneEvidence, MemoryLaneKind};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryLaneCheckpoint {
    pub id: String,
    pub project_id: String,
    pub created_at_ms: i64,
    pub profile_generation: u64,
    pub episodes: usize,
    pub path: String,
}

pub struct MemoryLaneStore { root: PathBuf }

impl MemoryLaneStore {
    pub fn new(workspace: impl AsRef<Path>) -> Self { Self { root: workspace.as_ref().join(".ckb").join("memory-lane") } }
    pub fn current_path(&self) -> PathBuf { self.root.join("current.json") }
    pub fn checkpoints_dir(&self) -> PathBuf { self.root.join("checkpoints") }

    pub fn load_or_new(&self, project_id: &str) -> Result<MemoryLaneEngine> {
        let current = self.current_path();
        if !current.exists() { return Ok(MemoryLaneEngine::new(project_id)); }
        let bytes = fs::read(&current).with_context(|| format!("read {}", current.display()))?;
        let engine: MemoryLaneEngine = serde_json::from_slice(&bytes).context("parse Memory Lane state")?;
        if engine.profile.project_id != project_id { anyhow::bail!("Memory Lane belongs to project '{}' not '{}'", engine.profile.project_id, project_id); }
        Ok(engine)
    }

    pub fn save(&self, engine: &MemoryLaneEngine) -> Result<()> {
        fs::create_dir_all(&self.root)?;
        let path = self.current_path();
        let temp = self.root.join("current.json.tmp");
        fs::write(&temp, serde_json::to_vec_pretty(engine)?)?;
        fs::rename(&temp, &path)?;
        Ok(())
    }

    pub fn checkpoint(&self, engine: &MemoryLaneEngine, now_ms: i64) -> Result<MemoryLaneCheckpoint> {
        let bytes = serde_json::to_vec_pretty(engine)?;
        let id = format!("ml-{}", stable_fingerprint(&bytes));
        let dir = self.checkpoints_dir();
        fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{id}.json"));
        if !path.exists() { fs::write(&path, bytes)?; }
        Ok(MemoryLaneCheckpoint { id, project_id:engine.profile.project_id.clone(), created_at_ms:now_ms, profile_generation:engine.profile.generation, episodes:engine.episodes().count(), path:path.to_string_lossy().replace('\\', "/") })
    }

    pub fn restore_checkpoint(&self, checkpoint_id: &str, project_id: &str) -> Result<MemoryLaneEngine> {
        if !checkpoint_id.starts_with("ml-") || checkpoint_id.contains('/') || checkpoint_id.contains('\\') { anyhow::bail!("invalid Memory Lane checkpoint id"); }
        let path = self.checkpoints_dir().join(format!("{checkpoint_id}.json"));
        let bytes = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
        let engine: MemoryLaneEngine = serde_json::from_slice(&bytes)?;
        if engine.profile.project_id != project_id { anyhow::bail!("checkpoint belongs to a different project"); }
        self.save(&engine)?;
        Ok(engine)
    }
}

// Dependency-free FNV-1a style content fingerprint. This is for deterministic
// snapshot naming/deduplication, not for cryptographic authentication.
fn stable_fingerprint(bytes:&[u8])->String {
    let mut a:u64=0xcbf29ce484222325;
    let mut b:u64=0x84222325cbf29ce4;
    for (index,byte) in bytes.iter().enumerate() {
        a^=*byte as u64; a=a.wrapping_mul(0x100000001b3);
        b^=((*byte as u64)<<((index%8) as u32)) ^ index as u64; b=b.wrapping_mul(0x100000001b3);
    }
    format!("{a:016x}{b:016x}")
}

pub fn observe_causal_snapshot(lane:&mut MemoryLaneEngine, causality:&DeepCausalityEngine, snapshot_id:&str, now_ms:i64) -> Result<usize> {
    let mut remembered=0usize;
    for entity in causality.entities() {
        let related:Vec<_>=causality.facts().iter().filter(|fact| fact.from==entity.id || fact.to==entity.id).collect();
        if related.is_empty(){continue;}
        let has_runtime=related.iter().any(|fact| matches!(fact.evidence,CausalEvidenceClass::Runtime));
        let has_validation=related.iter().any(|fact| matches!(fact.evidence,CausalEvidenceClass::Validation));
        let evidence=if has_validation{MemoryLaneEvidence::Validation}else if has_runtime{MemoryLaneEvidence::Runtime}else{MemoryLaneEvidence::Static};
        let episode=MemoryLaneEpisode{id:format!("snapshot:{}:{}",snapshot_id,entity.id),project_id:lane.profile.project_id.clone(),kind:if has_runtime{MemoryLaneKind::Runtime}else{MemoryLaneKind::Semantic},title:format!("{} {}",kind_name(entity),entity.name),summary:format!("Observed {} causal relations for {} in architecture snapshot {}.",related.len(),entity.id,snapshot_id),entities:vec![entity.id.clone()],strategy:None,predicted_score:None,observed_score:None,evidence,confidence:related.iter().map(|fact|fact.confidence as f64).fold(1.0,f64::min),created_at_ms:now_ms,metadata:std::collections::BTreeMap::from([("snapshotId".into(),snapshot_id.into()),("relationCount".into(),related.len().to_string())])};
        lane.remember(episode).map_err(anyhow::Error::msg)?; remembered+=1;
    }
    Ok(remembered)
}

fn kind_name(entity:&CausalEntity)->String{format!("{:?}",entity.kind).to_ascii_lowercase()}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::deep_causality::CausalEntityKind;
    use std::collections::BTreeMap;
    #[test] fn causal_snapshot_without_relations_does_not_invent_memory(){let mut lane=MemoryLaneEngine::new("p");let engine=DeepCausalityEngine::from_facts(vec![CausalEntity{id:"file".into(),kind:CausalEntityKind::File,name:"a.rs".into(),repository:Some("p".into()),path:Some("a.rs".into()),attributes:BTreeMap::new()}],vec![]);assert_eq!(observe_causal_snapshot(&mut lane,&engine,"s1",1).unwrap(),0);}
    #[test] fn fingerprint_is_stable(){assert_eq!(stable_fingerprint(b"ckb"),stable_fingerprint(b"ckb"));assert_ne!(stable_fingerprint(b"ckb"),stable_fingerprint(b"CKB"));}
}
