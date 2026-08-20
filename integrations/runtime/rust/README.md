# CKB Live Reality — Rust collector

This is the first-party Rust runtime collector for CKB's Live Execution Twin / Software MRI.

## Evidence contract

The library emits **observed runtime spans** only. It does not claim source lines executed unless a separate profiler/coverage/runtime-probe feed supplies explicit line evidence, and it does not turn process/network observations into source execution facts.

It provides:

- W3C `traceparent` creation and parsing;
- parent/child trace propagation;
- typed function, HTTP, database, cache, queue, event and WebSocket flow labels;
- bounded in-memory batching;
- privacy-filtered custom attributes;
- stable HTTP route-template instrumentation;
- OTLP/JSON payload generation compatible with the CKB telemetry boundary;
- retry-safe buffering when an exporter fails.

## Transport and credentials

The crate intentionally has **no networking dependency and no CKB credential field**. Your application supplies an `Exporter` and sends the generated OTLP/JSON payload through its trusted server-side telemetry path.

That separation is deliberate: long-lived CKB project credentials should not be compiled into reusable libraries, desktop/mobile client binaries, or browser code.

```rust
use ckb_live_runtime::{Exporter, FlowType, RuntimeCollector};
use std::collections::BTreeMap;

struct MyTrustedExporter;

impl Exporter for MyTrustedExporter {
    fn export(&mut self, payload: &str) -> Result<(), String> {
        // Forward `payload` through your server-side CKB telemetry transport.
        // Add credentials at that trusted boundary, not inside the collector.
        send_to_internal_telemetry_gateway(payload)
    }
}

fn send_to_internal_telemetry_gateway(_payload: &str) -> Result<(), String> {
    Ok(())
}

let mut ckb = RuntimeCollector::new("checkout-api", MyTrustedExporter);
let request = ckb.start_span("checkout", None, FlowType::Function, BTreeMap::new());

let db = ckb.start_span(
    "load-cart",
    Some(request.context()),
    FlowType::Database,
    BTreeMap::from([("db.system".into(), "postgresql".into())]),
);
ckb.finish_span(db, false)?;
ckb.finish_span(request, false)?;
ckb.flush()?;
# Ok::<(), String>(())
```

## HTTP boundaries

Use stable route templates, not raw customer-specific URLs:

```rust
let call = ckb.start_http_client("POST", "/payments/:id", Some(request.context()));
```

The helper strips origins, query strings and fragments. It does not accept request/response bodies, cookies, authorization headers or URL query data.

## Privacy filtering

Custom attributes whose keys indicate passwords, secrets, tokens, authorization, cookies, sessions, API keys, request/response bodies or payloads are removed before buffering/export.

Attribute values and names are bounded. This is defense in depth, not permission to pass arbitrary sensitive application state into telemetry.

## Verification

Run:

```bash
cargo test --manifest-path integrations/runtime/rust/Cargo.toml
cargo fmt --manifest-path integrations/runtime/rust/Cargo.toml -- --check
```

The repository Live Execution Twin CI gate runs the Rust collector tests and formatting checks alongside the Core runtime diagnostics, browser collector and Go agent.
