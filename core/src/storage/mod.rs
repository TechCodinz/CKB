//! Persistent storage for code knowledge graphs using sled (pure Rust DB)

mod models;

use sled::Db;
use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::Result;
use serde::{Serialize, Deserialize};
use crate::graph::DependencyGraph;

pub struct GraphStorage {
    db: Arc<Db>,
    cache: Arc<RwLock<lru::LruCache<String, Vec<u8>>>>,
}

impl GraphStorage {
    pub fn new(path: &str) -> Result<Self> {
        let db = sled::open(path)?;
        
        Ok(Self {
            db: Arc::new(db),
            cache: Arc::new(RwLock::new(lru::LruCache::new(std::num::NonZeroUsize::new(1000).unwrap()))),
        })
    }
    
    pub async fn store_snapshot(&self, graph: &DependencyGraph) -> Result<String> {
        let snapshot_id = uuid::Uuid::new_v4().to_string();
        let key = format!("snapshot:{}", snapshot_id);
        
        // Serialize graph
        let bytes = bincode::serialize(graph)?;
        self.db.insert(key.as_bytes(), bytes.as_slice())?;
        
        // Store metadata
        let metadata = SnapshotMetadata {
            id: snapshot_id.clone(),
            timestamp: chrono::Utc::now(),
            node_count: graph.node_count(),
            edge_count: graph.edge_count(),
        };
        
        let meta_bytes = bincode::serialize(&metadata)?;
        self.db.insert(format!("snapshot_meta:{}", snapshot_id).as_bytes(), meta_bytes.as_slice())?;
        
        // Update latest pointer
        self.db.insert("latest_snapshot".as_bytes(), snapshot_id.as_bytes())?;
        self.db.flush()?;
        
        Ok(snapshot_id)
    }
    
    pub async fn load_snapshot(&self, snapshot_id: &str) -> Result<Option<DependencyGraph>> {
        let key = format!("snapshot:{}", snapshot_id);
        
        // Check cache first
        {
            let mut cache = self.cache.write().await;
            if let Some(bytes) = cache.get(&key) {
                return Ok(Some(bincode::deserialize(bytes)?));
            }
        }
        
        // Load from DB
        if let Some(bytes) = self.db.get(key.as_bytes())? {
            let graph: DependencyGraph = bincode::deserialize(&bytes)?;
            
            // Update cache
            let mut cache = self.cache.write().await;
            cache.put(key, bytes.to_vec());
            
            Ok(Some(graph))
        } else {
            Ok(None)
        }
    }
    
    pub async fn get_latest_snapshot(&self) -> Result<Option<DependencyGraph>> {
        if let Some(snapshot_id) = self.db.get("latest_snapshot".as_bytes())? {
            let id = String::from_utf8(snapshot_id.to_vec())?;
            self.load_snapshot(&id).await
        } else {
            Ok(None)
        }
    }
    
    pub async fn list_snapshots(&self) -> Result<Vec<SnapshotMetadata>> {
        let mut snapshots = Vec::new();
        
        for item in self.db.scan_prefix("snapshot_meta:") {
            let (_key, value) = item?;
            if let Ok(metadata) = bincode::deserialize::<SnapshotMetadata>(&value) {
                snapshots.push(metadata);
            }
        }
        
        Ok(snapshots)
    }
    
    pub async fn delete_snapshot(&self, snapshot_id: &str) -> Result<()> {
        let key = format!("snapshot:{}", snapshot_id);
        let meta_key = format!("snapshot_meta:{}", snapshot_id);
        
        self.db.remove(key.as_bytes())?;
        self.db.remove(meta_key.as_bytes())?;
        self.db.flush()?;
        
        // Remove from cache
        let mut cache = self.cache.write().await;
        cache.pop(&key);
        
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SnapshotMetadata {
    pub id: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub node_count: usize,
    pub edge_count: usize,
}
