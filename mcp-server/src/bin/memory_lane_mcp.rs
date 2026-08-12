use ckb_core::{LearningOutcome, MemoryLaneEpisode, MemoryLaneStore};
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

fn workspace() -> String { std::env::var("CKB_WORKSPACE").unwrap_or_else(|_| ".".into()) }
fn project() -> anyhow::Result<String> { std::env::var("CKB_PROJECT_ID").map_err(|_| anyhow::anyhow!("CKB_PROJECT_ID is required")) }
fn now_ms() -> i64 { chrono::Utc::now().timestamp_millis() }
fn text_result(value: Value) -> Value { json!({"content":[{"type":"text","text":serde_json::to_string_pretty(&value).unwrap_or_else(|_|"null".into())}]}) }

fn tools() -> Vec<Value> { vec![
    json!({"name":"ckb_memory_lane_status","description":"Read the project-local adaptive Memory Lane profile and ranked strategies.","inputSchema":{"type":"object","properties":{}}}),
    json!({"name":"ckb_memory_lane_recall","description":"Recall project-specific episodic/semantic/procedural/runtime memories.","inputSchema":{"type":"object","properties":{"terms":{"type":"array","items":{"type":"string"}},"limit":{"type":"integer"}}}}),
    json!({"name":"ckb_memory_lane_reflect","description":"Consolidate observed project outcomes and generate guarded PREDICTED improvement proposals. Core source is never auto-applied.","inputSchema":{"type":"object","properties":{}}}),
    json!({"name":"ckb_memory_lane_remember","description":"Record an explicit evidence-classified Memory Lane episode for this project.","inputSchema":{"type":"object","properties":{"episode":{"type":"object"}},"required":["episode"]}}),
    json!({"name":"ckb_memory_lane_learn","description":"Learn from a validation/runtime/human outcome and adapt project-local strategy/risk policy.","inputSchema":{"type":"object","properties":{"outcome":{"type":"object"}},"required":["outcome"]}}),
    json!({"name":"ckb_memory_lane_checkpoint","description":"Persist and content-fingerprint a project-local Memory Lane checkpoint.","inputSchema":{"type":"object","properties":{}}}),
] }

fn call(name:&str,args:&Value)->anyhow::Result<Value>{
    let project=project()?;
    let store=MemoryLaneStore::new(workspace());
    let mut lane=store.load_or_new(&project)?;
    Ok(match name {
        "ckb_memory_lane_status"=>json!({"version":lane.version,"profile":lane.profile,"episodes":lane.episodes().count(),"strategies":lane.rank_strategies()}),
        "ckb_memory_lane_recall"=>{
            let terms: Vec<String>=args.get("terms").and_then(Value::as_array).map(|v|v.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
            let limit=args.get("limit").and_then(Value::as_u64).map(|v|v as usize).unwrap_or(20);
            serde_json::to_value(lane.recall(&terms,limit))?
        },
        "ckb_memory_lane_reflect"=>{let reflection=lane.consolidate(now_ms());store.save(&lane)?;serde_json::to_value(reflection)?},
        "ckb_memory_lane_remember"=>{let episode:MemoryLaneEpisode=serde_json::from_value(args.get("episode").cloned().unwrap_or(Value::Null))?;lane.remember(episode).map_err(anyhow::Error::msg)?;store.save(&lane)?;json!({"saved":true,"episodes":lane.episodes().count()})},
        "ckb_memory_lane_learn"=>{let outcome:LearningOutcome=serde_json::from_value(args.get("outcome").cloned().unwrap_or(Value::Null))?;lane.learn(outcome).map_err(anyhow::Error::msg)?;store.save(&lane)?;json!({"learned":true,"profile":lane.profile,"strategies":lane.rank_strategies()})},
        "ckb_memory_lane_checkpoint"=>{store.save(&lane)?;serde_json::to_value(store.checkpoint(&lane,now_ms())?)?},
        _=>anyhow::bail!("unknown tool {name}"),
    })
}

fn main()->anyhow::Result<()> {
    let stdin=io::stdin(); let mut stdout=io::stdout().lock();
    for line in stdin.lock().lines(){
        let line=line?; if line.trim().is_empty(){continue;}
        let request:Value=match serde_json::from_str(&line){Ok(value)=>value,Err(error)=>{writeln!(stdout,"{}",json!({"jsonrpc":"2.0","id":Value::Null,"error":{"code":-32700,"message":error.to_string()}}))?;stdout.flush()?;continue;}};
        let id=request.get("id").cloned().unwrap_or(Value::Null); let method=request.get("method").and_then(Value::as_str).unwrap_or("");
        let response=match method {
            "initialize"=>json!({"jsonrpc":"2.0","id":id,"result":{"protocolVersion":"2025-03-26","capabilities":{"tools":{}},"serverInfo":{"name":"ckb-memory-lane","version":"13.2.0"}}}),
            "notifications/initialized"=>continue,
            "tools/list"=>json!({"jsonrpc":"2.0","id":id,"result":{"tools":tools()}}),
            "tools/call"=>{let params=request.get("params").cloned().unwrap_or(Value::Null);let name=params.get("name").and_then(Value::as_str).unwrap_or("");let args=params.get("arguments").cloned().unwrap_or_else(||json!({}));match call(name,&args){Ok(value)=>json!({"jsonrpc":"2.0","id":id,"result":text_result(value)}),Err(error)=>json!({"jsonrpc":"2.0","id":id,"result":{"isError":true,"content":[{"type":"text","text":error.to_string()}]}})}},
            _=>json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":format!("method not found: {method}")}}),
        };
        writeln!(stdout,"{}",response)?; stdout.flush()?;
    }
    Ok(())
}
