# CKB Live Reality — Go Runtime Agent

`ckb_live.go` is a standard-library-only Go runtime agent for the CKB Live Execution Twin.

It emits OTLP/HTTP JSON for code that actually executes and carries exact parent/child span identity through Go `context.Context`. Outbound HTTP uses W3C `traceparent`, so independently instrumented Go services can remain part of one distributed execution trace.

## Environment

```bash
CKB_OTLP_ENDPOINT=https://<your-ckb-cloud-ingest>/api/v1/reality/intelligence/telemetry/otlp
CKB_OTLP_KEY=<project-scoped-live-key>
CKB_SERVICE_NAME=orders-api
```

A project-scoped runtime key should be used. Do not place the user's browser JWT or CKB's internal Reality credential inside the application deployment.

## Create the client

```go
package main

import (
    "context"
    "net/http"
    "os/signal"
    "syscall"
    "time"

    ckblive "github.com/TechCodinz/CKB/integrations/runtime/go"
)

func main() {
    live := ckblive.New(ckblive.Config{ServiceName: "orders-api"})

    mux := http.NewServeMux()
    mux.Handle("/orders", live.Middleware(
        "GET /orders",
        ckblive.Metadata{
            File: "internal/http/orders.go",
            Function: "listOrders",
            Namespace: "orders-api",
        },
        http.HandlerFunc(listOrders),
    ))

    // ... run server ...

    ctx, stop := signal.NotifyContext(context.Background(), syscall.SIGTERM, syscall.SIGINT)
    defer stop()
    <-ctx.Done()
    shutdown, cancel := context.WithTimeout(context.Background(), 5*time.Second)
    defer cancel()
    _ = live.Shutdown(shutdown)
}
```

## Internal functions

```go
err := live.Span(ctx, "authorizePayment", ckblive.Metadata{
    File: "internal/payment/service.go",
    Function: "authorizePayment",
    Namespace: "payment",
    FlowType: "function",
}, func(spanCtx context.Context) error {
    return authorizePayment(spanCtx, orderID)
})
```

Only the structural identity above is emitted. Function arguments and return values are not serialized by the agent.

## Outbound HTTP

Wrap an `http.Client` transport:

```go
client := &http.Client{
    Transport: ckblive.Transport{
        Client: live,
        Base: http.DefaultTransport,
        File: "internal/payment/gateway.go",
    },
}
```

The wrapper records method and destination hostname and propagates W3C `traceparent`. It does **not** copy the application's headers, body or URL query string into telemetry.

## Database / cache / messaging boundaries

```go
err := live.Database(ctx, "postgresql", "orders.select", ckblive.Metadata{
    File: "internal/store/orders.go",
    Function: "findOrder",
}, func(spanCtx context.Context) error {
    return findOrder(spanCtx, db, id)
})

err = live.Cache(ctx, "redis", "get", ckblive.Metadata{
    File: "internal/cache/session.go",
    Function: "getSession",
}, func(spanCtx context.Context) error {
    return getSession(spanCtx, cache)
})

err = live.Message(ctx, "kafka", "order.created", "producer", ckblive.Metadata{
    File: "internal/events/orders.go",
    Function: "publishOrderCreated",
}, func(spanCtx context.Context) error {
    return publish(spanCtx, event)
})
```

CKB records operation/system identity only. SQL text, bind parameters, Redis keys/values and message payloads are not parameters to these telemetry helpers.

## Privacy contract

The first-party Go agent does not intentionally export:

- request or response bodies;
- authorization/cookie values;
- application header sets;
- URL query strings or userinfo;
- SQL text or database values;
- Redis values;
- queue/event payloads;
- arbitrary application objects.

Custom metadata is bounded and filters attribute names associated with bodies, credentials, secrets, passwords, payloads and SQL. Applications should still treat custom telemetry metadata as operational data and avoid inserting sensitive values.

## Truth contract

- no application execution → no application span;
- missing source path → runtime evidence can remain unresolved rather than being guessed onto a source symbol;
- distributed parent/child links come from runtime context / W3C trace context;
- transport failure does not fail the application request;
- runtime replay in CKB represents already-observed spans, not a simulated execution;
- line-level execution is not claimed from these function/boundary spans.
