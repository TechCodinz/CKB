use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use ckb_core::{
    observe_causal_snapshot, DeepCausalityEngine, LearningOutcome, MemoryLaneEpisode,
    MemoryLaneStore,
};
use serde::de::DeserializeOwned;
use serde_json::json;
use std::{fs, path::Path};

#[derive(Parser)]
#[command(name="ckb-memory-lane", version, about="CKB V13.2 project-adaptive Memory Lane")]
struct Cli {
    #[arg(long, default_value=".")]
    workspace: String,
    #[arg(long)]
    project: String,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Status,
    Remember { episode: String },
    Learn { outcome: String },
    Observe { bundle: String, snapshot: String },
    Recall { #[arg(value_delimiter=',')] terms: Vec<String>, #[arg(long, default_value_t=20)] limit: usize },
    Reflect,
    Checkpoint,
    Restore { checkpoint: String },
}

fn read_json<T: DeserializeOwned>(path:&str)->Result<T>{
    let text=fs::read_to_string(path).with_context(||format!("read {path}"))?;
    serde_json::from_str(&text).with_context(||format!("parse JSON {path}"))
}
fn now_ms()->i64{chrono::Utc::now().timestamp_millis()}

fn main()->Result<()> {
    let cli=Cli::parse();
    let store=MemoryLaneStore::new(Path::new(&cli.workspace));
    let mut lane=store.load_or_new(&cli.project)?;
    match cli.command {
        Command::Status => {
            println!("{}",serde_json::to_string_pretty(&json!({
                "version":lane.version,
                "profile":lane.profile,
                "episodes":lane.episodes().count(),
                "strategies":lane.rank_strategies(),
            }))?);
        }
        Command::Remember{episode} => {
            let episode:MemoryLaneEpisode=read_json(&episode)?;
            lane.remember(episode).map_err(anyhow::Error::msg)?;
            store.save(&lane)?;
            println!("{}",serde_json::to_string_pretty(&json!({"saved":true,"episodes":lane.episodes().count()}))?);
        }
        Command::Learn{outcome} => {
            let outcome:LearningOutcome=read_json(&outcome)?;
            lane.learn(outcome).map_err(anyhow::Error::msg)?;
            store.save(&lane)?;
            println!("{}",serde_json::to_string_pretty(&json!({"learned":true,"profile":lane.profile,"strategies":lane.rank_strategies()}))?);
        }
        Command::Observe{bundle,snapshot} => {
            let causality:DeepCausalityEngine=read_json(&bundle)?;
            let remembered=observe_causal_snapshot(&mut lane,&causality,&snapshot,now_ms())?;
            store.save(&lane)?;
            println!("{}",serde_json::to_string_pretty(&json!({"remembered":remembered,"episodes":lane.episodes().count(),"snapshot":snapshot}))?);
        }
        Command::Recall{terms,limit} => {
            println!("{}",serde_json::to_string_pretty(&lane.recall(&terms,limit))?);
        }
        Command::Reflect => {
            let reflection=lane.consolidate(now_ms());
            store.save(&lane)?;
            println!("{}",serde_json::to_string_pretty(&reflection)?);
        }
        Command::Checkpoint => {
            store.save(&lane)?;
            println!("{}",serde_json::to_string_pretty(&store.checkpoint(&lane,now_ms())?)?);
        }
        Command::Restore{checkpoint} => {
            let restored=store.restore_checkpoint(&checkpoint,&cli.project)?;
            println!("{}",serde_json::to_string_pretty(&json!({"restored":checkpoint,"profile":restored.profile,"episodes":restored.episodes().count()}))?);
        }
    }
    Ok(())
}
