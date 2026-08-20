// Package ckblive provides a zero-dependency Go runtime agent for CKB Live Reality.
//
// Truth/privacy contract:
//   - spans are emitted only for code that actually executes;
//   - request/response bodies, cookies, authorization values, SQL text, database
//     values and arbitrary application objects are never copied into telemetry;
//   - exact source identity is emitted only when callers supply a repository path;
//   - W3C traceparent is propagated for distributed trace continuity, but the
//     application header set is never exported to CKB.
package ckblive

import (
	"bytes"
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/url"
	"os"
	"strings"
	"sync"
	"time"
)

type contextKey struct{}

type traceContext struct {
	TraceID string
	SpanID  string
}

// Metadata identifies one runtime boundary without carrying business payloads.
type Metadata struct {
	File            string
	Function        string
	Namespace       string
	Kind            string
	FlowType        string
	FlowDirection   string
	Protocol        string
	DBSystem        string
	MessagingSystem string
	Attributes      map[string]string
}

// Config controls batching and transport. Empty Endpoint/Key values fall back
// to CKB_OTLP_ENDPOINT and CKB_OTLP_KEY.
type Config struct {
	Endpoint      string
	Key           string
	ServiceName   string
	Environment   string
	FlushInterval time.Duration
	MaxBatch      int
	HTTPClient    *http.Client
}

type attribute struct {
	Key   string         `json:"key"`
	Value attributeValue `json:"value"`
}

type attributeValue struct {
	StringValue string `json:"stringValue,omitempty"`
	BoolValue   *bool  `json:"boolValue,omitempty"`
}

type spanStatus struct {
	Code int `json:"code"`
}

type spanRecord struct {
	TraceID           string      `json:"traceId"`
	SpanID            string      `json:"spanId"`
	ParentSpanID      string      `json:"parentSpanId,omitempty"`
	Name              string      `json:"name"`
	StartTimeUnixNano string      `json:"startTimeUnixNano"`
	EndTimeUnixNano   string      `json:"endTimeUnixNano"`
	Attributes        []attribute `json:"attributes"`
	Status            spanStatus  `json:"status"`
}

type otlpEnvelope struct {
	ResourceSpans []struct {
		Resource struct {
			Attributes []attribute `json:"attributes"`
		} `json:"resource"`
		ScopeSpans []struct {
			Scope struct {
				Name    string `json:"name"`
				Version string `json:"version"`
			} `json:"scope"`
			Spans []spanRecord `json:"spans"`
		} `json:"scopeSpans"`
	} `json:"resourceSpans"`
}

// Client batches observed spans and sends standard OTLP/HTTP JSON to CKB.
type Client struct {
	endpoint      string
	key           string
	serviceName   string
	environment   string
	flushInterval time.Duration
	maxBatch      int
	httpClient    *http.Client

	mu      sync.Mutex
	queue   []spanRecord
	closed  bool
	wake    chan struct{}
	stop    chan struct{}
	stopped chan struct{}
}

func New(config Config) *Client {
	endpoint := strings.TrimSpace(config.Endpoint)
	if endpoint == "" {
		endpoint = strings.TrimSpace(os.Getenv("CKB_OTLP_ENDPOINT"))
	}
	key := strings.TrimSpace(config.Key)
	if key == "" {
		key = strings.TrimSpace(os.Getenv("CKB_OTLP_KEY"))
	}
	service := strings.TrimSpace(config.ServiceName)
	if service == "" {
		service = strings.TrimSpace(os.Getenv("CKB_SERVICE_NAME"))
	}
	if service == "" {
		service = "go-service"
	}
	environment := strings.TrimSpace(config.Environment)
	if environment == "" {
		environment = strings.TrimSpace(os.Getenv("CKB_ENVIRONMENT"))
	}
	if environment == "" {
		environment = "unknown"
	}
	flushInterval := config.FlushInterval
	if flushInterval < 10*time.Second {
		flushInterval = 12 * time.Second
	}
	maxBatch := config.MaxBatch
	if maxBatch < 8 {
		maxBatch = 96
	}
	if maxBatch > 256 {
		maxBatch = 256
	}
	httpClient := config.HTTPClient
	if httpClient == nil {
		httpClient = &http.Client{Timeout: 8 * time.Second}
	}
	client := &Client{
		endpoint:      endpoint,
		key:           key,
		serviceName:   service,
		environment:   environment,
		flushInterval: flushInterval,
		maxBatch:      maxBatch,
		httpClient:    httpClient,
		wake:          make(chan struct{}, 1),
		stop:          make(chan struct{}),
		stopped:       make(chan struct{}),
	}
	go client.loop()
	return client
}

