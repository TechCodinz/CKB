//! Persistent storage for code knowledge graphs using RocksDB

mod models;

use rocksdb::{DB, Options, IteratorMode, WriteBatch};
use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::Result;
use serde::{Serialize, Deserialize};
use crate::graph::DependencyGraph;
use crate::types::*;

pub struct GraphStorage {
    db: Arc<DB>,
    cache: Arc<RwLock<lru::LruCache<String, Vec<u8>>>>,
}

impl GraphStorage {
    pub fn new(path: &str) -> Result<Self> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.set_compression_type(rocksdb::DBCompressionType::Lz4);
        opts.set_write_buffer_size(64 * 1024 * 1024); // 64MB
        opts.set_max_write_buffer_number(3);
        opts.set_target_file_size_base(64 * 1024 * 1024); // 64MB
        
        let db = DB::open(&opts, path)?;
        
        Ok(Self {
            db: Arc::new(db),
            cache: Arc::new(RwLock::new(lru::LruCache::new(1000))),
        })
    }
    
    pub async fn store_snapshot(&self, graph: &DependencyGraph) -> Result<String> {
        let snapshot_id = uuid::Uuid::new_v4().to_string();
        let key = format!("snapshot:{}", snapshot_id);
        
        // Serialize graph
        let bytes = bincode::serialize(graph)?;
        
        // Store in batch with metadata
        let mut batch = WriteBatch::default();
        batch.put(key.as_bytes(), &bytes);
        
        // Store metadata
        let metadata = SnapshotMetadata {
            id: snapshot_id.clone(),
            timestamp: chrono::Utc::now(),
            node_count: graph.node_count(),
            edge_count: graph.edge_count(),
        };
        
        let meta_bytes = bincode::serialize(&metadata)?;
        batch.put(format!("snapshot_meta:{}", snapshot_id).as_bytes(), &meta_bytes);
        
        // Update latest pointer
        batch.put("latest_snapshot".as_bytes(), snapshot_id.as_bytes());
        
        self.db.write(batch)?;
        
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
            cache.put(key, bytes);
            
            Ok(Some(graph))
        } else {
            Ok(None)
        }
    }
    
    pub async fn get_latest_snapshot(&self) -> Result<Option<DependencyGraph>> {
        if let Some(snapshot_id) = self.db.get("latest_snapshot".as_bytes())? {
            let id = String::from_utf8(snapshot_id)?;
            self.load_snapshot(&id).await
        } else {
            Ok(None)
        }
    }
    
    pub async fn list_snapshots(&self) -> Result<Vec<SnapshotMetadata>> {
        let mut snapshots = Vec::new();
        let iter = self.db.iterator(IteratorMode::Start);
        
        for item in iter {
            let (key, value) = item?;
            if key.starts_with(b"snapshot_meta:") {
                if let Ok(metadata) = bincode::deserialize::<SnapshotMetadata>(&value) {
                    snapshots.push(metadata);
                }
            }
        }
        
        Ok(snapshots)
    }
    
    pub async fn delete_snapshot(&self, snapshot_id: &str) -> Result<()> {
        let key = format!("snapshot:{}", snapshot_id);
        let meta_key = format!("snapshot_meta:{}", snapshot_id);
        
        let mut batch = WriteBatch::default();
        batch.delete(key.as_bytes());
        batch.delete(meta_key.as_bytes());
        
        self.db.write(batch)?;
        
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
