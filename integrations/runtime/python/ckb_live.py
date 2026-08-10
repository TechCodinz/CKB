"""CKB Live Reality runtime agent for Python 3.9+.

Dependency-free OTLP/HTTP JSON emitter with ContextVar parent/child tracing.
It records structural/runtime metadata only and never copies HTTP bodies,
headers, SQL text, cache values, queue payloads, cookies, or secrets.
"""

from __future__ import annotations

import asyncio
import contextvars
import json
import os
import secrets
import threading
import time
import urllib.parse
import urllib.request
from dataclasses import dataclass
from typing import Any, Awaitable, Callable, Dict, Iterable, Optional

_trace_context: contextvars.ContextVar[Optional[Dict[str, str]]] = contextvars.ContextVar("ckb_trace_context", default=None)


def _otlp_value(value: Any) -> Dict[str, Any]:
    if isinstance(value, bool):
        return {"boolValue": value}
    if isinstance(value, (int, float)) and not isinstance(value, bool):
        return {"doubleValue": float(value)}
    return {"stringValue": str(value if value is not None else "")}


def _safe_attributes(values: Optional[Dict[str, Any]] = None) -> list[Dict[str, Any]]:
    output: list[Dict[str, Any]] = []
    for key, raw in (values or {}).items():
        if raw is None or isinstance(raw, (dict, list, tuple, set, bytes, bytearray)):
            continue
        output.append({"key": str(key)[:120], "value": _otlp_value(raw)})
        if len(output) >= 64:
            break
    return output


def _hex(bytes_count: int) -> str:
    return secrets.token_hex(bytes_count)


def _now_ns() -> str:
    return str(time.time_ns())


def _host(value: str) -> str:
    try:
        return urllib.parse.urlparse(value).hostname or ""
    except Exception:
        return ""


@dataclass
class SpanMetadata:
    file: Optional[str] = None
    function_name: Optional[str] = None
    namespace: Optional[str] = None
    kind: str = "function"
    flow_type: str = "function"
    direction: str = "internal"
    attributes: Optional[Dict[str, Any]] = None


