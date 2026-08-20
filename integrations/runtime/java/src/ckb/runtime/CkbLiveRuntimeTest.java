package ckb.runtime;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

public final class CkbLiveRuntimeTest {
    private CkbLiveRuntimeTest() {}

    public static void main(String[] args) throws Exception {
        traceparentRoundTrips();
        routeTemplateDropsOriginQueryAndFragment();
        privacyFilterDropsSensitiveAttributes();
        collectorExportsObservedOtlpWithoutSecrets();
        failedExporterKeepsPendingBatch();
        System.out.println("CKB Java runtime collector tests passed");
    }

    private static void traceparentRoundTrips() {
        CkbLiveRuntime.TraceContext root = CkbLiveRuntime.TraceContext.root();
        CkbLiveRuntime.TraceContext parsed = CkbLiveRuntime.TraceContext.parse(root.traceparent());
        require(parsed != null, "traceparent should parse");
        require(parsed.traceId().equals(root.traceId()), "trace id should round trip");
        require(parsed.spanId().equals(root.spanId()), "span id should round trip");
        CkbLiveRuntime.TraceContext child = root.child();
        require(child.traceId().equals(root.traceId()), "child should retain trace id");
        require(!child.spanId().equals(root.spanId()), "child should receive a new span id");
        require(CkbLiveRuntime.TraceContext.parse("00-00000000000000000000000000000000-0000000000000000-01") == null, "all-zero identities must be rejected");
    }

    private static void routeTemplateDropsOriginQueryAndFragment() {
        require(
            CkbLiveRuntime.safeRouteTemplate("https://example.com/orders/:id?token=private#top").equals("/orders/:id"),
            "raw origin/query/fragment must not be retained"
        );
        require(CkbLiveRuntime.safeRouteTemplate("orders/:id?debug=true").equals("/orders/:id"), "relative route should normalize");
    }

    private static void privacyFilterDropsSensitiveAttributes() {
        Map<String, String> raw = new LinkedHashMap<>();
        raw.put("db.system", "postgresql");
        raw.put("authorization", "Bearer secret");
        raw.put("user.session.token", "hidden");
        raw.put("request.body", "private");
        Map<String, String> safe = CkbLiveRuntime.sanitizeAttributes(raw);
        require("postgresql".equals(safe.get("db.system")), "safe metadata should remain");
        require(!safe.containsKey("authorization"), "authorization must be removed");
        require(!safe.containsKey("user.session.token"), "tokens must be removed");
        require(!safe.containsKey("request.body"), "request body must be removed");
    }

    private static void collectorExportsObservedOtlpWithoutSecrets() throws Exception {
        List<String> payloads = new ArrayList<>();
        CkbLiveRuntime.Exporter exporter = payloads::add;
        CkbLiveRuntime.Collector collector = new CkbLiveRuntime.Collector("checkout-api", exporter, 1);
        CkbLiveRuntime.TraceContext root = CkbLiveRuntime.TraceContext.root();
        CkbLiveRuntime.ActiveSpan span = collector.startHttpClient("POST", "https://pay.example/charge/:id?token=nope", root);
        collector.finish(span, false);
        require(payloads.size() == 1, "batch should export at configured threshold");
        String payload = payloads.get(0);
        require(payload.contains("checkout-api"), "service identity should be exported");
        require(payload.contains("/charge/:id"), "stable route template should be exported");
        require(!payload.contains("token=nope"), "raw query data must not be exported");
        require(!payload.toLowerCase().contains("authorization"), "authorization field must not be exported");
        require(collector.pendingCount() == 0, "successful export should clear pending batch");
    }

    private static void failedExporterKeepsPendingBatch() {
        CkbLiveRuntime.Exporter exporter = payload -> { throw new IllegalStateException("blocked"); };
        CkbLiveRuntime.Collector collector = new CkbLiveRuntime.Collector("service", exporter, 1);
        CkbLiveRuntime.ActiveSpan span = collector.start("work", null, CkbLiveRuntime.FlowType.FUNCTION, Map.of());
        boolean failed = false;
        try {
            collector.finish(span, true);
        } catch (Exception expected) {
            failed = true;
        }
        require(failed, "export failure should surface");
        require(collector.pendingCount() == 1, "failed export must retain observations for retry");
    }

    private static void require(boolean value, String message) {
        if (!value) throw new AssertionError(message);
    }
}