func (c *Client) Configured() bool {
	return c != nil && c.endpoint != "" && c.key != ""
}

func randomHex(bytes int) string {
	buf := make([]byte, bytes)
	if _, err := rand.Read(buf); err != nil {
		// A trace identity must not be fabricated from a predictable fallback.
		// Returning empty causes the instrumentation call to execute normally but
		// skip telemetry for that span.
		return ""
	}
	return hex.EncodeToString(buf)
}

func nanoNow() string {
	return fmt.Sprintf("%d", time.Now().UnixNano())
}

func attr(key, value string) attribute {
	return attribute{Key: key, Value: attributeValue{StringValue: value}}
}

func boolAttr(key string, value bool) attribute {
	return attribute{Key: key, Value: attributeValue{BoolValue: &value}}
}

func bounded(value string, max int) string {
	value = strings.TrimSpace(value)
	if len(value) > max {
		return value[:max]
	}
	return value
}

func safeAttributes(metadata Metadata) []attribute {
	flowType := bounded(metadata.FlowType, 80)
	if flowType == "" {
		flowType = bounded(metadata.Kind, 80)
	}
	if flowType == "" {
		flowType = "function"
	}
	values := []attribute{
		attr("code.function.name", bounded(metadata.Function, 240)),
		attr("code.file.path", bounded(strings.ReplaceAll(metadata.File, "\\", "/"), 400)),
		attr("code.namespace", bounded(metadata.Namespace, 240)),
		attr("ckb.symbol.kind", bounded(metadata.Kind, 80)),
		attr("ckb.flow.type", flowType),
		attr("ckb.flow.direction", bounded(metadata.FlowDirection, 40)),
		attr("network.protocol.name", bounded(metadata.Protocol, 40)),
		attr("db.system", bounded(metadata.DBSystem, 80)),
		attr("messaging.system", bounded(metadata.MessagingSystem, 80)),
		boolAttr("ckb.runtime.observed", true),
	}
	for key, value := range metadata.Attributes {
		key = bounded(key, 120)
		value = bounded(value, 300)
		if key == "" || value == "" || strings.Contains(strings.ToLower(key), "authorization") || strings.Contains(strings.ToLower(key), "cookie") || strings.Contains(strings.ToLower(key), "secret") || strings.Contains(strings.ToLower(key), "password") || strings.Contains(strings.ToLower(key), "payload") || strings.Contains(strings.ToLower(key), "body") || strings.Contains(strings.ToLower(key), "sql") {
			continue
		}
		values = append(values, attr(key, value))
		if len(values) >= 64 {
			break
		}
	}
	out := values[:0]
	for _, item := range values {
		if item.Key == "ckb.runtime.observed" || item.Value.BoolValue != nil || item.Value.StringValue != "" {
			out = append(out, item)
		}
	}
	return out
}

func (c *Client) resourceAttributes() []attribute {
	return []attribute{
		attr("service.name", bounded(c.serviceName, 240)),
		attr("deployment.environment", bounded(c.environment, 120)),
		attr("telemetry.sdk.name", "ckb-live-reality"),
		attr("telemetry.sdk.language", "go"),
		attr("ckb.runtime.agent", "go-zero-dependency-v1"),
	}
}

func traceFromContext(ctx context.Context) traceContext {
	if ctx == nil {
		return traceContext{}
	}
	value, _ := ctx.Value(contextKey{}).(traceContext)
	return value
}

