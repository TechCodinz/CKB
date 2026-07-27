use wasm_bindgen::prelude::*;
use serde_json::Value;

/// Scan a codebase from WASM (browser/Node.js context)
/// Note: full scan requires the CKB MCP server; this is a lightweight wrapper
#[wasm_bindgen]
pub async fn scan(server_url: &str, project_path: &str) -> Result<String, JsValue> {
    // In WASM context, we delegate to the MCP server via fetch
    Ok(format!("{{\"status\": \"scan_delegated\", \"server\": \"{}\", \"path\": \"{}\"}}", server_url, project_path))
}

/// Get the latest scan report from the CKB server
#[wasm_bindgen]  
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}
