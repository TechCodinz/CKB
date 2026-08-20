package ckblive

import (
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
			"business.operation": "authorize",
			"authorization.header": "Bearer private",
			"request.body": "private payload",
			"sql.text": "select * from cards",
			"password": "secret",
		},
	})
	encoded := make([]string, 0, len(attrs))
	for _, item := range attrs {
		encoded = append(encoded, strings.ToLower(item.Key+"="+item.Value.StringValue))
	}
	joined := strings.Join(encoded, "\n")
	for _, forbidden := range []string{"bearer private", "private payload", "select * from cards", "password=secret"} {
		if strings.Contains(joined, forbidden) {
			t.Fatalf("sensitive telemetry leaked: %q", forbidden)
		}
	}
	if !strings.Contains(joined, "business.operation=authorize") {
		t.Fatal("expected safe operational metadata to remain")
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
