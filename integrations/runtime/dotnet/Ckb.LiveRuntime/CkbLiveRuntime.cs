using System.Security.Cryptography;
using System.Text;
using System.Text.Json;

namespace Ckb.LiveRuntime;

public enum FlowType
{
    Function,
    HttpServer,
    HttpClient,
    Database,
    Cache,
    Queue,
    Event,
    Websocket,
}

public sealed record TraceContext(string TraceId, string SpanId, byte TraceFlags = 1)
{
    public static TraceContext Root() => new(RandomHex(16), RandomHex(8));
    public TraceContext Child() => new(TraceId, RandomHex(8), TraceFlags);
    public string TraceParent => $"00-{TraceId}-{SpanId}-{TraceFlags:x2}";

    public static TraceContext? Parse(string? value)
    {
        if (string.IsNullOrWhiteSpace(value)) return null;
        var parts = value.Trim().Split('-');
        if (parts.Length != 4 || !ValidHex(parts[0], 2) || !ValidHex(parts[1], 32) || !ValidHex(parts[2], 16) || !ValidHex(parts[3], 2)) return null;
        if (parts[1].All(ch => ch == '0') || parts[2].All(ch => ch == '0')) return null;
        return byte.TryParse(parts[3], System.Globalization.NumberStyles.HexNumber, null, out var flags)
            ? new TraceContext(parts[1], parts[2], flags)
            : null;
    }

    private static string RandomHex(int bytes) => Convert.ToHexString(RandomNumberGenerator.GetBytes(bytes)).ToLowerInvariant();
    private static bool ValidHex(string value, int length) => value.Length == length && value.All(ch => ch is >= '0' and <= '9' or >= 'a' and <= 'f');
}

public interface IExporter
{
    ValueTask ExportAsync(string otlpJson, CancellationToken cancellationToken = default);
}

public sealed record ActiveSpan(
    string Name,
    TraceContext Context,
    string ParentSpanId,
    long StartUnixNano,
    IReadOnlyDictionary<string, string> Attributes);

public sealed record SpanRecord(
    string Name,
    TraceContext Context,
    string ParentSpanId,
    long StartUnixNano,
    long EndUnixNano,
    bool Error,
    IReadOnlyDictionary<string, string> Attributes);

public sealed class RuntimeCollector
{
    private static readonly string[] Forbidden =
    [
        "password", "passwd", "secret", "token", "authorization", "cookie", "session",
        "api_key", "apikey", "request_body", "response_body", "payload"
    ];

    private readonly string _serviceName;
    private readonly IExporter _exporter;
    private readonly int _maxBatch;
    private readonly List<SpanRecord> _pending = [];

    public RuntimeCollector(string serviceName, IExporter exporter, int maxBatch = 32)
    {
        _serviceName = Bounded(serviceName, 120);
        _exporter = exporter ?? throw new ArgumentNullException(nameof(exporter));
        _maxBatch = Math.Clamp(maxBatch, 1, 256);
    }

    public int PendingCount => _pending.Count;

    public ActiveSpan Start(string name, TraceContext? parent, FlowType flowType, IReadOnlyDictionary<string, string>? attributes = null)
    {
        var context = parent?.Child() ?? TraceContext.Root();
        var safe = SanitizeAttributes(attributes ?? new Dictionary<string, string>());
        safe["ckb.flow.type"] = FlowName(flowType);
        return new ActiveSpan(Bounded(name, 180), context, parent?.SpanId ?? string.Empty, UnixNanos(), safe);
    }

    public ActiveSpan StartHttpClient(string method, string routeTemplate, TraceContext? parent)
    {
        var route = SafeRouteTemplate(routeTemplate);
        return Start($"{Bounded(method, 16)} {route}", parent, FlowType.HttpClient, new Dictionary<string, string>
        {
            ["http.request.method"] = Bounded(method, 16),
            ["http.route"] = route,
        });
    }

    public async ValueTask FinishAsync(ActiveSpan span, bool error, CancellationToken cancellationToken = default)
    {
        _pending.Add(new SpanRecord(span.Name, span.Context, span.ParentSpanId, span.StartUnixNano, Math.Max(UnixNanos(), span.StartUnixNano), error, span.Attributes));
        if (_pending.Count >= _maxBatch) await FlushAsync(cancellationToken);
    }

    public async ValueTask FlushAsync(CancellationToken cancellationToken = default)
    {
        if (_pending.Count == 0) return;
        var payload = OtlpJson(_serviceName, _pending);
        await _exporter.ExportAsync(payload, cancellationToken);
        _pending.Clear();
    }

    public static Dictionary<string, string> SanitizeAttributes(IReadOnlyDictionary<string, string> attributes)
    {
        var safe = new Dictionary<string, string>(StringComparer.Ordinal);
        foreach (var (keyRaw, valueRaw) in attributes)
        {
            var key = Bounded(keyRaw, 120);
            var normalized = key.ToLowerInvariant().Replace('-', '_').Replace('.', '_');
            if (Forbidden.Any(normalized.Contains)) continue;
            safe[key] = Bounded(valueRaw, 512);
        }
        return safe;
    }

    public static string SafeRouteTemplate(string? value)
    {
        var raw = (value ?? string.Empty).Trim();
        var cut = raw.IndexOfAny(['?', '#']);
        if (cut >= 0) raw = raw[..cut];
        if (Uri.TryCreate(raw, UriKind.Absolute, out var absolute)) raw = absolute.AbsolutePath;
        if (string.IsNullOrWhiteSpace(raw)) raw = "/";
        if (!raw.StartsWith('/')) raw = "/" + raw;
        return Bounded(raw, 240);
    }

    private static string OtlpJson(string serviceName, IEnumerable<SpanRecord> spans)
    {
        var payload = new
        {
            resourceSpans = new[]
            {
                new
                {
                    resource = new { attributes = new[] { new { key = "service.name", value = new { stringValue = serviceName } } } },
                    scopeSpans = new[]
                    {
                        new
                        {
                            scope = new { name = "ckb-live-dotnet" },
                            spans = spans.Select(span => new
                            {
                                traceId = span.Context.TraceId,
                                spanId = span.Context.SpanId,
                                parentSpanId = span.ParentSpanId,
                                name = span.Name,
                                startTimeUnixNano = span.StartUnixNano.ToString(System.Globalization.CultureInfo.InvariantCulture),
                                endTimeUnixNano = span.EndUnixNano.ToString(System.Globalization.CultureInfo.InvariantCulture),
                                attributes = span.Attributes.Select(item => new { key = item.Key, value = new { stringValue = item.Value } }).ToArray(),
                                status = new { code = span.Error ? 2 : 1 },
                            }).ToArray(),
                        },
                    },
                },
            },
        };
        return JsonSerializer.Serialize(payload);
    }

    private static string FlowName(FlowType flowType) => flowType switch
    {
        FlowType.HttpServer => "http-server",
        FlowType.HttpClient => "http-client",
        FlowType.Database => "database",
        FlowType.Cache => "cache",
        FlowType.Queue => "queue",
        FlowType.Event => "event",
        FlowType.Websocket => "websocket",
        _ => "function",
    };

    private static string Bounded(string? value, int max)
    {
        var text = (value ?? string.Empty).Trim();
        return text.Length <= max ? text : text[..max];
    }

    private static long UnixNanos()
    {
        var now = DateTimeOffset.UtcNow;
        var milliseconds = now.ToUnixTimeMilliseconds();
        var nanosWithinMillisecond = (now.Ticks % TimeSpan.TicksPerMillisecond) * 100;
        return checked(milliseconds * 1_000_000L + nanosWithinMillisecond);
    }
}
