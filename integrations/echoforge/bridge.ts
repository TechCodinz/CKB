export const CKB_ECHOFORGE_PROTOCOL = "ckb-echoforge/v1" as const;

export type EchoForgeSeverity = "info" | "low" | "medium" | "high" | "critical";

export interface BridgeFileEvidence {
  path: string;
  line?: number;
  symbol?: string;
}

export interface EchoForgeIncidentEnvelope {
  protocol: typeof CKB_ECHOFORGE_PROTOCOL;
  project: {
    external_id: string;
    repo_name: string;
    commit_sha?: string;
  };
  incident: {
    id: string;
    title: string;
    severity: EchoForgeSeverity;
    timestamp: string;
  };
  evidence: {
    files?: BridgeFileEvidence[];
    services?: string[];
    trace_ids?: string[];
    deployment_ids?: string[];
  };
}

export interface CkbRuntimeSignal {
  name:
    | "ckb.failure_risk"
    | "ckb.blast_radius"
    | "ckb.architecture_drift"
    | "ckb.test_gap"
    | "ckb.hotpath_latency"
    | "ckb.semantic_clone_risk";
  /** Normalized risk/evidence strength. Values outside 0..1 are clamped. */
  value: number;
  severity?: EchoForgeSeverity;
  timestamp?: string;
  service?: string;
  entity?: string;
  tags?: string[];
  attributes?: Record<string, string | number | boolean | null>;
}

export interface EchoForgeBridgeOptions {
  baseUrl: string;
  projectKey: string;
  timeoutMs?: number;
  fetchImpl?: typeof fetch;
}

function normalizedBaseUrl(value: string): string {
  const url = new URL(value);
  if (url.protocol !== "https:" && url.hostname !== "localhost" && url.hostname !== "127.0.0.1") {
    throw new Error("EchoForge bridge requires HTTPS outside local development");
  }
  return url.toString().replace(/\/$/, "");
}

function clampScore(value: number): number {
  return Math.max(0, Math.min(1, Number.isFinite(value) ? value : 0));
}

function severityFromScore(score: number): EchoForgeSeverity {
  if (score >= 0.9) return "critical";
  if (score >= 0.75) return "high";
  if (score >= 0.5) return "medium";
  if (score > 0) return "low";
  return "info";
}

export class EchoForgeBridgeClient {
  private readonly baseUrl: string;
  private readonly projectKey: string;
  private readonly timeoutMs: number;
  private readonly fetchImpl: typeof fetch;

  constructor(options: EchoForgeBridgeOptions) {
    if (!options.projectKey.trim()) throw new Error("EchoForge project key is required");
    this.baseUrl = normalizedBaseUrl(options.baseUrl);
    this.projectKey = options.projectKey;
    this.timeoutMs = options.timeoutMs ?? 5_000;
    this.fetchImpl = options.fetchImpl ?? fetch;
  }

  async publishArchitectureSignals(input: {
    sourceIncidentId: string;
    repoName: string;
    commitSha?: string;
    signals: CkbRuntimeSignal[];
  }): Promise<{ ok: boolean; incidentId?: string; status: number }> {
    if (!input.signals.length) return { ok: true, status: 204 };

    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), this.timeoutMs);
    try {
      const response = await this.fetchImpl(`${this.baseUrl}/api/sentinel/ingest`, {
        method: "POST",
        headers: {
          "content-type": "application/json",
          "x-sentinel-key": this.projectKey,
          "x-ckb-bridge-protocol": CKB_ECHOFORGE_PROTOCOL,
        },
        body: JSON.stringify({
          domain: "infrastructure",
          context: {
            source: "ckb",
            sourceIncidentId: input.sourceIncidentId,
            repo: input.repoName,
            ...(input.commitSha ? { commit_sha: input.commitSha } : {}),
            bridge_protocol: CKB_ECHOFORGE_PROTOCOL,
          },
          signals: input.signals.slice(0, 250).map((signal, index) => {
            const score = clampScore(signal.value);
            return {
              id: `${input.sourceIncidentId}:${signal.name}:${index}`.slice(0, 160),
              name: signal.name,
              score,
              value: signal.value,
              timestamp: signal.timestamp ?? new Date().toISOString(),
              detector: "ckb",
              domain: "infrastructure",
              entity: signal.entity ?? input.repoName,
              service: signal.service ?? "ckb-architecture",
              tags: ["ckb", "architecture-intelligence", ...(signal.tags ?? [])].slice(0, 50),
              metadata: {
                ...(signal.attributes ?? {}),
                repo: input.repoName,
                severity: signal.severity ?? severityFromScore(score),
                ...(input.commitSha ? { commit_sha: input.commitSha } : {}),
                source_incident_id: input.sourceIncidentId,
                bridge_protocol: CKB_ECHOFORGE_PROTOCOL,
              },
            };
          }),
        }),
        signal: controller.signal,
      });

      const body = response.headers.get("content-type")?.includes("application/json")
        ? await response.json().catch(() => ({}))
        : {};
      return { ok: response.ok, status: response.status, incidentId: body?.incident?.id ?? body?.incidentId };
    } finally {
      clearTimeout(timeout);
    }
  }
}
