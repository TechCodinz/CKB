import * as vscode from 'vscode';
import { execFile } from 'child_process';
import { promisify } from 'util';
import * as path from 'path';

const execFileAsync = promisify(execFile);
const CACHE_PREFIX = 'ckb.ide.intelligence.v1:';

export type IntelligenceSource = 'local-core' | 'reality-server' | 'scan-report';

export interface IntelligenceState {
    workspace: string;
    source: IntelligenceSource;
    updatedAt: string;
    bundle?: any;
    scan?: any;
    activity?: any;
    dna?: any;
    memory?: any;
    error?: string;
}

function workspaceKey(root: string) {
    return `${CACHE_PREFIX}${root.replace(/\\/g, '/').toLowerCase()}`;
}

function binaryName() {
    return vscode.workspace.getConfiguration('ckb').get<string>('intelligenceBinary', 'ckb-intelligence').trim() || 'ckb-intelligence';
}

function timeoutMs() {
    return Math.max(30_000, vscode.workspace.getConfiguration('ckb').get<number>('analysisTimeoutMs', 120_000));
}

async function runJson(args: string[]): Promise<any> {
    const executable = binaryName();
    try {
        const { stdout } = await execFileAsync(executable, args, {
            timeout: timeoutMs(),
            maxBuffer: 64 * 1024 * 1024,
            windowsHide: true,
        });
        const body = String(stdout || '').trim();
        if (!body) throw new Error(`${executable} returned no JSON output`);
        return JSON.parse(body);
    } catch (error: any) {
        const stdout = typeof error?.stdout === 'string' ? error.stdout.trim() : '';
        if (stdout) {
            try { return JSON.parse(stdout); } catch { /* preserve the real process error below */ }
        }
        throw error;
    }
}

export function intelligenceBinaryMissing(error: any) {
    return error?.code === 'ENOENT' || /not recognized|command not found|spawn .*enoent/i.test(String(error?.message || ''));
}

export async function buildWorkspaceBundle(root: string, query = 'architecture hotspots dependencies runtime change risk'): Promise<any> {
    return runJson(['bundle', root, '--query', query, '--depth', '3', '--limit', '36']);
}

export async function queryWorkspaceMemory(root: string, query: string, depth = 3, limit = 32): Promise<any> {
    return runJson(['memory', root, query, '--depth', String(depth), '--limit', String(limit)]);
}

export async function analyzeWorkspaceActivity(root: string): Promise<any> {
    return runJson(['activity', root]);
}

export async function analyzeWorkspaceDna(root: string): Promise<any> {
    return runJson(['dna', root]);
}

export function fallbackStateFromScan(root: string, scan: any, error?: string): IntelligenceState {
    return {
        workspace: root,
        source: 'scan-report',
        updatedAt: new Date().toISOString(),
        scan,
        error,
    };
}

export function stateFromBundle(root: string, bundle: any): IntelligenceState {
    return {
        workspace: root,
        source: 'local-core',
        updatedAt: new Date().toISOString(),
        bundle,
        scan: bundle?.scan,
        activity: bundle?.activity,
        dna: bundle?.dna,
        memory: bundle?.memory,
    };
}

export async function persistIntelligence(context: vscode.ExtensionContext, state: IntelligenceState) {
    await context.workspaceState.update(workspaceKey(state.workspace), state);
}

export function restoreIntelligence(context: vscode.ExtensionContext, root: string): IntelligenceState | undefined {
    return context.workspaceState.get<IntelligenceState>(workspaceKey(root));
}

export function compactMemoryDigest(state: IntelligenceState | undefined) {
    const activity = state?.activity || state?.bundle?.activity;
    const memory = state?.memory || state?.bundle?.memory;
    const dna = state?.dna || state?.bundle?.dna;
    const hotspots = Array.isArray(activity?.hotspots) ? activity.hotspots.slice(0, 12) : [];
    const priorityIds = Array.isArray(activity?.memoryPriorityIds) ? activity.memoryPriorityIds.slice(0, 32) : [];
    const roots = Array.isArray(memory?.rootIds) ? memory.rootIds.slice(0, 16) : [];
    return {
        version: 'ckb-ide-memory-digest-v1',
        workspace: state?.workspace,
        updatedAt: state?.updatedAt,
        source: state?.source,
        snapshotId: state?.scan?.snapshot_id || state?.bundle?.scan?.snapshot_id,
        nodes: activity?.nodesAnalyzed ?? state?.scan?.nodes ?? 0,
        edges: activity?.edgesAnalyzed ?? state?.scan?.edges ?? 0,
        runtimeCoveragePct: activity?.runtimeCoveragePct ?? 0,
        codeDnaHealth: dna?.overallHealth,
        hotspots: hotspots.map((node: any) => ({
            id: node.id,
            name: node.name,
            path: node.path,
            role: node.role,
            activityIndex: node.activityIndex,
            changeSensitivityIndex: node.changeSensitivityIndex,
            runtimeObserved: node.runtimeObserved,
        })),
        memoryPriorityIds: priorityIds,
        memoryRoots: roots,
        evidencePolicy: 'static-runtime-predicted-separated',
        synthetic: false,
    };
}

export async function openNodeInEditor(node: any) {
    const rawPath = String(node?.path || node?.id || '').split('::')[0];
    if (!rawPath) return;
    const folders = vscode.workspace.workspaceFolders || [];
    let uri: vscode.Uri | undefined;
    for (const folder of folders) {
        const candidate = path.isAbsolute(rawPath) ? rawPath : path.join(folder.uri.fsPath, rawPath);
        try {
            await vscode.workspace.fs.stat(vscode.Uri.file(candidate));
            uri = vscode.Uri.file(candidate);
            break;
        } catch { /* try another workspace root */ }
    }
    if (!uri && path.isAbsolute(rawPath)) uri = vscode.Uri.file(rawPath);
    if (!uri) {
        vscode.window.showWarningMessage(`CKB could not resolve source file: ${rawPath}`);
        return;
    }
    const document = await vscode.workspace.openTextDocument(uri);
    const editor = await vscode.window.showTextDocument(document, { preview: false });
    const line = Math.max(0, Number(node?.line || 1) - 1);
    const column = Math.max(0, Number(node?.column || 1) - 1);
    const position = new vscode.Position(Math.min(line, Math.max(0, document.lineCount - 1)), column);
    editor.selection = new vscode.Selection(position, position);
    editor.revealRange(new vscode.Range(position, position), vscode.TextEditorRevealType.InCenterIfOutsideViewport);
}
