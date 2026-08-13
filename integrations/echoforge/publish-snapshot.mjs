#!/usr/bin/env node

/**
 * CKB → EchoForge Sentinel bridge publisher.
 *
 * Queries CKB's authenticated REST intelligence endpoints and publishes one
 * idempotent architecture snapshot to the mapped EchoForge Sentinel project.
 * Designed for a private worker, CI post-scan step, cron job, or deployment hook.
 */

const protocol = "ckb-echoforge/v1";
const ckbUrl = required("CKB_BRIDGE_SOURCE_URL").replace(/\/$/, "");
const ckbKey = required("CKB_BRIDGE_SOURCE_API_KEY");
const echoUrl = required("ECHOFORGE_SENTINEL_URL").replace(/\/$/, "");
const echoKey = required("ECHOFORGE_SENTINEL_PROJECT_KEY");
const repoName = required("CKB_REPO_NAME");
const commitSha = String(process.env.CKB_REPO_COMMIT || "current").trim() || "current";
const timeoutMs = clamp(Number(process.env.CKB_ECHOFORGE_TIMEOUT_MS || 5000), 1000, 15000);

function required(name) {
  const value = String(process.env[name] || "").trim();
  if (!value) throw new Error(`${name} is required`);
  return value;
}

function clamp(value, min, max) {
  return Math.max(min, Math.min(max, Number.isFinite(value) ? value : min));
}

function assertSafeBase(raw, label) {
  const url = new URL(raw);
  const local = ["localhost", "127.0.0.1", "::1"].includes(url.hostname);
  if (url.protocol !== "https:" && !local) throw new Error(`${label} must use HTTPS outside local development`);
}

assertSafeBase(ckbUrl, "CKB_BRIDGE_SOURCE_URL");
assertSafeBase(echoUrl, "ECHOFORGE_SENTINEL_URL");

async function fetchJson(url, options = {}) {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    const response = await fetch(url, { ...options, signal: controller.signal, cache: "no-store" });
    const text = await response.text();
    let body = null;
    try { body = text ? JSON.parse(text) : null; } catch { body = text; }
    if (!response.ok) throw new Error(`${url}: HTTP ${response.status}${typeof body === "string" ? ` · ${body.slice(0, 180)}` : ""}`);
    return body;
  } finally {
    clearTimeout(timer);
  }
}

function normalizeNumber(value) {
  if (typeof value !== "number" || !Number.isFinite(value)) return 0;
  if (value >= 0 && value <= 1) return value;
  if (value > 1 && value <= 100) return value / 100;
  return 0;
}

function maxRisk(value, depth = 0) {
  if (depth > 4) return 0;
  if (typeof value === "number") return normalizeNumber(value);
  if (Array.isArray(value)) return Math.max(0, ...value.slice(0, 100).map((item) => maxRisk(item, depth + 1)));
  if (!value || typeof value !== "object") return 0;
  const preferred = ["failure_probability", "failureProbability", "risk_score", "riskScore", "severity_score", "severityScore", "risk", "score", "confidence"];
  let max = 0;
  for (const key of preferred) if (key in value) max = Math.max(max, maxRisk(value[key], depth + 1));
  if (max) return clamp(max, 0, 1);
  for (const item of Object.values(value).slice(0, 100)) max = Math.max(max, maxRisk(item, depth + 1));
  return clamp(max, 0, 1);
}

function countFindings(value) {
  if (Array.isArray(value)) return value.length;
  if (!value || typeof value !== "object") return 0;
  for (const key of ["gaps", "violations", "items", "results", "entries", "repos"]) {
    if (Array.isArray(value[key])) return value[key].length;
  }
  return Object.keys(value).length;
}

function severity(score) {
  if (score >= 0.9) return "critical";
  if (score >= 0.75) return "high";
  if (score >= 0.5) return "medium";
  return score > 0 ? "low" : "info";
}

async function ckb(path) {
  const separator = path.includes("?") ? "&" : "?";
  return fetchJson(`${ckbUrl}${path}${separator}repo_name=${encodeURIComponent(repoName)}`, {
    headers: { "x-api-key": ckbKey, "x-ckb-bridge-protocol": protocol },
  });
}

const results = await Promise.allSettled([
  ckb("/api/v1/metrics/intelligence"),
  ckb("/api/v1/test-gaps"),
  ckb("/api/v1/drift-timeline"),
  ckb("/api/v1/report"),
]);

const names = ["metrics", "testGaps", "driftTimeline", "report"];
const evidence = Object.fromEntries(results.map((result, index) => [names[index], result.status === "fulfilled" ? result.value : null]));
const errors = results.flatMap((result, index) => result.status === "rejected" ? [`${names[index]}: ${result.reason?.message || "unavailable"}`] : []);

const metricRisk = maxRisk(evidence.metrics);
const reportRisk = maxRisk(evidence.report);
const testGapCount = countFindings(evidence.testGaps);
const driftCount = countFindings(evidence.driftTimeline);

const signals = [
  {
    name: "ckb.failure_risk",
    score: Math.max(metricRisk, reportRisk),
    value: Math.max(metricRisk, reportRisk),
    tags: ["ckb", "architecture-intelligence", "failure-risk"],
    metadata: { severity: severity(Math.max(metricRisk, reportRisk)), repo: repoName, commit_sha: commitSha },
  },
  {
    name: "ckb.test_gap",
    score: clamp(testGapCount / 20, 0, 1),
    value: testGapCount,
    tags: ["ckb", "architecture-intelligence", "test-gap"],
    metadata: { finding_count: testGapCount, repo: repoName, commit_sha: commitSha },
  },
  {
    name: "ckb.architecture_drift",
    score: clamp(driftCount / 20, 0, 1),
    value: driftCount,
    tags: ["ckb", "architecture-intelligence", "drift"],
    metadata: { finding_count: driftCount, repo: repoName, commit_sha: commitSha },
  },
].filter((signal) => signal.score > 0 || signal.value > 0);

if (!signals.length) {
  signals.push({
    name: "ckb.architecture_health",
    score: 0,
    value: 0,
    tags: ["ckb", "architecture-intelligence", "healthy-snapshot"],
    metadata: { repo: repoName, commit_sha: commitSha },
  });
}

const sourceIncidentId = `ckb:snapshot:${repoName}:${commitSha}`.slice(0, 240);
const payload = {
  domain: "infrastructure",
  context: {
    source: "ckb",
    sourceIncidentId,
    repo: repoName,
    commit_sha: commitSha,
    bridge_protocol: protocol,
    snapshot: true,
    endpoint_status: Object.fromEntries(results.map((result, index) => [names[index], result.status])),
    errors: errors.slice(0, 10),
  },
  signals: signals.map((signal, index) => ({
    id: `${sourceIncidentId}:${signal.name}:${index}`.slice(0, 160),
    ...signal,
    timestamp: new Date().toISOString(),
    detector: "ckb",
    domain: "infrastructure",
    entity: repoName,
    service: "ckb-architecture",
  })),
};

const response = await fetchJson(`${echoUrl}/api/sentinel/ingest`, {
  method: "POST",
  headers: {
    "content-type": "application/json",
    "x-sentinel-key": echoKey,
    "x-ckb-bridge-protocol": protocol,
  },
  body: JSON.stringify(payload),
});

console.log(JSON.stringify({
  ok: true,
  protocol,
  repoName,
  commitSha,
  signalsPublished: signals.length,
  incidentId: response?.incident?.id || null,
  codeIntelligence: response?.incident?.codeIntelligence || null,
  sourceErrors: errors,
}, null, 2));