func withTrace(ctx context.Context, traceID, spanID string) context.Context {
	return context.WithValue(ctx, contextKey{}, traceContext{TraceID: traceID, SpanID: spanID})
}

func parseTraceparent(value string) (traceID, parentSpanID string, ok bool) {
	parts := strings.Split(strings.TrimSpace(value), "-")
	if len(parts) != 4 || len(parts[1]) != 32 || len(parts[2]) != 16 {
		return "", "", false
	}
	if _, err := hex.DecodeString(parts[1]); err != nil {
		return "", "", false
	}
	if _, err := hex.DecodeString(parts[2]); err != nil {
		return "", "", false
	}
	if strings.Trim(parts[1], "0") == "" || strings.Trim(parts[2], "0") == "" {
		return "", "", false
	}
	return strings.ToLower(parts[1]), strings.ToLower(parts[2]), true
}

func traceparent(traceID, spanID string) string {
	if len(traceID) != 32 || len(spanID) != 16 {
		return ""
	}
	return "00-" + traceID + "-" + spanID + "-01"
}

func (c *Client) enqueue(span spanRecord) {
	if !c.Configured() || span.TraceID == "" || span.SpanID == "" {
		return
	}
	c.mu.Lock()
	if c.closed {
		c.mu.Unlock()
		return
	}
	c.queue = append(c.queue, span)
	shouldWake := len(c.queue) >= c.maxBatch
	c.mu.Unlock()
	if shouldWake {
		select {
		case c.wake <- struct{}{}:
		default:
		}
	}
}

func (c *Client) observedSpan(name string, metadata Metadata, traceID, spanID, parentSpanID, start string, failed bool) spanRecord {
	if metadata.Function == "" {
		metadata.Function = name
	}
	if metadata.Kind == "" {
		metadata.Kind = "function"
	}
	code := 1
	if failed {
		code = 2
	}
	return spanRecord{
		TraceID:           traceID,
		SpanID:            spanID,
		ParentSpanID:      parentSpanID,
		Name:              bounded(name, 240),
		StartTimeUnixNano: start,
		EndTimeUnixNano:   nanoNow(),
		Attributes:        safeAttributes(metadata),
		Status:            spanStatus{Code: code},
	}
}

// Span executes fn and emits one observed span after it finishes. The returned
// context may be used for child work so parent/child identity is exact.
func (c *Client) Span(ctx context.Context, name string, metadata Metadata, fn func(context.Context) error) error {
	if fn == nil {
		return errors.New("CKB Span requires a function")
	}
	if ctx == nil {
		ctx = context.Background()
	}
	parent := traceFromContext(ctx)
	traceID := parent.TraceID
	if traceID == "" {
		traceID = randomHex(16)
	}
	spanID := randomHex(8)
	childCtx := withTrace(ctx, traceID, spanID)
	start := nanoNow()
	err := fn(childCtx)
	c.enqueue(c.observedSpan(name, metadata, traceID, spanID, parent.SpanID, start, err != nil))
	return err
}

// Middleware wraps a real net/http handler. Route templates should be supplied
// by the application as name when available; raw query strings are never sent.
func (c *Client) Middleware(name string, metadata Metadata, next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if next == nil {
			http.Error(w, "handler unavailable", http.StatusInternalServerError)
			return
		}
		ctx := r.Context()
		parent := traceFromContext(ctx)
		if parent.TraceID == "" {
			if traceID, parentSpanID, ok := parseTraceparent(r.Header.Get("traceparent")); ok {
				parent = traceContext{TraceID: traceID, SpanID: parentSpanID}
				ctx = withTrace(ctx, traceID, parentSpanID)
			}
		}
		metadata.FlowType = "http-server"
		metadata.FlowDirection = "inbound"
		metadata.Protocol = r.Proto
		metadata.Attributes = cloneMetadata(metadata.Attributes)
		metadata.Attributes["http.request.method"] = bounded(r.Method, 16)
		// Use only host + path. Query values are intentionally dropped.
		metadata.Attributes["http.route.observed"] = bounded(r.URL.Path, 300)
		_ = c.Span(ctx, name, metadata, func(spanCtx context.Context) error {
			next.ServeHTTP(w, r.WithContext(spanCtx))
			return nil
		})
	})
}

