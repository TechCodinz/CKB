#!/usr/bin/env python3
"""Production-safe smoke test for the CKB ChatGPT/Codex remote MCP surface.

This script intentionally tests only public discovery and the unauthenticated
OAuth challenge. It never needs or prints a CKB user token or infrastructure
secret.

Usage:
    python scripts/smoke_chatgpt_mcp.py
    CKB_MCP_BASE_URL=https://preview.example.com python scripts/smoke_chatgpt_mcp.py
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
MCP_URL = f"{BASE_URL}/mcp"
RESOURCE_METADATA_URL = f"{BASE_URL}/.well-known/oauth-protected-resource"
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
            return response.status, dict(response.headers.items()), json.loads(raw or "{}")
    except urllib.error.HTTPError as error:
        raw = error.read().decode("utf-8")
        try:
            body = json.loads(raw or "{}")
        except json.JSONDecodeError:
            body = {"raw": raw}
        return error.code, dict(error.headers.items()), body


def rpc(method: str, params: dict | None = None, request_id: int = 1) -> dict:
    payload = {"jsonrpc": "2.0", "id": request_id, "method": method}
    if params is not None:
        payload["params"] = params
    status, _headers, body = request_json(MCP_URL, payload=payload)
    if status != 200:
        fail(f"{method} returned HTTP {status}: {body}")
    if body.get("jsonrpc") != "2.0" or body.get("id") != request_id:
        fail(f"{method} returned an invalid JSON-RPC envelope: {body}")
    return body


def main() -> None:
    print(f"CKB MCP smoke test: {BASE_URL}")

    status, _headers, metadata = request_json(RESOURCE_METADATA_URL)
    if status != 200:
        fail(f"protected-resource metadata returned HTTP {status}: {metadata}")
    if metadata.get("resource") != BASE_URL:
        fail(
            "protected-resource metadata resource mismatch: "
            f"expected {BASE_URL!r}, got {metadata.get('resource')!r}"
        )
    authorization_servers = metadata.get("authorization_servers") or []
    if not authorization_servers:
        fail("protected-resource metadata has no authorization_servers")
    scopes = set(metadata.get("scopes_supported") or [])
    for scope in {"architecture:read", "repository:scan", "offline_access"}:
        if scope not in scopes:
            fail(f"protected-resource metadata is missing scope {scope!r}")
    print("PASS: RFC 9728 protected-resource metadata")

    auth_issuer = str(authorization_servers[0]).rstrip("/")
    status, _headers, auth_metadata = request_json(
        f"{auth_issuer}/.well-known/oauth-authorization-server"
    )
    if status != 200:
        fail(f"OAuth discovery returned HTTP {status}: {auth_metadata}")
    if auth_metadata.get("issuer", "").rstrip("/") != auth_issuer:
        fail("OAuth discovery issuer does not match protected-resource metadata")
    if "S256" not in (auth_metadata.get("code_challenge_methods_supported") or []):
        fail("OAuth discovery does not advertise PKCE S256")
    if "authorization_code" not in (auth_metadata.get("grant_types_supported") or []):
        fail("OAuth discovery does not advertise authorization_code")
    if not auth_metadata.get("authorization_endpoint") or not auth_metadata.get("token_endpoint"):
        fail("OAuth discovery is missing authorization/token endpoints")
    print("PASS: OAuth authorization-server discovery + PKCE")

    initialized = rpc(
        "initialize",
        {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "ckb-smoke-test", "version": "1.0"},
        },
        request_id=2,
    )
    result = initialized.get("result") or {}
    if (result.get("serverInfo") or {}).get("name") != "ckb-chatgpt-mcp":
        fail(f"unexpected MCP serverInfo: {result.get('serverInfo')}")
    if not (result.get("capabilities") or {}).get("tools"):
        fail("MCP initialize did not advertise tools capability")
    print("PASS: MCP initialize")

    listed = rpc("tools/list", {}, request_id=3)
    tools = (listed.get("result") or {}).get("tools") or []
    names = {tool.get("name") for tool in tools}
    missing = EXPECTED_TOOLS - names
    extra = names - EXPECTED_TOOLS
    if missing or extra:
        fail(f"tool inventory mismatch; missing={sorted(missing)}, extra={sorted(extra)}")
    for tool in tools:
        schemes = tool.get("securitySchemes") or []
        if not schemes or schemes[0].get("type") != "oauth2":
            fail(f"tool {tool.get('name')} is missing OAuth securitySchemes")
    print(f"PASS: tools/list exposes exactly {len(EXPECTED_TOOLS)} scoped CKB tools")

    challenge = rpc(
        "tools/call",
        {
            "name": "ckb_get_architecture_graph",
            "arguments": {"project_id": "smoke-test-never-executed"},
        },
        request_id=4,
    )
    result = challenge.get("result") or {}
    if result.get("isError") is not True:
        fail("unauthenticated protected tool did not return an MCP error result")
    meta = result.get("_meta") or {}
    challenges = meta.get("mcp/www_authenticate") or []
    if not challenges:
        fail("unauthenticated tool result is missing mcp/www_authenticate")
    serialized = " ".join(map(str, challenges))
    if "resource_metadata=" not in serialized or "error=" not in serialized or "error_description=" not in serialized:
        fail("OAuth challenge is missing required account-linking parameters")
    print("PASS: unauthenticated tool triggers OAuth account-linking challenge")

    print("PASS: CKB remote MCP public smoke suite completed")


if __name__ == "__main__":
    main()
