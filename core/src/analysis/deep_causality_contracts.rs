//! Contract snapshots derived from observed causal schema/API evidence.

use crate::analysis::deep_causality::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DerivedContractSnapshot {
    pub contract: ApiContract,
    pub entity_id: String,
    pub repository: Option<String>,
    pub path: Option<String>,
    pub evidence_complete: bool,
}

pub fn derive_contract_snapshots(engine: &DeepCausalityEngine) -> Vec<DerivedContractSnapshot> {
    let mut snapshots = Vec::new();
    for entity in engine.entities().filter(|entity| matches!(entity.kind, CausalEntityKind::Schema | CausalEntityKind::Api)) {
        let mut fields = Vec::new();
        for fact in engine.facts().iter().filter(|fact| fact.from == entity.id && fact.relation == CausalRelationKind::Defines) {
            let Some(child) = engine.entities().find(|candidate| candidate.id == fact.to) else { continue; };
            if !matches!(child.kind, CausalEntityKind::Value | CausalEntityKind::Column | CausalEntityKind::Parameter) { continue; }
            let type_name = child.attributes.get("contract.type")
                .or_else(|| child.attributes.get("protobuf.type"))
                .or_else(|| child.attributes.get("type"))
                .cloned()
                .unwrap_or_else(|| "unknown".to_string());
            let required = child.attributes.get("contract.required")
                .and_then(|value| value.parse::<bool>().ok())
                .unwrap_or(false);
            fields.push(ContractField { name: child.name.clone(), required, type_name });
        }
        fields.sort_by(|left, right| left.name.cmp(&right.name));
        fields.dedup_by(|left, right| left.name == right.name && left.type_name == right.type_name && left.required == right.required);
        if fields.is_empty() { continue; }
        let evidence_complete = fields.iter().all(|field| field.type_name != "unknown");
        snapshots.push(DerivedContractSnapshot {
            contract: ApiContract { id: entity.id.clone(), fields },
            entity_id: entity.id.clone(),
            repository: entity.repository.clone(),
            path: entity.path.clone(),
            evidence_complete,
        });
    }
    snapshots.sort_by(|left, right| left.entity_id.cmp(&right.entity_id));
    snapshots
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn derives_typed_schema_fields() {
        let mut engine=DeepCausalityEngine::new();
        engine.upsert_entity(CausalEntity{id:"schema".into(),kind:CausalEntityKind::Schema,name:"User".into(),repository:Some("r".into()),path:None,attributes:BTreeMap::new()});
        engine.upsert_entity(CausalEntity{id:"field".into(),kind:CausalEntityKind::Value,name:"id".into(),repository:Some("r".into()),path:None,attributes:BTreeMap::from([("contract.type".into(),"ID".into()),("contract.required".into(),"true".into())])});
        engine.add_fact(CausalFact{from:"schema".into(),to:"field".into(),relation:CausalRelationKind::Defines,evidence:CausalEvidenceClass::Static,confidence:1.0,condition:None,timestamp_ms:None,metadata:BTreeMap::new()}).unwrap();
        let snapshots=derive_contract_snapshots(&engine);
        assert_eq!(snapshots[0].contract.fields[0].type_name,"ID");
        assert!(snapshots[0].contract.fields[0].required);
    }
}
