"""
A thin Python HTTP client for the CKB MCP REST server (`ckb-mcp-server`).

No native extension / compiled bindings here — this talks to the server over
HTTP the same way the dashboard and CLI's `--stdio` sibling REST mode do.
Only depends on the standard library (`urllib`), so it works without pip
installing anything extra.
"""

import json
import urllib.error
import urllib.parse
import urllib.request
from typing import Any, Dict, Optional


class CkbApiError(Exception):
    """Raised when the CKB server returns a non-2xx response, or is unreachable."""

    def __init__(self, message: str, status: Optional[int] = None, body: Any = None):
        super().__init__(message)
        self.status = status
        self.body = body


class CkbClient:
    def __init__(self, base_url: str = "http://localhost:3000", api_key: Optional[str] = None,
                 timeout: float = 60.0):
        """
        :param base_url: Base URL of the ckb-mcp-server REST API.
        :param api_key: API key, if the server was started with CKB_API_KEY set.
        :param timeout: Request timeout in seconds.
        """
        self.base_url = base_url.rstrip("/")
        self.api_key = api_key
        self.timeout = timeout

    def _request(self, method: str, path: str, body: Optional[Dict[str, Any]] = None) -> Any:
        url = f"{self.base_url}{path}"
        data = json.dumps(body).encode("utf-8") if body is not None else None

        headers = {"Content-Type": "application/json"}
        if self.api_key:
            headers["X-API-Key"] = self.api_key

        req = urllib.request.Request(url, data=data, headers=headers, method=method)

        try:
            with urllib.request.urlopen(req, timeout=self.timeout) as resp:
                raw = resp.read().decode("utf-8")
                return json.loads(raw) if raw else None
        except urllib.error.HTTPError as e:
            raw = e.read().decode("utf-8", errors="replace")
            try:
                parsed = json.loads(raw) if raw else None
            except json.JSONDecodeError:
                parsed = raw
            message = (parsed.get("message") if isinstance(parsed, dict) else None) \
                or f"CKB server returned {e.code} for {method} {path}"
            raise CkbApiError(message, status=e.code, body=parsed) from e
        except urllib.error.URLError as e:
            raise CkbApiError(f"Failed to reach CKB server at {self.base_url}: {e.reason}") from e

    def health(self) -> str:
        """Health check. Returns the raw "OK" string if the server is reachable."""
        return self._request("GET", "/health")

    def scan(self, path: str, repo_name: Optional[str] = None) -> Dict[str, Any]:
        """Scan a codebase path. Pass repo_name to register this scan for multi-repo federation/org analytics."""
        return self._request("POST", "/api/v1/scan", {"path": path, "repo_name": repo_name})

    def list_federated_repos(self) -> Any:
        """List repos currently registered for multi-repo federation (via scan(path, repo_name))."""
        return self._request("GET", "/api/v1/federation/repos")

    def get_report(self, repo_name: Optional[str] = None) -> Dict[str, Any]:
        """Fetch the most recent scan report (raises CkbApiError with status=404 if none exists)."""
        qs = f"?repo={urllib.parse.quote(repo_name)}" if repo_name else ""
        return self._request("GET", f"/api/v1/report{qs}")

    def analyze_impact(self, path: str, file: str, line: int, change_type: str = "modify",
                        repo_name: Optional[str] = None) -> Dict[str, Any]:
        """Analyze the blast radius of a change. change_type is one of 'modify' | 'delete' | 'rename'."""
        return self._request("POST", "/api/v1/impact", {
            "path": path, "file": file, "line": line, "change_type": change_type, "repo_name": repo_name,
        })

    def search(self, query: str, repo_name: Optional[str] = None) -> Dict[str, Any]:
        """Search the most recent scan's detected patterns for a text query."""
        return self._request("POST", "/api/v1/search", {"query": query, "repo_name": repo_name})

    def detect_clones(self, path: str) -> Dict[str, Any]:
        """Detect duplicate/near-duplicate code (semantic clones) under a path."""
        return self._request("POST", "/api/v1/clones", {"path": path})

    def analyze_session_impact(self, changes: list, repo_name: Optional[str] = None) -> Dict[str, Any]:
        """
        Aggregate blast-radius across multiple changes in one call — e.g. every
        edit an AI coding agent made in a session.
        :param changes: list of dicts like {"file": str, "line": int, "change_type": "modify"}
        :param repo_name: isolates this call to a named session/repo (see scan()).
        """
        return self._request("POST", "/api/v1/session-impact", {"changes": changes, "repo_name": repo_name})

    def explain_violation(self, violation: Dict[str, Any]) -> Dict[str, Any]:
        """
        Explain a single violation in plain language and get a suggested fix,
        via an LLM on the server side. Requires the server to have
        ANTHROPIC_API_KEY configured. Pass the exact violation object as
        returned in a scan report's `drift` list.
        """
        return self._request("POST", "/api/v1/violations/explain", {"violation": violation})

    def ask(self, question: str, repo_name: Optional[str] = None) -> Dict[str, Any]:
        """
        Ask a natural-language question about the most recently scanned
        codebase. Keyword-retrieval based, not full semantic search. Requires
        ANTHROPIC_API_KEY and at least one prior scan() call on the server.
        :param repo_name: ask against a named session/repo (see scan()) instead of the default.
        """
        return self._request("POST", "/api/v1/ask", {"question": question, "repo_name": repo_name})

    def get_drift_timeline(self) -> Any:
        """Fetch drift timeline history (git-based architectural drift over time)."""
        return self._request("GET", "/api/v1/drift-timeline")

    def get_test_gaps(self, repo_name: Optional[str] = None) -> Any:
        """Fetch untested-hotpath / test coverage gap analysis for the last scan."""
        qs = f"?repo={urllib.parse.quote(repo_name)}" if repo_name else ""
        return self._request("GET", f"/api/v1/test-gaps{qs}")

    def generate_rules(self, repo_name: Optional[str] = None) -> Any:
        """Generate suggested architecture rules from the current graph."""
        qs = f"?repo={urllib.parse.quote(repo_name)}" if repo_name else ""
        return self._request("GET", f"/api/v1/rules{qs}")

    def get_org_analytics(self) -> Any:
        """Org-level analytics (multi-project rollups), if configured on the server."""
        return self._request("GET", "/api/v1/org/analytics")

    def get_intelligence_metrics(self) -> Any:
        """Aggregate intelligence metrics for the current scan/graph."""
        return self._request("GET", "/api/v1/metrics/intelligence")
