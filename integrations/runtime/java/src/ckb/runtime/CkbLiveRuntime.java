package ckb.runtime;

import java.nio.charset.StandardCharsets;
import java.security.SecureRandom;
import java.time.Instant;
import java.util.ArrayList;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Objects;
import java.util.Set;

/** Evidence-first, dependency-free CKB runtime collector for JVM applications. */
public final class CkbLiveRuntime {
    private static final SecureRandom RANDOM = new SecureRandom();
    private static final Set<String> FORBIDDEN = Set.of(
        "password", "passwd", "secret", "token", "authorization", "cookie", "session",
        "api_key", "apikey", "request_body", "response_body", "payload"
    );

    private CkbLiveRuntime() {}

    public enum FlowType {
        FUNCTION("function"), HTTP_SERVER("http-server"), HTTP_CLIENT("http-client"),
        DATABASE("database"), CACHE("cache"), QUEUE("queue"), EVENT("event"), WEBSOCKET("websocket");

        private final String value;
        FlowType(String value) { this.value = value; }
        public String value() { return value; }
    }

    public interface Exporter {
        void export(String otlpJson) throws Exception;
    }

    public record TraceContext(String traceId, String spanId, int traceFlags) {
        public TraceContext {
            if (!validHex(traceId, 32) || !validHex(spanId, 16) || traceId.chars().allMatch(ch -> ch == '0') || spanId.chars().allMatch(ch -> ch == '0')) {
                throw new IllegalArgumentException("invalid W3C trace identity");
            }
        }

        public static TraceContext root() {
            return new TraceContext(randomHex(16), randomHex(8), 1);
        }

        public TraceContext child() {
            return new TraceContext(traceId, randomHex(8), traceFlags);
        }

        public String traceparent() {
            return "00-" + traceId + "-" + spanId + "-" + String.format("%02x", traceFlags & 0xff);
        }

        public static TraceContext parse(String value) {
            if (value == null) return null;
            String[] parts = value.trim().split("-", -1);
            if (parts.length != 4 || !validHex(parts[0], 2) || !validHex(parts[1], 32) || !validHex(parts[2], 16) || !validHex(parts[3], 2)) return null;
            if (parts[1].chars().allMatch(ch -> ch == '0') || parts[2].chars().allMatch(ch -> ch == '0')) return null;
            try {
                return new TraceContext(parts[1], parts[2], Integer.parseInt(parts[3], 16));
            } catch (RuntimeException ignored) {
                return null;
            }
        }
    }

    public static final class ActiveSpan {
        private final String name;
        private final TraceContext context;
        private final String parentSpanId;
        private final long startUnixNano;
        private final Map<String, String> attributes;

        private ActiveSpan(String name, TraceContext context, String parentSpanId, long startUnixNano, Map<String, String> attributes) {
            this.name = name;
            this.context = context;
            this.parentSpanId = parentSpanId;
            this.startUnixNano = startUnixNano;
            this.attributes = attributes;
        }

        public TraceContext context() { return context; }
    }

    public record SpanRecord(
        String name,
        TraceContext context,
        String parentSpanId,
        long startUnixNano,
        long endUnixNano,
        boolean error,
        Map<String, String> attributes
    ) {}

    public static final class Collector {
        private final String serviceName;
        private final Exporter exporter;
        private final int maxBatch;
        private final List<SpanRecord> pending = new ArrayList<>();

        public Collector(String serviceName, Exporter exporter) {
            this(serviceName, exporter, 32);
        }

        public Collector(String serviceName, Exporter exporter, int maxBatch) {
            this.serviceName = bounded(serviceName, 120);
            this.exporter = Objects.requireNonNull(exporter, "exporter");
            this.maxBatch = Math.max(1, Math.min(maxBatch, 256));
        }

        public ActiveSpan start(String name, TraceContext parent, FlowType flowType, Map<String, String> attributes) {
            TraceContext context = parent == null ? TraceContext.root() : parent.child();
            Map<String, String> safe = sanitizeAttributes(attributes == null ? Map.of() : attributes);
            safe.put("ckb.flow.type", flowType.value());
            return new ActiveSpan(
                bounded(name, 180),
                context,
                parent == null ? "" : parent.spanId(),
                unixNanos(),
                safe
            );
        }

        public ActiveSpan startHttpClient(String method, String routeTemplate, TraceContext parent) {
            String route = safeRouteTemplate(routeTemplate);
            Map<String, String> attributes = new LinkedHashMap<>();
            attributes.put("http.request.method", bounded(method, 16));
            attributes.put("http.route", route);
            return start(bounded(method, 16) + " " + route, parent, FlowType.HTTP_CLIENT, attributes);
        }

        public void finish(ActiveSpan span, boolean error) throws Exception {
            pending.add(new SpanRecord(
                span.name,
                span.context,
                span.parentSpanId,
                span.startUnixNano,
                Math.max(unixNanos(), span.startUnixNano),
                error,
                Collections.unmodifiableMap(new LinkedHashMap<>(span.attributes))
            ));
            if (pending.size() >= maxBatch) flush();
        }