func cloneMetadata(input map[string]string) map[string]string {
	out := make(map[string]string, len(input)+4)
	for key, value := range input {
		out[key] = value
	}
	return out
}

// Transport wraps outbound HTTP and propagates W3C traceparent. It records the
// method and destination host only; body, headers and query values stay private.
type Transport struct {
	Client *Client
	Base   http.RoundTripper
	File   string
}

func (t Transport) RoundTrip(req *http.Request) (*http.Response, error) {
	base := t.Base
	if base == nil {
		base = http.DefaultTransport
	}
	if t.Client == nil || req == nil {
		return base.RoundTrip(req)
	}
	parent := traceFromContext(req.Context())
	traceID := parent.TraceID
	if traceID == "" {
		traceID = randomHex(16)
	}
	spanID := randomHex(8)
	start := nanoNow()
	clone := req.Clone(withTrace(req.Context(), traceID, spanID))
	clone.Header = req.Header.Clone()
	if value := traceparent(traceID, spanID); value != "" {
		clone.Header.Set("traceparent", value)
	}
	response, err := base.RoundTrip(clone)
	metadata := Metadata{
		File:          t.File,
		Function:      "http.RoundTrip",
		Namespace:     t.Client.serviceName,
		Kind:          "http-client",
		FlowType:      "http-client",
		FlowDirection: "outbound",
		Protocol:      clone.URL.Scheme,
		Attributes: map[string]string{
			"http.request.method": bounded(clone.Method, 16),
			"server.address":      bounded(clone.URL.Hostname(), 240),
		},
	}
	t.Client.enqueue(t.Client.observedSpan("http.client", metadata, traceID, spanID, parent.SpanID, start, err != nil || (response != nil && response.StatusCode >= 500)))
	return response, err
}

// Database records a database operation identity without SQL text or params.
func (c *Client) Database(ctx context.Context, system, operation string, metadata Metadata, fn func(context.Context) error) error {
	metadata.FlowType = "database"
	metadata.FlowDirection = "outbound"
	metadata.DBSystem = system
	metadata.Attributes = cloneMetadata(metadata.Attributes)
	metadata.Attributes["db.operation.name"] = bounded(operation, 120)
	return c.Span(ctx, "database."+bounded(operation, 120), metadata, fn)
}

// Cache records a cache operation without keys or values.
func (c *Client) Cache(ctx context.Context, system, operation string, metadata Metadata, fn func(context.Context) error) error {
	metadata.FlowType = "cache"
	metadata.FlowDirection = "outbound"
	metadata.DBSystem = system
	metadata.Attributes = cloneMetadata(metadata.Attributes)
	metadata.Attributes["db.operation.name"] = bounded(operation, 120)
	return c.Span(ctx, "cache."+bounded(operation, 120), metadata, fn)
}

// Message records queue/event producer or consumer execution without payloads.
func (c *Client) Message(ctx context.Context, system, operation, direction string, metadata Metadata, fn func(context.Context) error) error {
	metadata.FlowType = "queue"
	metadata.FlowDirection = bounded(direction, 40)
	metadata.MessagingSystem = system
	metadata.Attributes = cloneMetadata(metadata.Attributes)
	metadata.Attributes["messaging.operation.name"] = bounded(operation, 120)
	return c.Span(ctx, "messaging."+bounded(operation, 120), metadata, fn)
}

// SafeURL returns scheme/host/path only. It exists for integrations that need a
// bounded destination identity while guaranteeing query/userinfo removal.
func SafeURL(raw string) string {
	parsed, err := url.Parse(raw)
	if err != nil {
		return ""
	}
	parsed.RawQuery = ""
	parsed.Fragment = ""
	parsed.User = nil
	return bounded(parsed.String(), 400)
}

