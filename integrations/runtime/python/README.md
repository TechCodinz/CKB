# CKB Live Reality — Python Runtime Agent

`ckb_live.py` is a dependency-free Python 3.9+ runtime agent for CKB's Semantic Universe.

It emits real OTLP/HTTP JSON evidence with `ContextVar` parent/child trace identity so CKB can fuse deployed execution back onto the static repository graph.

## Environment

Create a project-scoped key from **Living Graph Universe → LIVE LINK** and configure:

```bash
CKB_OTLP_ENDPOINT=https://ckb-private.onrender.com/api/v1/reality/intelligence/telemetry/otlp
CKB_OTLP_KEY=<project-scoped-live-key>
CKB_SERVICE_NAME=orders-api
```

## FastAPI / Starlette

```python
from fastapi import FastAPI
from ckb_live import CkbLive, CkbASGIMiddleware

live = CkbLive(service_name="orders-api")
app = FastAPI()
app.add_middleware(
    CkbASGIMiddleware,
    live=live,
    file="src/main.py",
    namespace="orders-api",
)
```

## Django ASGI

```python
from ckb_live import CkbLive, CkbASGIMiddleware
from myproject.asgi import application

live = CkbLive(service_name="django-api")
application = CkbASGIMiddleware(
    application,
    live,
    file="myproject/asgi.py",
    namespace="django-api",
)
```

## Flask / Django WSGI

```python
from ckb_live import CkbLive, CkbWSGIMiddleware

live = CkbLive(service_name="web-api")
app.wsgi_app = CkbWSGIMiddleware(
    app.wsgi_app,
    live,
    file="src/app.py",
    namespace="web-api",
)
```

## Internal functions

```python
from ckb_live import CkbLive, SpanMetadata

live = CkbLive(service_name="payments")

result = live.span(
    "authorize_payment",
    authorize_payment,
    order,
    metadata=SpanMetadata(
        file="src/services/payment.py",
        function_name="authorize_payment",
        namespace="payment",
        flow_type="function",
    ),
)
```

Async functions:

```python
result = await live.span_async(
    "charge_card",
    charge_card,
    order,
    metadata=SpanMetadata(
        file="src/services/payment.py",
        function_name="charge_card",
        namespace="payment",
    ),
)
```

## Database calls

Wrap any Python DB-API connection. CKB records operation identity only; SQL text and parameters are never copied into telemetry.

```python
from ckb_live import instrument_db

observed = instrument_db(
    connection,
    live,
    system="postgresql",
    file="src/db.py",
    namespace="persistence",
)

cursor = observed.cursor()
cursor.execute("SELECT ...", params)
```

## Outbound HTTP

The built-in helper records method/host only. It deliberately drops URL query strings and does not copy headers or request bodies into telemetry.

```python
response = live.request(
    "https://payments.example.com/authorize",
    method="POST",
    data=encoded_body,
)
```

## Typed flow evidence

The Python agent emits `ckb.flow.type` values such as:

- `http-server`
- `http-client`
- `database`
- `function`

This lets the Semantic Universe distinguish runtime transmission types from static relationships while preserving the rule that only observed execution can pulse as runtime.

## Shutdown

```python
live.shutdown()
```

## Privacy contract

The agent does not record request/response bodies, headers, cookies, authorization values, SQL text, query parameters, database values, or arbitrary objects. CKB receives only bounded structural/runtime metadata required to visualize execution.
