using Ckb.LiveRuntime;

static void Require(bool value, string message)
{
    if (!value) throw new InvalidOperationException(message);
}

var root = TraceContext.Root();
var parsed = TraceContext.Parse(root.TraceParent);
Require(parsed is not null, "traceparent should parse");
Require(parsed!.TraceId == root.TraceId && parsed.SpanId == root.SpanId, "traceparent should round-trip");
Require(TraceContext.Parse("00-00000000000000000000000000000000-0000000000000000-01") is null, "all-zero identity must be rejected");

Require(RuntimeCollector.SafeRouteTemplate("https://example.com/orders/:id?token=private#top") == "/orders/:id", "route must drop origin/query/fragment");
var safe = RuntimeCollector.SanitizeAttributes(new Dictionary<string, string>
{
    ["db.system"] = "postgresql",
    ["authorization"] = "Bearer hidden",
    ["user.session.token"] = "private",
    ["request.body"] = "sensitive",
});
Require(safe.TryGetValue("db.system", out var dbSystem) && dbSystem == "postgresql", "safe metadata should remain");
Require(!safe.ContainsKey("authorization") && !safe.ContainsKey("user.session.token") && !safe.ContainsKey("request.body"), "sensitive attributes must be removed");

var exporter = new MemoryExporter();
var collector = new RuntimeCollector("checkout-api", exporter, 1);
var httpSpan = collector.StartHttpClient("POST", "https://pay.example/charge/:id?token=nope", root);
await collector.FinishAsync(httpSpan, false);
Require(exporter.Payloads.Count == 1, "configured threshold should export one batch");
var payload = exporter.Payloads[0];
Require(payload.Contains("checkout-api", StringComparison.Ordinal), "service identity should be exported");
Require(payload.Contains("/charge/:id", StringComparison.Ordinal), "stable route should be exported");
Require(!payload.Contains("token=nope", StringComparison.OrdinalIgnoreCase), "raw query must not be exported");
Require(!payload.Contains("authorization", StringComparison.OrdinalIgnoreCase), "authorization must not be exported");
Require(collector.PendingCount == 0, "successful export should clear pending observations");

var failing = new RuntimeCollector("service", new FailingExporter(), 1);
var failedSpan = failing.Start("work", null, FlowType.Function);
var surfaced = false;
try
{
    await failing.FinishAsync(failedSpan, true);
}
catch (InvalidOperationException)
{
    surfaced = true;
}
Require(surfaced, "export failure should surface");
Require(failing.PendingCount == 1, "failed export must retain observation for retry");

Console.WriteLine("CKB .NET runtime collector tests passed");

sealed class MemoryExporter : IExporter
{
    public List<string> Payloads { get; } = [];
    public ValueTask ExportAsync(string otlpJson, CancellationToken cancellationToken = default)
    {
        Payloads.Add(otlpJson);
        return ValueTask.CompletedTask;
    }
}

sealed class FailingExporter : IExporter
{
    public ValueTask ExportAsync(string otlpJson, CancellationToken cancellationToken = default)
        => ValueTask.FromException(new InvalidOperationException("blocked"));
}
