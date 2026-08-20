package ckblive

import (
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func TestTraceparentValidation(t *testing.T) {
	traceID := "0123456789abcdef0123456789abcdef"
	spanID := "0123456789abcdef"
	parsedTrace, parsedSpan, ok := parseTraceparent("00-" + traceID + "-" + spanID + "-01")
	if !ok || parsedTrace != traceID || parsedSpan != spanID {
		t.Fatalf("expected valid traceparent, got %q %q %v", parsedTrace, parsedSpan, ok)
	}
	if _, _, ok := parseTraceparent("00-bad-bad-01"); ok {
		t.Fatal("malformed traceparent must be rejected")
	}
	if _, _, ok := parseTraceparent("00-00000000000000000000000000000000-0000000000000000-01"); ok {
		t.Fatal("all-zero trace/span identities must be rejected")
	}
}

func TestSafeURLRemovesSensitiveComponents(t *testing.T) {
	got := SafeURL("https://user:secret@example.com/orders/42?token=private#fragment")
	if got != "https://example.com/orders/42" {
		t.Fatalf("unexpected safe URL: %s", got)
	}
}

func TestSafeAttributesFilterSensitiveNames(t *testing.T) {
	attrs := safeAttributes(Metadata{
		File:      "internal/payment/service.go",
		Function:  "authorizePayment",
		Namespace: "payment",
		Attributes: map[string]string{
			"business.operation":   "authorize",
			"authorization.header": "Bearer private",
			"request.body":         "private payload",
			"sql.text":             "select * from cards",
			"password":             "secret",
			"session.token":        "private-token",
		},
	})
	encoded := make([]string, 0, len(attrs))
	for _, item := range attrs {
		encoded = append(encoded, strings.ToLower(item.Key+"="+item.Value.StringValue))
	}
	joined := strings.Join(encoded, "\n")
	for _, forbidden := range []string{"bearer private", "private payload", "select * from cards", "password=secret", "private-token"} {
		if strings.Contains(joined, forbidden) {
			t.Fatalf("sensitive telemetry leaked: %q", forbidden)
		}
	}
	if !strings.Contains(joined, "business.operation=authorize") {
		t.Fatal("expected safe operational metadata to remain")
	}
}

func TestMiddlewareUsesStableRouteLabelNotRawPath(t *testing.T) {
	client := &Client{
		endpoint:    "https://ckb.invalid/otlp",
		key:         "test-key",
		serviceName: "orders",
		maxBatch:    96,
		queue:       []spanRecord{},
	}
	handler := client.Middleware("GET /orders/{id}", Metadata{
		File:      "internal/http/orders.go",
		Function:  "getOrder",
		Namespace: "orders",
	}, http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusOK)
	}))

	request := httptest.NewRequest(http.MethodGet, "https://example.com/orders/customer-123?token=private", nil)
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)

	if len(client.queue) != 1 {
		t.Fatalf("expected one observed span, got %d", len(client.queue))
	}
	var text []string
	for _, item := range client.queue[0].Attributes {
		text = append(text, item.Key+"="+item.Value.StringValue)
	}
	joined := strings.Join(text, "\n")
	if !strings.Contains(joined, "http.route=GET /orders/{id}") {
		t.Fatalf("stable route label missing: %s", joined)
	}
	if strings.Contains(joined, "customer-123") || strings.Contains(joined, "token=private") {
		t.Fatalf("raw request path/query leaked into telemetry: %s", joined)
	}
}

func TestTraceparentCreationRequiresExactLengths(t *testing.T) {
	if got := traceparent("short", "short"); got != "" {
		t.Fatalf("invalid ids must not produce traceparent: %s", got)
	}
	got := traceparent("0123456789abcdef0123456789abcdef", "0123456789abcdef")
	if got != "00-0123456789abcdef0123456789abcdef-0123456789abcdef-01" {
		t.Fatalf("unexpected traceparent: %s", got)
	}
}
