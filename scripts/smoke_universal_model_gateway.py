#!/usr/bin/env python3
"""Public production-safe smoke test for the CKB Universal Model Gateway.

The test uses no CKB credentials. It verifies provider tool-schema views and
checks that an unauthenticated execution is rejected through the canonical MCP
OAuth challenge rather than bypassing authentication.

Usage:
    python scripts/smoke_universal_model_gateway.py
    CKB_MCP_BASE_URL=https://preview.example.com python scripts/smoke_universal_model_gateway.py
"""

from __future__ import annotations

import json
import os
import sys
import urllib.error
import urllib.request

BASE_URL = os.environ.get(
    "CKB_MCP_BASE_URL", "https://ckb-mcp-server.onrender.com"
).rstrip("/")
EXPECTED_TOOLS = {
    "ckb_scan_repository",
    "ckb_get_architecture_graph",
    "ckb_analyze_impact",
    "ckb_get_runtime_intelligence",
    "ckb_get_drift_history",
    "ckb_get_test_gaps",
    "ckb_find_causal_path",
    "ckb_get_failure_cone",
    "ckb_query_architecture_memory",
    "ckb_get_code_dna",
    "ckb_list_snapshots",
    "ckb_diff_snapshots",
    "ckb_generate_ai_rules",
}


def fail(message: str) -> "NoReturn":
    print(f"FAIL: {message}", file=sys.stderr)
    raise SystemExit(1)


def lower_headers(message) -> dict:
    """HTTP header names are case-insensitive, and hyper/axum emit them
    lowercased on the wire. Normalize so lookups do not depend on casing."""
    return {name.lower(): value for name, value in message.items()}


def request_json(url: str, *, payload: dict | None = None) -> tuple[int, dict, dict]:
    data = None if payload is None else json.dumps(payload).encode("utf-8")
    headers = {"Accept": "application/json"}
    method = "GET"
    if data is not None:
        headers["Content-Type"] = "application/json"
        method = "POST"
    request = urllib.request.Request(url, data=data, headers=headers, method=method)
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            raw = response.read().decode("utf-8")
            return response.status, lower_headers(response.headers), json.loads(raw or "{}")
    except urllib.error.HTTPError as error:
        raw = error.read().decode("utf-8")
        try:
            body = json.loads(raw or "{}")
        except json.JSONDecodeError:
            body = {"raw": raw}
        return error.code, lower_headers(error.headers), body


def provider_tool_names(provider: str, body: dict) -> set[str]:
    tools = body.get("tools") or []
    if provider in {"openai", "deepseek", "xai"}:
        return {
            str((tool.get("function") or {}).get("name"))
            for tool in tools
            if (tool.get("function") or {}).get("name")
        }
    return {str(tool.get("name")) for tool in tools if tool.get("name")}


def main() -> None:
    print(f"CKB Universal Model Gateway smoke test: {BASE_URL}")

    status, _headers, capabilities = request_json(f"{BASE_URL}/llm/capabilities")
    if status != 200:
        fail(f"capabilities returned HTTP {status}: {capabilities}")
    if capabilities.get("service") != "CKB Universal Model Gateway":
        fail(f"unexpected gateway service: {capabilities.get('service')!r}")
    if (capabilities.get("native_mcp") or {}).get("endpoint") != "/mcp":
        fail("capabilities does not advertise canonical /mcp endpoint")
    if (capabilities.get("function_tool_adapter") or {}).get("call_endpoint") != "/llm/call":
        fail("capabilities does not advertise /llm/call")
    if capabilities.get("tool_count") != len(EXPECTED_TOOLS):
        fail("capabilities tool_count mismatch")
    print("PASS: universal capability discovery")

    expected_formats = {
        "openai": "openai-compatible-function-tools",
        "deepseek": "openai-compatible-function-tools",
        "xai": "openai-compatible-function-tools",
        "anthropic": "anthropic-tools",
        "gemini": "gemini-interactions-functions",
        "generic": "json-schema-functions",
        "mcp": "mcp-tools-list",
    }
    for provider, expected_format in expected_formats.items():
        status, _headers, body = request_json(
            f"{BASE_URL}/llm/tools?provider={provider}"
        )
        if status != 200:
            fail(f"{provider} tool discovery returned HTTP {status}: {body}")
        if body.get("format") != expected_format:
            fail(
                f"{provider} format mismatch: expected {expected_format!r}, "
                f"got {body.get('format')!r}"
            )
        names = provider_tool_names(provider, body)
        if names != EXPECTED_TOOLS:
            fail(
                f"{provider} tool inventory mismatch; "
                f"missing={sorted(EXPECTED_TOOLS - names)}, "
                f"extra={sorted(names - EXPECTED_TOOLS)}"
            )
        print(f"PASS: {provider} exposes all {len(EXPECTED_TOOLS)} CKB tools")

    status, headers, body = request_json(
        f"{BASE_URL}/llm/call",
        payload={
            "provider": "deepseek",
            "name": "ckb_get_architecture_graph",
            "arguments": {"project_id": "smoke_test_never_executed"},
        },
    )
    if status != 401:
        fail(f"unauthenticated /llm/call expected 401, got {status}: {body}")
    if body.get("error") != "authentication_required":
        fail(f"unexpected unauthenticated error payload: {body}")
    authenticate = headers.get("www-authenticate")
    if not authenticate or "resource_metadata=" not in authenticate:
        fail("unauthenticated /llm/call did not forward the MCP OAuth challenge")
    print("PASS: function adapter preserves MCP OAuth authorization boundary")

    print("PASS: CKB Universal Model Gateway public smoke suite completed")


if __name__ == "__main__":
    main()