class CkbLive:
    def __init__(
        self,
        endpoint: Optional[str] = None,
        key: Optional[str] = None,
        service_name: Optional[str] = None,
        environment: Optional[str] = None,
        flush_interval: float = 12.0,
        max_batch: int = 96,
    ) -> None:
        self.endpoint = (endpoint or os.getenv("CKB_OTLP_ENDPOINT") or "").strip()
        self.key = (key or os.getenv("CKB_OTLP_KEY") or "").strip()
        self.service_name = service_name or os.getenv("CKB_SERVICE_NAME") or os.getenv("RENDER_SERVICE_NAME") or "python-service"
        self.environment = environment or os.getenv("ENVIRONMENT") or os.getenv("NODE_ENV") or "unknown"
        self.flush_interval = max(10.0, float(flush_interval))
        self.max_batch = max(8, min(256, int(max_batch)))
        self._queue: list[Dict[str, Any]] = []
        self._lock = threading.Lock()
        self._wake = threading.Event()
        self._stopped = False
        self._worker = threading.Thread(target=self._run_worker, name="ckb-live-reality", daemon=True)
        self._worker.start()

    @property
    def configured(self) -> bool:
        return bool(self.endpoint and self.key)

    def _resource_attributes(self) -> list[Dict[str, Any]]:
        return _safe_attributes({
            "service.name": self.service_name,
            "deployment.environment": self.environment,
            "telemetry.sdk.name": "ckb-live-reality",
            "telemetry.sdk.language": "python",
            "ckb.runtime.agent": "python-zero-dependency-v1",
        })

    def _run_worker(self) -> None:
        while not self._stopped:
            self._wake.wait(self.flush_interval)
            self._wake.clear()
            if self._stopped:
                break
            try:
                self.flush()
            except Exception:
                pass

    def _enqueue(self, span: Dict[str, Any]) -> None:
        if not self.configured or self._stopped:
            return
        should_wake = False
        with self._lock:
            self._queue.append(span)
            should_wake = len(self._queue) >= self.max_batch
        if should_wake:
            self._wake.set()

    def flush(self) -> Dict[str, Any]:
        if not self.configured or self._stopped:
            return {"sent": 0}
        with self._lock:
            if not self._queue:
                return {"sent": 0}
            batch = self._queue[: self.max_batch]
            del self._queue[: len(batch)]

        payload = json.dumps({
            "resourceSpans": [{
                "resource": {"attributes": self._resource_attributes()},
                "scopeSpans": [{
                    "scope": {"name": "ckb.live.reality", "version": "1.0.0"},
                    "spans": batch,
                }],
            }],
        }).encode("utf-8")
        request = urllib.request.Request(
            self.endpoint,
            data=payload,
            method="POST",
            headers={
                "Content-Type": "application/json",
                "X-CKB-Telemetry-Key": self.key,
                "User-Agent": "CKB-Live-Reality-Python/1.0",
            },
        )
        try:
            with urllib.request.urlopen(request, timeout=10) as response:
                status = int(getattr(response, "status", 200))
            if status >= 400:
                raise RuntimeError(f"CKB telemetry returned HTTP {status}")
            return {"sent": len(batch), "status": status}
        except Exception as exc:
            with self._lock:
                self._queue[0:0] = batch[-self.max_batch :]
            return {"sent": 0, "error": str(exc)}

    def _context(self) -> Dict[str, str]:
        parent = _trace_context.get() or {}
        return {
            "traceId": parent.get("traceId") or _hex(16),
            "spanId": _hex(8),
            "parentSpanId": parent.get("spanId") or "",
        }

    def _record(self, name: str, metadata: SpanMetadata, context: Dict[str, str], start: str, error: Optional[BaseException]) -> None:
        attrs: Dict[str, Any] = {
            "code.function.name": metadata.function_name or name,
            "code.file.path": metadata.file,
            "code.namespace": metadata.namespace,
            "ckb.symbol.kind": metadata.kind,
            "ckb.runtime.observed": True,
            "ckb.flow.type": metadata.flow_type,
            "ckb.flow.direction": metadata.direction,
        }
        attrs.update(metadata.attributes or {})
        self._enqueue({
            "traceId": context["traceId"],
            "spanId": context["spanId"],
            "parentSpanId": context["parentSpanId"],
            "name": str(name),
            "startTimeUnixNano": start,
            "endTimeUnixNano": _now_ns(),
            "attributes": _safe_attributes(attrs),
            "status": {"code": 2 if error else 1},
        })

    def span(self, name: str, fn: Callable[..., Any], *args: Any, metadata: Optional[SpanMetadata] = None, **kwargs: Any) -> Any:
        meta = metadata or SpanMetadata(function_name=name)
        context = self._context()
        token = _trace_context.set({"traceId": context["traceId"], "spanId": context["spanId"]})
        start = _now_ns()
        error: Optional[BaseException] = None
        try:
            return fn(*args, **kwargs)
        except BaseException as exc:
            error = exc
            raise
        finally:
            _trace_context.reset(token)
            self._record(name, meta, context, start, error)

    async def span_async(self, name: str, fn: Callable[..., Awaitable[Any]], *args: Any, metadata: Optional[SpanMetadata] = None, **kwargs: Any) -> Any:
        meta = metadata or SpanMetadata(function_name=name)
        context = self._context()
        token = _trace_context.set({"traceId": context["traceId"], "spanId": context["spanId"]})
        start = _now_ns()
        error: Optional[BaseException] = None
        try:
            return await fn(*args, **kwargs)
        except BaseException as exc:
            error = exc
            raise
        finally:
            _trace_context.reset(token)
            self._record(name, meta, context, start, error)

    def wrap(self, name: str, fn: Callable[..., Any], metadata: Optional[SpanMetadata] = None) -> Callable[..., Any]:
        def wrapped(*args: Any, **kwargs: Any) -> Any:
            return self.span(name, fn, *args, metadata=metadata, **kwargs)
        return wrapped

    def wrap_async(self, name: str, fn: Callable[..., Awaitable[Any]], metadata: Optional[SpanMetadata] = None) -> Callable[..., Awaitable[Any]]:
        async def wrapped(*args: Any, **kwargs: Any) -> Any:
            return await self.span_async(name, fn, *args, metadata=metadata, **kwargs)
        return wrapped

    def request(self, url: str, *, method: str = "GET", data: Optional[bytes] = None, headers: Optional[Dict[str, str]] = None, timeout: float = 10.0) -> Any:
        # Deliberately does not copy URL query parameters, headers, or request body into telemetry.
        host = _host(url)
        safe_url = urllib.parse.urlunparse((*urllib.parse.urlparse(url)[:3], "", "", ""))
        meta = SpanMetadata(
            function_name="urllib.request",
            kind="outbound-http",
            flow_type="http-client",
            direction="outbound",
            attributes={
                "http.request.method": method.upper(),
                "server.address": host,
                "network.protocol.name": "http",
            },
        )
        return self.span(
            f"HTTP {method.upper()} {host or 'external'}",
            lambda: urllib.request.urlopen(urllib.request.Request(safe_url, data=data, method=method, headers=headers or {}), timeout=timeout),
            metadata=meta,
        )

    def shutdown(self) -> Dict[str, Any]:
        result = self.flush()
        self._stopped = True
        self._wake.set()
        return result


