export interface CkbClientOptions {
  baseUrl?: string;
  apiKey?: string;
  timeoutMs?: number;
}

export interface AnalyzeImpactParams {
  path: string;
  file: string;
  line: number;
  changeType?: 'modify' | 'delete' | 'rename';
  repoName?: string;
}

export class CkbApiError extends Error {
  status?: number;
  body?: unknown;
}

export class CkbClient {
  constructor(options?: CkbClientOptions);
  health(): Promise<string>;
  scan(path: string, repoName?: string): Promise<unknown>;
  listFederatedRepos(): Promise<unknown>;
  getReport(repoName?: string): Promise<unknown>;
  analyzeImpact(params: AnalyzeImpactParams): Promise<unknown>;
  search(query: string, repoName?: string): Promise<unknown>;
  detectClones(path: string): Promise<unknown>;
  analyzeSessionImpact(changes: Array<{ file: string; line: number; changeType?: string }>, repoName?: string): Promise<unknown>;
  explainViolation(violation: unknown): Promise<{ explanation: string; suggested_fix: string; model_used: string }>;
  ask(question: string, repoName?: string): Promise<{ answer: string }>;
  getDriftTimeline(): Promise<unknown>;
  getTestGaps(repoName?: string): Promise<unknown>;
  generateRules(repoName?: string): Promise<unknown>;
  getOrgAnalytics(): Promise<unknown>;
  getIntelligenceMetrics(): Promise<unknown>;
}
