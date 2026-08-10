//! Explicit distributed-event identity extraction for V13.1.
//!
//! Only literal names present in event/queue/topic APIs are unified. Producer
//! edges point into the shared event/topic/queue; consumer edges point outward
//! from it so causal paths can traverse producer -> event -> consumer.

use crate::analysis::deep_causality::*;
use crate::analysis::deep_causality_extractors::RepositoryArtifact;
use std::collections::BTreeMap;

pub fn enrich_event_identity(engine: &mut DeepCausalityEngine, artifacts: &[RepositoryArtifact]) {
    for artifact in artifacts {
        let file_id = format!("repo:{}::file:{}", artifact.repository, normalize(&artifact.path));
        ensure(engine, CausalEntity {
            id: file_id.clone(),
            kind: CausalEntityKind::File,
            name: artifact.path.rsplit('/').next().unwrap_or(&artifact.path).to_string(),
            repository: Some(artifact.repository.clone()),
            path: Some(normalize(&artifact.path)),
            attributes: BTreeMap::new(),
        });

        for (index, line) in artifact.content.lines().enumerate() {
            for observation in event_literals(line) {
                let id = format!("event:{}", observation.name);
                ensure(engine, CausalEntity {
                    id: id.clone(),
                    kind: observation.kind,
                    name: observation.name.clone(),
                    repository: None,
                    path: None,
                    attributes: BTreeMap::from([
                        ("identity.basis".to_string(), "explicit_literal".to_string()),
                        ("distributed.kind".to_string(), observation.kind_name.to_string()),
                    ]),
                });
                let (from, to) = if observation.consumer {
                    (id, file_id.clone())
                } else {
                    (file_id.clone(), id)
                };
                let fact = CausalFact {
                    from,
                    to,
                    relation: observation.relation,
                    evidence: CausalEvidenceClass::Static,
                    confidence: 1.0,
                    condition: None,
                    timestamp_ms: None,
                    metadata: BTreeMap::from([
                        ("source.path".to_string(), artifact.path.clone()),
                        ("source.line".to_string(), (index + 1).to_string()),
                        ("api.method".to_string(), observation.method.to_string()),
                    ]),
                };
                add(engine, fact);
            }
        }
    }
}

#[derive(Debug, Clone)]
struct EventObservation {
    name: String,
    method: &'static str,
    kind: CausalEntityKind,
    kind_name: &'static str,
    relation: CausalRelationKind,
    consumer: bool,
}

fn event_literals(line: &str) -> Vec<EventObservation> {
    let candidates = [
        (".emit(", "emit", CausalEntityKind::Event, "event", CausalRelationKind::Emits, false),
        (".publish(", "publish", CausalEntityKind::Topic, "topic", CausalRelationKind::Publishes, false),
        (".produce(", "produce", CausalEntityKind::Topic, "topic", CausalRelationKind::Publishes, false),
        (".send(", "send", CausalEntityKind::Queue, "queue", CausalRelationKind::Publishes, false),
        (".subscribe(", "subscribe", CausalEntityKind::Topic, "topic", CausalRelationKind::Subscribes, true),
        (".consume(", "consume", CausalEntityKind::Queue, "queue", CausalRelationKind::Consumes, true),
        (".on(", "on", CausalEntityKind::Event, "event", CausalRelationKind::Consumes, true),
    ];
    let mut out = Vec::new();
    for (needle, method, kind, kind_name, relation, consumer) in candidates {
        let mut cursor = 0usize;
        while cursor < line.len() {
            let Some(relative) = line[cursor..].find(needle) else { break; };
            let start = cursor + relative + needle.len();
            let tail = &line[start..];
            let Some((name, consumed)) = leading_literal(tail) else {
                cursor = start.saturating_add(1);
                continue;
            };
            if !name.is_empty() {
                out.push(EventObservation { name, method, kind: kind.clone(), kind_name, relation: relation.clone(), consumer });
            }
            cursor = start.saturating_add(consumed).max(start + 1);
        }
    }
    out
}

fn leading_literal(text: &str) -> Option<(String, usize)> {
    let trimmed = text.trim_start();
    let skipped = text.len().saturating_sub(trimmed.len());
    let quote = trimmed.chars().next()?;
    if quote != '"' && quote != '\'' && quote != '`' { return None; }
    let body = &trimmed[quote.len_utf8()..];
    let end = body.find(quote)?;
    let value = body[..end].trim().to_string();
    Some((value, skipped + quote.len_utf8() + end + quote.len_utf8()))
}

fn ensure(engine: &mut DeepCausalityEngine, entity: CausalEntity) {
    if !engine.entities().any(|existing| existing.id == entity.id) {
        engine.upsert_entity(entity);
    }
}
fn add(engine: &mut DeepCausalityEngine, fact: CausalFact) {
    if !engine.facts().iter().any(|existing| existing == &fact) {
        let _ = engine.add_fact(fact);
    }
}
fn normalize(path: &str) -> String { path.replace('\\', "/").trim_start_matches("./").to_string() }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_shared_topic_literal() {
        let rows = event_literals("producer.publish(\"orders.created\", payload)");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "orders.created");
        assert_eq!(rows[0].relation, CausalRelationKind::Publishes);
        assert!(!rows[0].consumer);
    }

    #[test]
    fn consumer_edges_are_marked_reverse_direction() {
        let rows = event_literals("bus.subscribe(\"orders.created\", handler)");
        assert_eq!(rows.len(), 1);
        assert!(rows[0].consumer);
    }

    #[test]
    fn ignores_dynamic_topic_names() {
        assert!(event_literals("producer.publish(topicName, payload)").is_empty());
    }
}
