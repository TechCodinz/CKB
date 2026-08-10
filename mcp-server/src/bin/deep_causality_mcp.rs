use ckb_core::analysis::{ApiContract, ArchitectureRule, ChangeOperation, DeepCausalityEngine};
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

fn load_engine() -> anyhow::Result<DeepCausalityEngine> {
    let path = std::env::var("CKB_CAUSALITY_BUNDLE")
        .map_err(|_| anyhow::anyhow!("CKB_CAUSALITY_BUNDLE must point to a DeepCausalityEngine JSON evidence bundle"))?;
    let text = std::fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&text)?)
}

fn text_result(value: Value) -> Value {
    json!({ "content": [{ "type": "text", "text": serde_json::to_string_pretty(&value).unwrap_or_else(|_| "null".into()) }] })
}

fn arg_str<'a>(args: &'a Value, key: &str) -> anyhow::Result<&'a str> {
    args.get(key).and_then(Value::as_str).ok_or_else(|| anyhow::anyhow!("missing string argument `{key}`"))
}

fn arg_usize(args: &Value, key: &str, default: usize) -> usize {
    args.get(key).and_then(Value::as_u64).map(|v| v as usize).unwrap_or(default)
}

fn arg_strings(args: &Value, key: &str) -> Vec<String> {
    args.get(key).and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default()
}

fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": { "type": "object", "properties": properties, "required": required }
    })
}

fn tools() -> Vec<Value> {
    vec![
        tool("ckb_data_flow", "Find an evidence-backed interprocedural data-flow path.", json!({"source":{"type":"string"},"sink":{"type":"string"},"depth":{"type":"integer"}}), &["source","sink"]),
        tool("ckb_taint", "Find unsanitized flows from untrusted sources to sinks. Sanitizer/validator evidence suppresses only the path it actually protects.", json!({"sources":{"type":"array","items":{"type":"string"}},"sinks":{"type":"array","items":{"type":"string"}},"depth":{"type":"integer"}}), &["sources","sinks"]),
        tool("ckb_reachable_under", "Evaluate path-sensitive reachability under recorded conditions.", json!({"source":{"type":"string"},"sink":{"type":"string"},"conditions":{"type":"array","items":{"type":"string"}},"depth":{"type":"integer"}}), &["source","sink"]),
        tool("ckb_constraints", "Check bounded equality/inequality and numeric range constraint satisfiability.", json!({"constraints":{"type":"array","items":{"type":"string"}}}), &["constraints"]),
        tool("ckb_concurrency_hazards", "Detect unprotected multi-writers and lock/wait cycles.", json!({}), &[]),
        tool("ckb_schema_impact", "Find code/API/test/migration impact of a schema entity.", json!({"entity":{"type":"string"},"depth":{"type":"integer"}}), &["entity"]),
        tool("ckb_infra_impact", "Find service/deployment/repository impact of infrastructure changes.", json!({"entity":{"type":"string"},"depth":{"type":"integer"}}), &["entity"]),
        tool("ckb_config_impact", "Trace configuration/feature-flag dependents.", json!({"entity":{"type":"string"},"depth":{"type":"integer"}}), &["entity"]),
        tool("ckb_distributed_flow", "Trace event/queue/job/service flow across distributed boundaries.", json!({"source":{"type":"string"},"sink":{"type":"string"},"depth":{"type":"integer"}}), &["source","sink"]),
        tool("ckb_contract_diff", "Classify API/schema contract evolution.", json!({"before":{"type":"object"},"after":{"type":"object"}}), &["before","after"]),
        tool("ckb_tests_for_change", "Select evidence-connected behavioral tests for changed entities.", json!({"changed":{"type":"array","items":{"type":"string"}},"depth":{"type":"integer"}}), &["changed"]),
        tool("ckb_policy", "Enforce executable architecture invariants.", json!({"rules":{"type":"array"}}), &["rules"]),
        tool("ckb_drift_forecast", "Forecast structural relation-count drift from history. This is PREDICTIVE trend output, not observed future truth.", json!({"edgeCounts":{"type":"array","items":{"type":"integer"}},"horizon":{"type":"integer"}}), &["edgeCounts"]),
        tool("ckb_simulate_change", "Simulate proposed changes; results are always labeled PREDICTED.", json!({"operations":{"type":"array"},"depth":{"type":"integer"}}), &["operations"]),
        tool("ckb_runtime_hotspots", "Rank observed runtime CPU/memory/latency/error hotspots.", json!({}), &[]),
        tool("ckb_failure_propagation", "Propagate a dependency/resource failure back through callers/dependents and forward through observed delivery semantics.", json!({"source":{"type":"string"},"depth":{"type":"integer"}}), &["source"]),
        tool("ckb_temporal_diff", "Compare current causal architecture against an older evidence bundle.", json!({"olderBundle":{"type":"string"}}), &["olderBundle"]),
        tool("ckb_cross_repo_path", "Find a causal path only when it crosses repository boundaries.", json!({"source":{"type":"string"},"sink":{"type":"string"},"depth":{"type":"integer"}}), &["source","sink"]),
        tool("ckb_ownership_risk", "Measure bus-factor/ownership risk from ownership/authorship/review facts.", json!({}), &[]),
        tool("ckb_quality_metrics", "Compute evidence-derived coupling/cyclicity/instability metrics.", json!({}), &[]),
    ]
}

