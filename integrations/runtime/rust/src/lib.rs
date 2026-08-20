//! First-party evidence-first CKB Live Reality collector for Rust.
//!
//! The collector intentionally owns instrumentation semantics, trace-context
//! propagation, privacy filtering and batching, but not transport credentials.
//! Applications provide an `Exporter` that forwards the generated OTLP/JSON
//! payload through their trusted server boundary. Long-lived CKB secrets do not
//! belong in reusable application libraries or client-distributed binaries.

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static ID_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceContext {
    pub trace_id: String,
    pub span_id: String,
    pub trace_flags: u8,
}

impl TraceContext {
    pub fn new_root() -> Self {
        Self {
            trace_id: generated_hex(32, "trace"),
            span_id: generated_hex(16, "span"),
            trace_flags: 1,
        }
    }

    pub fn child(&self) -> Self {
        Self {
            trace_id: self.trace_id.clone(),
            span_id: generated_hex(16, "span"),
            trace_flags: self.trace_flags,
        }
    }

    pub fn traceparent(&self) -> String {
        format!(
            "00-{}-{}-{:02x}",
            self.trace_id, self.span_id, self.trace_flags
        )
    }

    pub fn parse_traceparent(value: &str) -> Option<Self> {
        let mut parts = value.trim().split('-');
        let version = parts.next()?;
        let trace_id = parts.next()?;
        let span_id = parts.next()?;
        let flags = parts.next()?;
        if parts.next().is_some()
            || version.len() != 2
            || trace_id.len() != 32
            || span_id.len() != 16
            || flags.len() != 2
            || !is_lower_hex(version)
            || !is_lower_hex(trace_id)
            || !is_lower_hex(span_id)
            || !is_lower_hex(flags)
            || trace_id.chars().all(|ch| ch == '0')
            || span_id.chars().all(|ch| ch == '0')
        {
            return None;
        }
        let trace_flags = u8::from_str_radix(flags, 16).ok()?;
        Some(Self {
            trace_id: trace_id.to_string(),
            span_id: span_id.to_string(),
            trace_flags,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowType {
    Function,
    HttpServer,
    HttpClient,
    Database,
    Cache,
    Queue,
    Event,
    Websocket,
}

impl FlowType {
    fn as_str(self) -> &'static str {
        match self {
            Self::Function => "function",
            Self::HttpServer => "http-server",
            Self::HttpClient => "http-client",
            Self::Database => "database",
            Self::Cache => "cache",
            Self::Queue => "queue",
            Self::Event => "event",
            Self::Websocket => "websocket",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpanRecord {
    pub name: String,
    pub context: TraceContext,
    pub parent_span_id: String,
    pub start_unix_nano: u64,
    pub end_unix_nano: u64,
    pub error: bool,
    pub attributes: BTreeMap<String, String>,
}

impl SpanRecord {
    pub fn duration_ms(&self) -> f64 {
        self.end_unix_nano.saturating_sub(self.start_unix_nano) as f64 / 1_000_000.0
    }
}

#[derive(Debug, Clone)]
pub struct ActiveSpan {
    name: String,
    context: TraceContext,
    parent_span_id: String,
    start_unix_nano: u64,
    attributes: BTreeMap<String, String>,
}

impl ActiveSpan {
    pub fn context(&self) -> &TraceContext {
        &self.context
    }
}

pub trait Exporter {
    /// Export one bounded OTLP/JSON payload through the application's trusted
    /// transport boundary. Returning an error leaves the batch buffered so the
    /// caller may retry rather than silently dropping observations.
    fn export(&mut self, payload: &str) -> Result<(), String>;
}

pub struct RuntimeCollector<E: Exporter> {
    service_name: String,
    exporter: E,
    pending: Vec<SpanRecord>,
    max_batch: usize,
}

impl<E: Exporter> RuntimeCollector<E> {
    pub fn new(service_name: impl Into<String>, exporter: E) -> Self {
        Self::with_batch_limit(service_name, exporter, 32)
    }

    pub fn with_batch_limit(
        service_name: impl Into<String>,
        exporter: E,
        max_batch: usize,
    ) -> Self {
        Self {
            service_name: service_name.into().trim().chars().take(120).collect(),
            exporter,
            pending: Vec::new(),
            max_batch: max_batch.clamp(1, 256),
        }
    }

    pub fn root_context(&self) -> TraceContext {
        TraceContext::new_root()
    }

    pub fn start_span(
        &self,
        name: impl Into<String>,
        parent: Option<&TraceContext>,
        flow_type: FlowType,
        attributes: BTreeMap<String, String>,
    ) -> ActiveSpan {
        let context = parent
            .map(TraceContext::child)
            .unwrap_or_else(TraceContext::new_root);
        let parent_span_id = parent
            .map(|value| value.span_id.clone())
            .unwrap_or_default();
        let mut safe = sanitize_attributes(attributes);
        safe.insert("ckb.flow.type".into(), flow_type.as_str().into());
        ActiveSpan {
            name: name.into().chars().take(180).collect(),
            context,
            parent_span_id,
            start_unix_nano: unix_nanos(),
            attributes: safe,
        }
    }

    /// Convenience helper for an outbound HTTP boundary. `route_template` must
    /// be a stable application route such as `/orders/:id`; raw query strings,
    /// authorization headers and request/response bodies are intentionally not
    /// accepted by this API.
    pub fn start_http_client(
        &self,
        method: &str,
        route_template: &str,
        parent: Option<&TraceContext>,
    ) -> ActiveSpan {
        let mut attributes = BTreeMap::new();
        attributes.insert("http.request.method".into(), bounded(method, 16));
        attributes.insert("http.route".into(), safe_route_template(route_template));
        self.start_span(
            format!(
                "{} {}",
                bounded(method, 16),
                safe_route_template(route_template)
            ),
            parent,
            FlowType::HttpClient,
            attributes,
        )
    }

    pub fn finish_span(&mut self, span: ActiveSpan, error: bool) -> Result<(), String> {
        self.pending.push(SpanRecord {
            name: span.name,
            context: span.context,
            parent_span_id: span.parent_span_id,
            start_unix_nano: span.start_unix_nano,
            end_unix_nano: unix_nanos().max(span.start_unix_nano),
            error,
            attributes: span.attributes,
        });
        if self.pending.len() >= self.max_batch {
            self.flush()?;
        }
        Ok(())
    }

    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    pub fn flush(&mut self) -> Result<(), String> {
        if self.pending.is_empty() {
            return Ok(());
        }
        let payload = otlp_json(&self.service_name, &self.pending);
        self.exporter.export(&payload)?;
        self.pending.clear();
        Ok(())
    }

    pub fn exporter(&self) -> &E {
        &self.exporter
    }

    pub fn exporter_mut(&mut self) -> &mut E {
        &mut self.exporter
    }
}

pub fn sanitize_attributes(attributes: BTreeMap<String, String>) -> BTreeMap<String, String> {
    attributes
        .into_iter()
        .filter_map(|(key, value)| {
            let normalized = key.to_ascii_lowercase().replace(['-', '.'], "_");
            let forbidden = [
                "password",
                "passwd",
                "secret",
                "token",
                "authorization",
                "cookie",
                "session",
                "api_key",
                "apikey",
                "request_body",
                "response_body",
                "payload",
            ];
            if forbidden.iter().any(|needle| normalized.contains(needle)) {
                None
            } else {
                Some((bounded(&key, 120), bounded(&value, 512)))
            }
        })
        .collect()
}

pub fn safe_route_template(value: &str) -> String {
    let raw = value.trim();
    let without_query = raw.split(['?', '#']).next().unwrap_or("/");
    let path = if let Some(scheme_index) = without_query.find("://") {
        let after_scheme = &without_query[scheme_index + 3..];
        match after_scheme.find('/') {
            Some(index) => &after_scheme[index..],
            None => "/",
        }
    } else {
        without_query
    };
    let normalized = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    bounded(&normalized, 240)
}

fn otlp_json(service_name: &str, spans: &[SpanRecord]) -> String {
    let rendered_spans = spans.iter().map(render_span).collect::<Vec<_>>().join(",");
    format!(
        "{{\"resourceSpans\":[{{\"resource\":{{\"attributes\":[{{\"key\":\"service.name\",\"value\":{{\"stringValue\":\"{}\"}}}}]}},\"scopeSpans\":[{{\"scope\":{{\"name\":\"ckb-live-rust\"}},\"spans\":[{}]}}]}}]}}",
        json_escape(service_name),
        rendered_spans,
    )
}

fn render_span(span: &SpanRecord) -> String {
    let attributes = span
        .attributes
        .iter()
        .map(|(key, value)| {
            format!(
                "{{\"key\":\"{}\",\"value\":{{\"stringValue\":\"{}\"}}}}",
                json_escape(key),
                json_escape(value),
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let status = if span.error { 2 } else { 1 };
    format!(
        "{{\"traceId\":\"{}\",\"spanId\":\"{}\",\"parentSpanId\":\"{}\",\"name\":\"{}\",\"startTimeUnixNano\":\"{}\",\"endTimeUnixNano\":\"{}\",\"attributes\":[{}],\"status\":{{\"code\":{}}}}}",
        span.context.trace_id,
        span.context.span_id,
        span.parent_span_id,
        json_escape(&span.name),
        span.start_unix_nano,
        span.end_unix_nano,
        attributes,
        status,
    )
}

fn json_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out
}

fn bounded(value: &str, max: usize) -> String {
    value.trim().chars().take(max).collect()
}

fn unix_nanos() -> u64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    nanos.min(u64::MAX as u128) as u64
}

fn generated_hex(width: usize, domain: &str) -> String {
    let counter = ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    let now = unix_nanos();
    let pid = std::process::id();
    let mut out = String::new();
    let mut salt = 0u64;
    while out.len() < width {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        domain.hash(&mut hasher);
        now.hash(&mut hasher);
        counter.hash(&mut hasher);
        pid.hash(&mut hasher);
        salt.hash(&mut hasher);
        out.push_str(&format!("{:016x}", hasher.finish()));
        salt = salt.wrapping_add(1);
    }
    out.truncate(width);
    if out.chars().all(|ch| ch == '0') {
        out.replace_range(width - 1.., "1");
    }
    out
}

fn is_lower_hex(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct MemoryExporter {
        payloads: Vec<String>,
        fail: bool,
    }

    impl Exporter for MemoryExporter {
        fn export(&mut self, payload: &str) -> Result<(), String> {
            if self.fail {
                return Err("blocked".into());
            }
            self.payloads.push(payload.to_string());
            Ok(())
        }
    }

    #[test]
    fn traceparent_round_trips_and_child_keeps_trace() {
        let root = TraceContext::new_root();
        let parsed = TraceContext::parse_traceparent(&root.traceparent()).unwrap();
        assert_eq!(parsed, root);
        let child = root.child();
        assert_eq!(child.trace_id, root.trace_id);
        assert_ne!(child.span_id, root.span_id);
    }

    #[test]
    fn invalid_traceparents_are_rejected() {
        assert!(TraceContext::parse_traceparent(
            "00-00000000000000000000000000000000-0000000000000000-01"
        )
        .is_none());
        assert!(TraceContext::parse_traceparent("00-nothex-1234-01").is_none());
        assert!(TraceContext::parse_traceparent(
            "00-ABCDEFABCDEFABCDEFABCDEFABCDEFAB-abcdefabcdefabcd-01"
        )
        .is_none());
    }

    #[test]
    fn privacy_filter_removes_secret_and_payload_keys() {
        let attributes = BTreeMap::from([
            ("db.system".into(), "postgresql".into()),
            ("authorization".into(), "Bearer hidden".into()),
            ("user.session.token".into(), "hidden".into()),
            ("request.body".into(), "sensitive".into()),
        ]);
        let safe = sanitize_attributes(attributes);
        assert_eq!(
            safe.get("db.system").map(String::as_str),
            Some("postgresql")
        );
        assert!(!safe.contains_key("authorization"));
        assert!(!safe.contains_key("user.session.token"));
        assert!(!safe.contains_key("request.body"));
    }

    #[test]
    fn route_template_drops_origin_query_and_fragment() {
        assert_eq!(
            safe_route_template("https://example.com/orders/:id?token=nope#top"),
            "/orders/:id"
        );
        assert_eq!(safe_route_template("orders/:id?debug=true"), "/orders/:id");
    }

    #[test]
    fn batching_exports_otlp_without_credentials_or_raw_payloads() {
        let exporter = MemoryExporter::default();
        let mut collector = RuntimeCollector::with_batch_limit("checkout-api", exporter, 1);
        let root = collector.root_context();
        let span = collector.start_http_client("POST", "/payments/:id?secret=gone", Some(&root));
        collector.finish_span(span, false).unwrap();
        assert_eq!(collector.pending_len(), 0);
        assert_eq!(collector.exporter().payloads.len(), 1);
        let payload = &collector.exporter().payloads[0];
        assert!(payload.contains("checkout-api"));
        assert!(payload.contains("/payments/:id"));
        assert!(!payload.contains("secret=gone"));
        assert!(!payload.contains("authorization"));
    }

    #[test]
    fn failed_export_keeps_batch_for_retry() {
        let exporter = MemoryExporter {
            payloads: vec![],
            fail: true,
        };
        let mut collector = RuntimeCollector::with_batch_limit("svc", exporter, 1);
        let span = collector.start_span("work", None, FlowType::Function, BTreeMap::new());
        assert!(collector.finish_span(span, true).is_err());
        assert_eq!(collector.pending_len(), 1);
        collector.exporter_mut().fail = false;
        collector.flush().unwrap();
        assert_eq!(collector.pending_len(), 0);
    }
}