class CkbASGIMiddleware:
    """ASGI middleware for FastAPI, Starlette, Django ASGI, Quart-compatible stacks."""

    def __init__(self, app: Callable[..., Awaitable[Any]], live: CkbLive, *, file: Optional[str] = None, namespace: Optional[str] = None) -> None:
        self.app = app
        self.live = live
        self.file = file
        self.namespace = namespace or live.service_name

    async def __call__(self, scope: Dict[str, Any], receive: Callable[..., Awaitable[Any]], send: Callable[..., Awaitable[Any]]) -> None:
        if scope.get("type") != "http" or not self.live.configured:
            await self.app(scope, receive, send)
            return

        method = str(scope.get("method") or "HTTP").upper()
        route = str(scope.get("path") or "/")[:220]
        status = 0

        async def send_observed(message: Dict[str, Any]) -> None:
            nonlocal status
            if message.get("type") == "http.response.start":
                status = int(message.get("status") or 0)
            await send(message)

        meta = SpanMetadata(
            file=self.file,
            function_name="asgi.request",
            namespace=self.namespace,
            kind="route",
            flow_type="http-server",
            direction="inbound",
            attributes={"http.request.method": method, "http.route": route, "network.protocol.name": "http"},
        )

        async def run() -> None:
            await self.app(scope, receive, send_observed)

        try:
            await self.live.span_async(f"{method} {route}", run, metadata=meta)
        finally:
            # Response status is intentionally omitted from the already-completed span when unavailable.
            _ = status


class CkbWSGIMiddleware:
    """WSGI middleware for Django WSGI, Flask, and other WSGI applications."""

    def __init__(self, app: Callable[..., Iterable[bytes]], live: CkbLive, *, file: Optional[str] = None, namespace: Optional[str] = None) -> None:
        self.app = app
        self.live = live
        self.file = file
        self.namespace = namespace or live.service_name

    def __call__(self, environ: Dict[str, Any], start_response: Callable[..., Any]) -> Iterable[bytes]:
        method = str(environ.get("REQUEST_METHOD") or "HTTP").upper()
        route = str(environ.get("PATH_INFO") or "/")[:220]
        meta = SpanMetadata(
            file=self.file,
            function_name="wsgi.request",
            namespace=self.namespace,
            kind="route",
            flow_type="http-server",
            direction="inbound",
            attributes={"http.request.method": method, "http.route": route, "network.protocol.name": "http"},
        )
        return self.live.span(f"{method} {route}", lambda: self.app(environ, start_response), metadata=meta)


class CkbDbConnection:
    """DB-API connection proxy that observes operations without recording SQL text/parameters."""

    def __init__(self, connection: Any, live: CkbLive, *, system: str = "database", file: Optional[str] = None, namespace: Optional[str] = None) -> None:
        self._connection = connection
        self._live = live
        self._system = system
        self._file = file
        self._namespace = namespace

    def cursor(self, *args: Any, **kwargs: Any) -> "CkbDbCursor":
        return CkbDbCursor(self._connection.cursor(*args, **kwargs), self._live, system=self._system, file=self._file, namespace=self._namespace)

    def __getattr__(self, name: str) -> Any:
        return getattr(self._connection, name)


class CkbDbCursor:
    def __init__(self, cursor: Any, live: CkbLive, *, system: str, file: Optional[str], namespace: Optional[str]) -> None:
        self._cursor = cursor
        self._live = live
        self._system = system
        self._file = file
        self._namespace = namespace

    def _operation(self, method: str, fn: Callable[..., Any], *args: Any, **kwargs: Any) -> Any:
        meta = SpanMetadata(
            file=self._file,
            function_name=f"{self._system}.{method}",
            namespace=self._namespace,
            kind="database",
            flow_type="database",
            direction="outbound",
            attributes={"db.system": self._system, "db.operation.name": method, "ckb.data.capture": "metadata-only"},
        )
        return self._live.span(f"{self._system}.{method}", fn, *args, metadata=meta, **kwargs)

    def execute(self, *args: Any, **kwargs: Any) -> Any:
        return self._operation("execute", self._cursor.execute, *args, **kwargs)

    def executemany(self, *args: Any, **kwargs: Any) -> Any:
        return self._operation("executemany", self._cursor.executemany, *args, **kwargs)

    def callproc(self, *args: Any, **kwargs: Any) -> Any:
        return self._operation("callproc", self._cursor.callproc, *args, **kwargs)

    def __getattr__(self, name: str) -> Any:
        return getattr(self._cursor, name)


def instrument_db(connection: Any, live: CkbLive, *, system: str = "database", file: Optional[str] = None, namespace: Optional[str] = None) -> CkbDbConnection:
    return CkbDbConnection(connection, live, system=system, file=file, namespace=namespace)


__all__ = [
    "CkbLive",
    "SpanMetadata",
    "CkbASGIMiddleware",
    "CkbWSGIMiddleware",
    "CkbDbConnection",
    "instrument_db",
]
