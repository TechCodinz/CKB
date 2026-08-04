'use strict';

/**
 * @ckb/sdk — a thin Node.js HTTP client for the CKB MCP REST server.
 *
 * Requires Node >= 18 for the built-in `fetch`. No native/N-API build step —
 * this talks to `ckb-mcp-server` (see `mcp-server/`) over HTTP, the same way
 * the dashboard and VS Code extension do.
 *
 * Usage:
 *   const { CkbClient } = require('@ckb/sdk');
 *   const client = new CkbClient({ baseUrl: 'http://localhost:3000', apiKey: process.env.CKB_API_KEY });
 *   const report = await client.scan('/path/to/repo');
 */

class CkbApiError extends Error {
  constructor(message, status, body) {
    super(message);
    this.name = 'CkbApiError';
    this.status = status;
    this.body = body;
  }
}

class CkbClient {
  /**
   * @param {Object} [options]
   * @param {string} [options.baseUrl] - Base URL of the ckb-mcp-server REST API. Defaults to http://localhost:3000.
   * @param {string} [options.apiKey] - API key, if the server was started with CKB_API_KEY set.
   * @param {number} [options.timeoutMs] - Request timeout in milliseconds. Defaults to 60000.
   */
  constructor(options = {}) {
    this.baseUrl = (options.baseUrl || 'http://localhost:3000').replace(/\/+$/, '');
    this.apiKey = options.apiKey;
    this.timeoutMs = options.timeoutMs || 60000;
  }

  async _request(method, path, body) {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), this.timeoutMs);

    const headers = { 'Content-Type': 'application/json' };
    if (this.apiKey) headers['X-API-Key'] = this.apiKey;

    let res;
    try {
      res = await fetch(`${this.baseUrl}${path}`, {
        method,
        headers,
        body: body !== undefined ? JSON.stringify(body) : undefined,
        signal: controller.signal,
      });
    } catch (err) {
      if (err.name === 'AbortError') {
        throw new CkbApiError(`Request to ${path} timed out after ${this.timeoutMs}ms`, undefined, undefined);
      }
      throw new CkbApiError(`Failed to reach CKB server at ${this.baseUrl}: ${err.message}`, undefined, undefined);
    } finally {
      clearTimeout(timer);
    }

    const text = await res.text();
    let parsed;
    try {
      parsed = text ? JSON.parse(text) : undefined;
    } catch {
      parsed = text;
    }

    if (!res.ok) {
      const message = (parsed && parsed.message) || `CKB server returned ${res.status} for ${method} ${path}`;
      throw new CkbApiError(message, res.status, parsed);
    }

    return parsed;
  }

  /** Health check. Resolves if the server is reachable and responding. */
  async health() {
    return this._request('GET', '/health');
  }

  /** Scan a codebase path. Pass repoName to register this scan for multi-repo federation/org analytics. */
  async scan(path, repoName) {
    return this._request('POST', '/api/v1/scan', { path, repo_name: repoName });
  }

  /** List repos currently registered for multi-repo federation (via scan(path, repoName)). */
  async listFederatedRepos() {
    return this._request('GET', '/api/v1/federation/repos');
  }

  /** Fetch the most recent scan report from the server (404s if no scan has run yet). */
  async getReport(repoName) {
    const qs = repoName ? `?repo=${encodeURIComponent(repoName)}` : '';
    return this._request('GET', `/api/v1/report${qs}`);
  }

  /**
   * Analyze the blast radius of a change.
   * @param {Object} params
   * @param {string} params.path - Codebase root path (scanned if not already cached).
   * @param {string} params.file - File being changed.
   * @param {number} params.line - Line number of the change.
   * @param {string} [params.changeType='modify'] - One of 'modify' | 'delete' | 'rename', per the server's ChangeType.
   * @param {string} [params.repoName] - Isolates this call to a named session/repo (see scan()). Omit for the server's default shared session.
   */
  async analyzeImpact({ path, file, line, changeType = 'modify', repoName }) {
    return this._request('POST', '/api/v1/impact', { path, file, line, change_type: changeType, repo_name: repoName });
  }

  /** Search the most recent scan's detected patterns for a text query. */
  async search(query, repoName) {
    return this._request('POST', '/api/v1/search', { query, repo_name: repoName });
  }

  /** Detect duplicate/near-duplicate code (semantic clones) under a path. */
  async detectClones(path) {
    return this._request('POST', '/api/v1/clones', { path });
  }

  /**
   * Aggregate blast-radius across multiple changes in one call — e.g. every
   * edit an AI coding agent made in a session — instead of calling
   * analyzeImpact() once per file and merging results yourself.
   * @param {Array<{file: string, line: number, changeType?: string}>} changes
   * @param {string} [repoName] - Isolates this call to a named session/repo (see scan()).
   */
  async analyzeSessionImpact(changes, repoName) {
    const body = {
      changes: changes.map(c => ({ file: c.file, line: c.line, change_type: c.changeType || 'modify' })),
      repo_name: repoName,
    };
    return this._request('POST', '/api/v1/session-impact', body);
  }

  /**
   * Explain a single violation in plain language and get a suggested fix,
   * via an LLM on the server side. Requires the server to have
   * ANTHROPIC_API_KEY configured. Pass the exact violation object as
   * returned in a scan report's `drift` list.
   */
  async explainViolation(violation) {
    return this._request('POST', '/api/v1/violations/explain', { violation });
  }

  /**
   * Ask a natural-language question about the most recently scanned codebase.
   * Keyword-retrieval based, not full semantic search — see ask.rs server-side
   * for the scope note. Requires the server to have ANTHROPIC_API_KEY set and
   * at least one prior scan() call.
   * @param {string} [repoName] - Ask against a named session/repo (see scan()) instead of the default.
   */
  async ask(question, repoName) {
    return this._request('POST', '/api/v1/ask', { question, repo_name: repoName });
  }

  /** Fetch drift timeline history (git-based architectural drift over time). */
  async getDriftTimeline() {
    return this._request('GET', '/api/v1/drift-timeline');
  }

  /** Fetch untested-hotpath / test coverage gap analysis for the last scan. */
  async getTestGaps(repoName) {
    const qs = repoName ? `?repo=${encodeURIComponent(repoName)}` : '';
    return this._request('GET', `/api/v1/test-gaps${qs}`);
  }

  /** Generate suggested architecture rules from the current graph. */
  async generateRules(repoName) {
    const qs = repoName ? `?repo=${encodeURIComponent(repoName)}` : '';
    return this._request('GET', `/api/v1/rules${qs}`);
  }

  /** Org-level analytics (multi-project rollups), if configured on the server. */
  async getOrgAnalytics() {
    return this._request('GET', '/api/v1/org/analytics');
  }

  /** Aggregate intelligence metrics for the current scan/graph. */
  async getIntelligenceMetrics() {
    return this._request('GET', '/api/v1/metrics/intelligence');
  }
}

module.exports = { CkbClient, CkbApiError };