fn call(engine: &DeepCausalityEngine, name: &str, args: &Value) -> anyhow::Result<Value> {
    Ok(match name {
        "ckb_data_flow" => serde_json::to_value(engine.data_flow_path(arg_str(args,"source")?, arg_str(args,"sink")?, arg_usize(args,"depth",24)))?,
        "ckb_taint" => serde_json::to_value(engine.taint_paths_v2(&arg_strings(args,"sources"), &arg_strings(args,"sinks"), arg_usize(args,"depth",24)))?,
        "ckb_reachable_under" => serde_json::to_value(engine.reachable_under(arg_str(args,"source")?, arg_str(args,"sink")?, &arg_strings(args,"conditions"), arg_usize(args,"depth",24)))?,
        "ckb_constraints" => json!({"satisfiable": engine.constraints_satisfiable_v2(&arg_strings(args,"constraints"))}),
        "ckb_concurrency_hazards" => serde_json::to_value(engine.concurrency_hazards())?,
        "ckb_schema_impact" => serde_json::to_value(engine.schema_impact(arg_str(args,"entity")?, arg_usize(args,"depth",12)))?,
        "ckb_infra_impact" => serde_json::to_value(engine.infrastructure_impact(arg_str(args,"entity")?, arg_usize(args,"depth",12)))?,
        "ckb_config_impact" => serde_json::to_value(engine.config_dependents(arg_str(args,"entity")?, arg_usize(args,"depth",12)))?,
        "ckb_distributed_flow" => serde_json::to_value(engine.distributed_flow(arg_str(args,"source")?, arg_str(args,"sink")?, arg_usize(args,"depth",32)))?,
        "ckb_contract_diff" => {
            let before: ApiContract = serde_json::from_value(args.get("before").cloned().unwrap_or(Value::Null))?;
            let after: ApiContract = serde_json::from_value(args.get("after").cloned().unwrap_or(Value::Null))?;
            serde_json::to_value(engine.compare_contracts(&before, &after))?
        },
        "ckb_tests_for_change" => serde_json::to_value(engine.tests_for_change(&arg_strings(args,"changed"), arg_usize(args,"depth",12)))?,
        "ckb_policy" => {
            let rules: Vec<ArchitectureRule> = serde_json::from_value(args.get("rules").cloned().unwrap_or_else(|| json!([])))?;
            serde_json::to_value(engine.enforce_rules(&rules))?
        },
        "ckb_drift_forecast" => {
            let counts = args.get("edgeCounts").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).map(|v| v as usize).collect::<Vec<_>>()).unwrap_or_default();
            json!({"evidence":"predicted","values":engine.forecast_drift(&counts, arg_usize(args,"horizon",5))})
        },
        "ckb_simulate_change" => {
            let ops: Vec<ChangeOperation> = serde_json::from_value(args.get("operations").cloned().unwrap_or_else(|| json!([])))?;
            serde_json::to_value(engine.simulate_change(&ops, arg_usize(args,"depth",12)))?
        },
        "ckb_runtime_hotspots" => serde_json::to_value(engine.runtime_hotspots())?,
        "ckb_failure_propagation" => serde_json::to_value(engine.failure_propagation_v2(arg_str(args,"source")?, arg_usize(args,"depth",12)))?,
        "ckb_temporal_diff" => {
            let older: DeepCausalityEngine = serde_json::from_str(&std::fs::read_to_string(arg_str(args,"olderBundle")?)?)?;
            let (added, removed) = engine.temporal_diff(&older);
            json!({"added":added,"removed":removed})
        },
        "ckb_cross_repo_path" => serde_json::to_value(engine.cross_repo_path(arg_str(args,"source")?, arg_str(args,"sink")?, arg_usize(args,"depth",32)))?,
        "ckb_ownership_risk" => serde_json::to_value(engine.ownership_risks())?,
        "ckb_quality_metrics" => serde_json::to_value(engine.quality_metrics())?,
        _ => anyhow::bail!("unknown tool {name}"),
    })
}

fn main() -> anyhow::Result<()> {
    let engine = load_engine()?;
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() { continue; }
        let request: Value = match serde_json::from_str(&line) { Ok(v) => v, Err(e) => { writeln!(stdout,"{}",json!({"jsonrpc":"2.0","error":{"code":-32700,"message":e.to_string()},"id":Value::Null}))?; stdout.flush()?; continue; } };
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let method = request.get("method").and_then(Value::as_str).unwrap_or("");
        let response = match method {
            "initialize" => json!({"jsonrpc":"2.0","id":id,"result":{"protocolVersion":"2025-03-26","capabilities":{"tools":{}},"serverInfo":{"name":"ckb-deep-causality","version":"13.1.0"}}}),
            "notifications/initialized" => continue,
            "tools/list" => json!({"jsonrpc":"2.0","id":id,"result":{"tools":tools()}}),
            "tools/call" => {
                let params = request.get("params").cloned().unwrap_or(Value::Null);
                let name = params.get("name").and_then(Value::as_str).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
                match call(&engine, name, &args) {
                    Ok(v) => json!({"jsonrpc":"2.0","id":id,"result":text_result(v)}),
                    Err(e) => json!({"jsonrpc":"2.0","id":id,"result":{"isError":true,"content":[{"type":"text","text":e.to_string()}]}}),
                }
            },
            _ => json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":format!("method not found: {method}")}}),
        };
        writeln!(stdout, "{}", response)?;
        stdout.flush()?;
    }
    Ok(())
}