        public int pendingCount() { return pending.size(); }

        public void flush() throws Exception {
            if (pending.isEmpty()) return;
            String payload = otlpJson(serviceName, pending);
            exporter.export(payload);
            pending.clear();
        }
    }

    public static Map<String, String> sanitizeAttributes(Map<String, String> attributes) {
        Map<String, String> safe = new LinkedHashMap<>();
        for (Map.Entry<String, String> entry : attributes.entrySet()) {
            String key = bounded(entry.getKey(), 120);
            String normalized = key.toLowerCase(Locale.ROOT).replace('-', '_').replace('.', '_');
            boolean forbidden = FORBIDDEN.stream().anyMatch(normalized::contains);
            if (!forbidden) safe.put(key, bounded(entry.getValue(), 512));
        }
        return safe;
    }

    public static String safeRouteTemplate(String value) {
        String raw = value == null ? "" : value.trim();
        int query = raw.indexOf('?');
        int fragment = raw.indexOf('#');
        int cut = raw.length();
        if (query >= 0) cut = Math.min(cut, query);
        if (fragment >= 0) cut = Math.min(cut, fragment);
        String path = raw.substring(0, cut);
        int scheme = path.indexOf("://");
        if (scheme >= 0) {
            String after = path.substring(scheme + 3);
            int slash = after.indexOf('/');
            path = slash >= 0 ? after.substring(slash) : "/";
        }
        if (path.isBlank()) path = "/";
        if (!path.startsWith("/")) path = "/" + path;
        return bounded(path, 240);
    }

    private static String otlpJson(String serviceName, List<SpanRecord> spans) {
        StringBuilder out = new StringBuilder();
        out.append("{\"resourceSpans\":[{\"resource\":{\"attributes\":[{\"key\":\"service.name\",\"value\":{\"stringValue\":\"")
            .append(jsonEscape(serviceName)).append("\"}}]},\"scopeSpans\":[{\"scope\":{\"name\":\"ckb-live-java\"},\"spans\":[");
        for (int i = 0; i < spans.size(); i++) {
            if (i > 0) out.append(',');
            SpanRecord span = spans.get(i);
            out.append("{\"traceId\":\"").append(span.context.traceId())
                .append("\",\"spanId\":\"").append(span.context.spanId())
                .append("\",\"parentSpanId\":\"").append(jsonEscape(span.parentSpanId))
                .append("\",\"name\":\"").append(jsonEscape(span.name))
                .append("\",\"startTimeUnixNano\":\"").append(span.startUnixNano)
                .append("\",\"endTimeUnixNano\":\"").append(span.endUnixNano)
                .append("\",\"attributes\":[");
            int attributeIndex = 0;
            for (Map.Entry<String, String> entry : span.attributes.entrySet()) {
                if (attributeIndex++ > 0) out.append(',');
                out.append("{\"key\":\"").append(jsonEscape(entry.getKey()))
                    .append("\",\"value\":{\"stringValue\":\"").append(jsonEscape(entry.getValue())).append("\"}}");
            }
            out.append("],\"status\":{\"code\":").append(span.error ? 2 : 1).append("}}");
        }
        return out.append("]}]}]}" ).toString();
    }

    private static String randomHex(int bytes) {
        byte[] value = new byte[bytes];
        RANDOM.nextBytes(value);
        StringBuilder out = new StringBuilder(bytes * 2);
        for (byte item : value) out.append(String.format("%02x", item & 0xff));
        return out.toString();
    }

    private static boolean validHex(String value, int length) {
        if (value == null || value.length() != length) return false;
        for (int i = 0; i < value.length(); i++) {
            char ch = value.charAt(i);
            if (!((ch >= '0' && ch <= '9') || (ch >= 'a' && ch <= 'f'))) return false;
        }
        return true;
    }

    private static String bounded(String value, int max) {
        String text = value == null ? "" : value.trim();
        return text.substring(0, Math.min(text.length(), max));
    }

    private static long unixNanos() {
        Instant now = Instant.now();
        long seconds = now.getEpochSecond();
        long nanos = now.getNano();
        if (seconds > (Long.MAX_VALUE - nanos) / 1_000_000_000L) return Long.MAX_VALUE;
        return seconds * 1_000_000_000L + nanos;
    }

    private static String jsonEscape(String value) {
        StringBuilder out = new StringBuilder();
        for (byte raw : value.getBytes(StandardCharsets.UTF_8)) {
            int ch = raw & 0xff;
            switch (ch) {
                case '\\' -> out.append("\\\\");
                case '"' -> out.append("\\\"");
                case '\n' -> out.append("\\n");
                case '\r' -> out.append("\\r");
                case '\t' -> out.append("\\t");
                default -> {
                    if (ch < 0x20) out.append(String.format("\\u%04x", ch));
                    else out.append((char) ch);
                }
            }
        }
        return out.toString();
    }
}
