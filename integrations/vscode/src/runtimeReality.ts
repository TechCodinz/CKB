import * as vscode from 'vscode';
import * as path from 'path';

export type RuntimeFlowType = 'http' | 'database' | 'cache' | 'queue' | 'event' | 'websocket' | 'function' | 'other';

export type RuntimeTraceStep = {
    traceId?: string;
    spanId?: string;
    parentSpanId?: string;
    source?: string;
    target?: string;
    operation?: string;
    flowType?: string;
    flowDirection?: string;
    protocol?: string;
    dbSystem?: string;
    messagingSystem?: string;
    durationMs?: number;
    error?: boolean;
    observedAt?: string;
};

export type RuntimeRealityFeed = {
    online: boolean;
    observed: boolean;
    replaySafe: boolean;
    traceSemantics: string;
    runtimeNodes: number;
    traces: Record<string, RuntimeTraceStep[]>;
    flowCounts: Record<RuntimeFlowType, number>;
    updatedAt: string;
    projectHint: string;
    error?: string;
};

const FLOW_TYPES: RuntimeFlowType[] = ['http', 'database', 'cache', 'queue', 'event', 'websocket', 'function', 'other'];

function emptyCounts(): Record<RuntimeFlowType, number> {
    return Object.fromEntries(FLOW_TYPES.map(type => [type, 0])) as Record<RuntimeFlowType, number>;
}

function normalizeFlowType(step: RuntimeTraceStep): RuntimeFlowType {
    const explicit = String(step.flowType || '').toLowerCase();
    const context = `${explicit} ${step.operation || ''} ${step.protocol || ''} ${step.dbSystem || ''} ${step.messagingSystem || ''}`.toLowerCase();
    if (/websocket|\bws\b|\bwss\b/.test(context)) return 'websocket';
    if (/redis|cache/.test(context)) return 'cache';
    if (/postgres|mysql|sqlite|mongo|prisma|database|\bsql\b/.test(context)) return 'database';
    if (/queue|kafka|rabbit|bull|sqs|pubsub|message/.test(context)) return 'queue';
    if (/event/.test(context)) return 'event';
    if (/http|rpc|fetch|request|response/.test(context)) return 'http';
    if (/function|call|internal|handler|method/.test(context)) return 'function';
    return 'other';
}

function requestJson(url: URL, apiKey: string, timeoutMs: number): Promise<any> {
    return new Promise((resolve, reject) => {
        const transport = url.protocol === 'https:' ? require('https') : require('http');
        const headers: Record<string, string> = { Accept: 'application/json' };
        if (apiKey) headers['X-API-Key'] = apiKey;
        const request = transport.request(url, { method: 'GET', headers }, (response: any) => {
            let raw = '';
            response.on('data', (chunk: any) => raw += chunk);
            response.on('end', () => {
                let parsed: any = raw;
                try { parsed = raw ? JSON.parse(raw) : {}; } catch { /* preserve raw */ }
                if (Number(response.statusCode || 500) >= 400) {
                    reject(new Error(parsed?.message || parsed || `CKB runtime server returned HTTP ${response.statusCode}`));
                    return;
                }
                resolve(parsed || {});
            });
        });
        request.on('error', reject);
        request.setTimeout(timeoutMs, () => request.destroy(new Error('CKB runtime request timed out')));
        request.end();
    });
}

function endpoint(baseUrl: string, route: string, projectId: string) {
    const url = new URL(`${baseUrl.replace(/\/$/, '')}${route}`);
    if (projectId) url.searchParams.set('project_id', projectId);
    return url;
}

/**
 * Retrieves runtime truth from the configured CKB Reality server without
 * uploading local source. When `ckb.runtimeProjectId` is blank the server's
 * current/default project scope is used, which matches local CKB server usage.
 */
export async function fetchRuntimeReality(workspaceRoot: string): Promise<RuntimeRealityFeed> {
    const config = vscode.workspace.getConfiguration('ckb');
    const baseUrl = config.get<string>('serverUrl', 'http://localhost:3000').trim() || 'http://localhost:3000';
    const apiKey = config.get<string>('apiKey', '').trim();
    const configuredProject = config.get<string>('runtimeProjectId', '').trim();
    const projectHint = configuredProject || path.basename(workspaceRoot || '') || 'current';
    const timeoutMs = Math.max(3_000, Math.min(config.get<number>('runtimeRequestTimeoutMs', 8_000), 30_000));

    try {
        const [traceResult, runtimeResult] = await Promise.allSettled([
            requestJson(endpoint(baseUrl, '/api/v1/intelligence/traces', configuredProject), apiKey, timeoutMs),
            requestJson(endpoint(baseUrl, '/api/v1/intelligence/runtime', configuredProject), apiKey, timeoutMs),
        ]);
        if (traceResult.status === 'rejected' && runtimeResult.status === 'rejected') {
            throw traceResult.reason || runtimeResult.reason;
        }
        const traceData = traceResult.status === 'fulfilled' ? traceResult.value || {} : {};
        const runtimeData = runtimeResult.status === 'fulfilled' ? runtimeResult.value || {} : {};
        const traces = traceData?.traces && typeof traceData.traces === 'object' && !Array.isArray(traceData.traces)
            ? traceData.traces as Record<string, RuntimeTraceStep[]>
            : {};
        const flowCounts = emptyCounts();
        for (const steps of Object.values(traces)) {
            for (const step of Array.isArray(steps) ? steps : []) flowCounts[normalizeFlowType(step)] += 1;
        }
        const runtimeNodes = Array.isArray(runtimeData?.nodes) ? runtimeData.nodes.length : Number(runtimeData?.runtimeNodes || 0);
        const replaySafe = traceData?.replaySafe === true && traceData?.traceSemantics === 'exact-observed-span-instances';
        const observed = runtimeData?.observed === true || Object.keys(traces).length > 0 || runtimeNodes > 0;
        return {
            online: true,
            observed,
            replaySafe,
            traceSemantics: String(traceData?.traceSemantics || ''),
            runtimeNodes,
            traces,
            flowCounts,
            updatedAt: new Date().toISOString(),
            projectHint,
        };
    } catch (error: any) {
        return {
            online: false,
            observed: false,
            replaySafe: false,
            traceSemantics: '',
            runtimeNodes: 0,
            traces: {},
            flowCounts: emptyCounts(),
            updatedAt: new Date().toISOString(),
            projectHint,
            error: String(error?.message || error || 'Runtime feed unavailable'),
        };
    }
}
