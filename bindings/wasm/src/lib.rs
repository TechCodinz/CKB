use wasm_bindgen::prelude::*;

/// Scan a codebase from WASM (browser/Node.js context).
///
/// This is intentionally an error, not a stub success: an in-browser
/// scan is impossible today. `ckb-core` cannot run under
/// `wasm32-unknown-unknown` — its tree-sitter grammars are native C
/// build scripts, its storage is a sled database on the local
/// filesystem, and its analysis runs on rayon threads; a browser has
/// none of those. (`cargo check -p ckb-core --target
/// wasm32-unknown-unknown` fails in tree-sitter's build script.)
///
/// The previous implementation returned a hardcoded
/// `{"status": "scan_delegated", ...}` success payload without
/// contacting any server — fabricated output presented as a scan
/// result. It now fails honestly and points at the real path: run the
/// `ckb-mcp-server` binary and call its REST API
/// (`POST /api/v1/scan`), which is what the Node and Python bindings
/// in this repo do.
#[wasm_bindgen]
pub async fn scan(server_url: &str, project_path: &str) -> Result<String, JsValue> {
    let _ = (server_url, project_path);
    Err(JsValue::from_str(
        "ckb-wasm: in-browser scanning is not supported — ckb-core requires a \
         filesystem (sled storage, source files) and native threads (rayon) that \
         wasm32 in a browser does not provide. Run ckb-mcp-server and call \
         POST /api/v1/scan on it instead (see the Node/Python bindings).",
    ))
}

/// The ckb-wasm crate version.
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}