func (c *Client) loop() {
	ticker := time.NewTicker(c.flushInterval)
	defer func() {
		ticker.Stop()
		close(c.stopped)
	}()
	for {
		select {
		case <-ticker.C:
			_ = c.Flush(context.Background())
		case <-c.wake:
			_ = c.Flush(context.Background())
		case <-c.stop:
			_ = c.Flush(context.Background())
			return
		}
	}
}

func (c *Client) takeBatch() []spanRecord {
	c.mu.Lock()
	defer c.mu.Unlock()
	if len(c.queue) == 0 {
		return nil
	}
	count := c.maxBatch
	if len(c.queue) < count {
		count = len(c.queue)
	}
	batch := append([]spanRecord(nil), c.queue[:count]...)
	c.queue = append([]spanRecord(nil), c.queue[count:]...)
	return batch
}

func (c *Client) restoreBatch(batch []spanRecord) {
	if len(batch) == 0 {
		return
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.closed {
		return
	}
	combined := append([]spanRecord(nil), batch...)
	combined = append(combined, c.queue...)
	if len(combined) > c.maxBatch*4 {
		combined = combined[:c.maxBatch*4]
	}
	c.queue = combined
}

// Flush sends one bounded batch. Failed batches are restored to memory; the
// application request is never failed because CKB telemetry transport failed.
func (c *Client) Flush(ctx context.Context) error {
	if !c.Configured() {
		return nil
	}
	batch := c.takeBatch()
	if len(batch) == 0 {
		return nil
	}
	payload := otlpEnvelope{}
	payload.ResourceSpans = make([]struct {
		Resource struct {
			Attributes []attribute `json:"attributes"`
		} `json:"resource"`
		ScopeSpans []struct {
			Scope struct {
				Name    string `json:"name"`
				Version string `json:"version"`
			} `json:"scope"`
			Spans []spanRecord `json:"spans"`
		} `json:"scopeSpans"`
	}, 1)
	payload.ResourceSpans[0].Resource.Attributes = c.resourceAttributes()
	payload.ResourceSpans[0].ScopeSpans = make([]struct {
		Scope struct {
			Name    string `json:"name"`
			Version string `json:"version"`
		} `json:"scope"`
		Spans []spanRecord `json:"spans"`
	}, 1)
	payload.ResourceSpans[0].ScopeSpans[0].Scope.Name = "ckb.live.reality"
	payload.ResourceSpans[0].ScopeSpans[0].Scope.Version = "1.0.0"
	payload.ResourceSpans[0].ScopeSpans[0].Spans = batch

	encoded, err := json.Marshal(payload)
	if err != nil {
		c.restoreBatch(batch)
		return err
	}
	request, err := http.NewRequestWithContext(ctx, http.MethodPost, c.endpoint, bytes.NewReader(encoded))
	if err != nil {
		c.restoreBatch(batch)
		return err
	}
	request.Header.Set("Content-Type", "application/json")
	request.Header.Set("X-CKB-Telemetry-Key", c.key)
	request.Header.Set("User-Agent", "CKB-Live-Reality-Go/1.0")
	response, err := c.httpClient.Do(request)
	if err != nil {
		c.restoreBatch(batch)
		return err
	}
	defer response.Body.Close()
	if response.StatusCode < 200 || response.StatusCode >= 300 {
		c.restoreBatch(batch)
		return fmt.Errorf("CKB telemetry endpoint returned %s", response.Status)
	}
	return nil
}

// Shutdown flushes queued telemetry without changing application semantics.
func (c *Client) Shutdown(ctx context.Context) error {
	if c == nil {
		return nil
	}
	c.mu.Lock()
	if c.closed {
		c.mu.Unlock()
		return nil
	}
	c.closed = true
	c.mu.Unlock()
	close(c.stop)
	select {
	case <-c.stopped:
		return nil
	case <-ctx.Done():
		return ctx.Err()
	}
}
