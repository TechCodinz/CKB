import * as vscode from 'vscode';
import { execFile } from 'child_process';
import { promisify } from 'util';
import * as path from 'path';
import {
    IntelligenceState,
    buildWorkspaceBundle,
    fallbackStateFromScan,
    intelligenceBinaryMissing,
    openNodeInEditor,
    persistIntelligence,
    queryWorkspaceMemory,
    restoreIntelligence,
    stateFromBundle,
} from './intelligence';
import { CkbRealityViewProvider } from './realityView';

const execFileAsync = promisify(execFile);

let statusBarItem: vscode.StatusBarItem;
let diagnosticCollection: vscode.DiagnosticCollection;
let realityProvider: CkbRealityViewProvider;
let extensionContext: vscode.ExtensionContext;
let activeState: IntelligenceState | undefined;
let debounceTimer: ReturnType<typeof setTimeout> | undefined;
let changedFiles = new Set<string>();
let cliAvailabilityWarned = false;

function workspaceFolder() {
    return vscode.workspace.workspaceFolders?.[0];
}

function workspaceRoot() {
    return workspaceFolder()?.uri.fsPath || '';
}

function ckbBinary() {
    return vscode.workspace.getConfiguration('ckb').get<string>('cliBinary', 'ckb').trim() || 'ckb';
}

function analysisTimeout() {
    return Math.max(30_000, vscode.workspace.getConfiguration('ckb').get<number>('analysisTimeoutMs', 120_000));
}

async function runCliJson(args: string[], timeout = analysisTimeout()): Promise<any> {
    try {
        const { stdout } = await execFileAsync(ckbBinary(), args, {
            timeout,
            maxBuffer: 64 * 1024 * 1024,
            windowsHide: true,
        });
        return JSON.parse(String(stdout || '').trim());
    } catch (error: any) {
        const stdout = typeof error?.stdout === 'string' ? error.stdout.trim() : '';
        if (stdout) {
            try { return JSON.parse(stdout); } catch { /* rethrow real error */ }
        }
        throw error;
    }
}

function isCliMissing(error: any) {
    return error?.code === 'ENOENT' || /not recognized|command not found|spawn .*enoent/i.test(String(error?.message || ''));
}

async function fetchApi(endpoint: string, method: string = 'GET', body?: any): Promise<any> {
    const config = vscode.workspace.getConfiguration('ckb');
    const baseUrl = config.get<string>('serverUrl', 'http://localhost:3000').replace(/\/$/, '');
    const apiKey = config.get<string>('apiKey', '').trim();
    const url = new URL(`${baseUrl}${endpoint}`);

    return new Promise((resolve, reject) => {
        const transport = url.protocol === 'https:' ? require('https') : require('http');
        const payload = body === undefined ? '' : JSON.stringify(body);
        const headers: Record<string, string> = { 'Content-Type': 'application/json' };
        if (payload) headers['Content-Length'] = String(Buffer.byteLength(payload));
        if (apiKey) headers['X-API-Key'] = apiKey;
        const request = transport.request(url, { method, headers }, (response: any) => {
            let raw = '';
            response.on('data', (chunk: any) => raw += chunk);
            response.on('end', () => {
                let parsed: any = raw;
                try { parsed = raw ? JSON.parse(raw) : {}; } catch { /* keep text */ }
                if (response.statusCode >= 400) {
                    reject(new Error(parsed?.message || parsed || `CKB server returned HTTP ${response.statusCode}`));
                    return;
                }
                resolve(parsed);
            });
        });
        request.on('error', reject);
        request.setTimeout(analysisTimeout(), () => request.destroy(new Error('CKB server request timed out')));
        if (payload) request.write(payload);
        request.end();
    });
}

function warnCliUnavailableOnce() {
    if (cliAvailabilityWarned) return;
    cliAvailabilityWarned = true;
    vscode.window.showWarningMessage(
        'CKB local intelligence binaries are not available. Install the current CKB CLI package for deep local activity + memory, or connect a local CKB server.',
        'Install Instructions',
    ).then(choice => {
        if (choice === 'Install Instructions') {
            vscode.env.openExternal(vscode.Uri.parse('https://github.com/TechCodinz/CKB/releases'));
        }
    });
}

function nodeIdValue(value: any) {
    if (typeof value === 'string') return value;
    if (value && typeof value === 'object') return String(value['0'] || value.id || value.path || '');
    return '';
}

function fileFromNodeId(value: any) {
    const id = nodeIdValue(value);
    const separator = id.indexOf('::');
    return separator >= 0 ? id.slice(0, separator) : id;
}

function resolveViolationUri(root: string, violation: any) {
    const file = fileFromNodeId(violation?.from);
    if (!file) return undefined;
    return vscode.Uri.file(path.isAbsolute(file) ? file : path.join(root, file));
}

function applyDiagnostics(root: string, report: any) {
    diagnosticCollection.clear();
    if (!vscode.workspace.getConfiguration('ckb').get<boolean>('showDiagnostics', true)) return;
    const map = new Map<string, vscode.Diagnostic[]>();
    for (const violation of report?.drift || []) {
        const uri = resolveViolationUri(root, violation);
        if (!uri) continue;
        const severityName = String(violation?.severity || '').toLowerCase();
        const severity = severityName === 'critical' || severityName === 'error'
            ? vscode.DiagnosticSeverity.Error
            : severityName === 'warning'
                ? vscode.DiagnosticSeverity.Warning
                : vscode.DiagnosticSeverity.Information;
        const line = Math.max(0, Number(violation?.line || 1) - 1);
        const diagnostic = new vscode.Diagnostic(
            new vscode.Range(line, 0, line, 200),
            String(violation?.message || 'Architecture finding'),
            severity,
        );
        diagnostic.source = 'CKB Reality';
        diagnostic.code = String(violation?.kind || violation?.rule || 'architecture');
        const key = uri.toString();
        const rows = map.get(key) || [];
        rows.push(diagnostic);
        map.set(key, rows);
    }
    for (const [uri, diagnostics] of map) diagnosticCollection.set(vscode.Uri.parse(uri), diagnostics);
}

function updateStatusFromState(state: IntelligenceState | undefined) {
    if (!state) {
        statusBarItem.text = '$(shield) CKB';
        statusBarItem.tooltip = 'CKB Living Architecture Reality';
        return;
    }
    const activity = state.activity || state.bundle?.activity;
    const scan = state.scan || state.bundle?.scan;
    const hotspots = activity?.hotspots?.length || 0;
    const runtimeCoverage = Number(activity?.runtimeCoveragePct || 0);
    if (activity) {
        statusBarItem.text = `$(pulse) CKB: ${activity.nodesAnalyzed || 0} symbols • ${hotspots} hotspots`;
        statusBarItem.tooltip = `CKB Deep Activity • ${runtimeCoverage.toFixed(1)}% of architecture symbols carry runtime observations • ${state.source}`;
    } else {
        statusBarItem.text = `$(shield) CKB: ${scan?.nodes || 0} nodes • ${(scan?.drift || []).length} findings`;
        statusBarItem.tooltip = 'Base architecture scan ready. Run CKB: Deep Activity Analysis for activity + memory intelligence.';
    }
}

async function commitState(state: IntelligenceState) {
    activeState = state;
    await persistIntelligence(extensionContext, state);
    realityProvider.setState(state);
    updateStatusFromState(state);
    applyDiagnostics(state.workspace, state.scan || state.bundle?.scan || {});
}

async function scanWorkspaceReport(root: string): Promise<any> {
    try {
        const result = await runCliJson(['scan', root, '--format', 'json']);
        return result?.report || result;
    } catch (cliError: any) {
        if (!isCliMissing(cliError)) console.warn('CKB CLI scan failed:', cliError);
        try {
            await fetchApi('/api/v1/scan', 'POST', { path: root });
            return await fetchApi('/api/v1/report');
        } catch (serverError: any) {
            if (isCliMissing(cliError)) warnCliUnavailableOnce();
            throw serverError;
        }
    }
}

async function scanProject(options: { quiet?: boolean } = {}) {
    const root = workspaceRoot();
    if (!root) {
        if (!options.quiet) vscode.window.showWarningMessage('CKB: Open a workspace folder first.');
        return;
    }
    statusBarItem.text = '$(sync~spin) CKB: scanning architecture…';
    try {
        const report = await scanWorkspaceReport(root);
        const state = fallbackStateFromScan(root, report);
        await commitState(state);
        if (!options.quiet) {
            vscode.window.showInformationMessage(`CKB: ${report?.files_processed || 0} files • ${report?.nodes || 0} nodes • ${(report?.drift || []).length} architecture findings`);
        }
    } catch (error: any) {
        statusBarItem.text = '$(error) CKB: unavailable';
        if (!options.quiet) vscode.window.showErrorMessage(`CKB scan failed: ${error?.message || error}`);
    }
}

async function deepAnalyze(options: { quiet?: boolean; reveal?: boolean } = {}) {
    const root = workspaceRoot();
    if (!root) return;
    statusBarItem.text = '$(sync~spin) CKB: mapping living architecture…';
    try {
        const bundle = await buildWorkspaceBundle(root);
        const state = stateFromBundle(root, bundle);
        await commitState(state);
        if (options.reveal !== false) realityProvider.reveal();
        if (!options.quiet) {
            const activity = bundle?.activity || {};
            vscode.window.showInformationMessage(`CKB Reality ready: ${activity.nodesAnalyzed || 0} symbols • ${activity.boundaryCount || 0} boundaries • ${Number(activity.runtimeCoveragePct || 0).toFixed(1)}% runtime coverage`);
        }
        return;
    } catch (error: any) {
        if (!intelligenceBinaryMissing(error)) console.warn('CKB intelligence facade failed:', error);
        if (intelligenceBinaryMissing(error)) warnCliUnavailableOnce();
        // Preserve useful base architecture evidence rather than substituting
        // made-up activity metrics when the deeper local facade is missing.
        try {
            const report = await scanWorkspaceReport(root);
            const fallback = fallbackStateFromScan(
                root,
                report,
                `Deep activity/memory facade unavailable: ${error?.message || error}. Base scan remains authoritative; no activity metrics were fabricated.`,
            );
            await commitState(fallback);
            if (options.reveal !== false) realityProvider.reveal();
        } catch (scanError: any) {
            statusBarItem.text = '$(error) CKB: analysis unavailable';
            if (!options.quiet) vscode.window.showErrorMessage(`CKB deep analysis failed: ${scanError?.message || scanError}`);
        }
    }
}

async function queryMemory(initialQuery?: string) {
    const root = workspaceRoot();
    if (!root) return;
    const query = initialQuery?.trim() || await vscode.window.showInputBox({
        title: 'CKB Architecture Memory',
        prompt: 'Ask about a symbol, service, flow, responsibility, risk, or dependency path.',
        placeHolder: 'e.g. how does authentication reach the database?',
        ignoreFocusOut: true,
    });
    if (!query?.trim()) return;
    statusBarItem.text = '$(database) CKB: retrieving architecture memory…';
    try {
        const result = await queryWorkspaceMemory(root, query.trim(), 3, 36);
        const memory = result?.memory || result;
        const next: IntelligenceState = {
            ...(activeState || { workspace: root, source: 'local-core' as const, updatedAt: new Date().toISOString() }),
            workspace: root,
            source: 'local-core',
            updatedAt: new Date().toISOString(),
            memory,
        };
        await commitState(next);
        realityProvider.reveal();
        return;
    } catch (localError: any) {
        try {
            const result = await fetchApi('/api/v1/intelligence/memory/query', 'POST', {
                query: query.trim(), depth: 3, limit: 36,
            });
            const next: IntelligenceState = {
                ...(activeState || { workspace: root, source: 'reality-server' as const, updatedAt: new Date().toISOString() }),
                workspace: root,
                source: 'reality-server',
                updatedAt: new Date().toISOString(),
                memory: result?.memory || result,
            };
            await commitState(next);
            realityProvider.reveal();
        } catch (serverError: any) {
            vscode.window.showErrorMessage(`CKB memory query unavailable: ${serverError?.message || localError?.message || serverError}`);
            updateStatusFromState(activeState);
        }
    }
}

async function checkArchitecture() {
    const root = workspaceRoot();
    if (!root) return;
    try {
        const report = await runCliJson(['check', root, '--format', 'json', '--strict']);
        const drift = report?.drift || [];
        if (drift.length === 0) vscode.window.showInformationMessage('CKB: No architecture findings at the configured guardrail threshold.');
        else vscode.window.showWarningMessage(`CKB: ${drift.length} architecture findings. Open Living Reality for context.`);
        applyDiagnostics(root, report);
    } catch (error: any) {
        const stdout = typeof error?.stdout === 'string' ? error.stdout.trim() : '';
        if (stdout) {
            try {
                const report = JSON.parse(stdout);
                applyDiagnostics(root, report);
                vscode.window.showWarningMessage(`CKB: ${(report?.drift || []).length} architecture findings.`);
                return;
            } catch { /* continue */ }
        }
        vscode.window.showErrorMessage(`CKB architecture check failed: ${error?.message || error}`);
    }
}

function impactItems(rows: any[]) {
    return rows.map(item => {
        const pathValue = item?.path || item?.node?.['0'] || item?.node || item;
        const confidence = Number(item?.confidence);
        return `<div class="impact"><code>${escapeHtml(String(pathValue || 'unknown'))}</code>${Number.isFinite(confidence) ? `<span>${(confidence * 100).toFixed(0)}% graph confidence</span>` : ''}</div>`;
    }).join('');
}

function escapeHtml(value: string) {
    return value.replace(/[&<>"']/g, char => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[char] || char));
}

function showImpactReality(impact: any, file: string, line: number) {
    const direct = impact?.direct_impacts || impact?.directly_affected || [];
    const indirect = impact?.indirect_impacts || impact?.transitively_affected || [];
    const risk = Number(impact?.risk_score ?? 0);
    const panel = vscode.window.createWebviewPanel('ckbImpactReality', `CKB Ripple • ${path.basename(file)}:${line}`, vscode.ViewColumn.Beside, {});
    panel.webview.html = `<!doctype html><html><head><meta name="viewport" content="width=device-width,initial-scale=1"><style>
body{background:#05070d;color:#eaf4ff;font:13px/1.5 system-ui;padding:18px;margin:0}.hero{border:1px solid rgba(67,233,255,.25);background:radial-gradient(circle at 0 0,rgba(67,233,255,.11),transparent 40%),#0b101d;border-radius:16px;padding:20px}.tag{display:inline-block;border:1px solid rgba(194,140,255,.5);color:#c99cff;border-radius:999px;padding:3px 8px;font-size:10px;font-weight:800;letter-spacing:1px}h1{font-size:23px;margin:9px 0 2px}.file{color:#7689a6;font-family:monospace;word-break:break-all}.metrics{display:grid;grid-template-columns:repeat(3,1fr);gap:8px;margin:15px 0}.metric{padding:12px;border-radius:10px;background:rgba(255,255,255,.035);border:1px solid rgba(255,255,255,.08)}.metric strong{font-size:20px;display:block;color:#43e9ff}.metric span{color:#8798b1;font-size:10px}.grid{display:grid;grid-template-columns:1fr 1fr;gap:10px;margin-top:10px}.box{background:#0a0f1b;border:1px solid rgba(255,255,255,.08);border-radius:12px;padding:13px}.box h2{font-size:12px;text-transform:uppercase;letter-spacing:1px;color:#ffbd66;margin:0 0 8px}.box:nth-child(2) h2{color:#43e9ff}.impact{padding:7px 0;border-bottom:1px solid rgba(255,255,255,.05)}.impact code{display:block;word-break:break-all;color:#dbe8f5}.impact span{font-size:10px;color:#788aa5}.truth{color:#9caec5;font-size:11px;margin-top:13px}.truth b{color:#c99cff}@media(max-width:650px){.grid{grid-template-columns:1fr}.metrics{grid-template-columns:1fr 1fr}}
</style></head><body><div class="hero"><span class="tag">PREDICTED • GRAPH IMPACT</span><h1>Change Ripple Reality</h1><div class="file">${escapeHtml(file)}:${line}</div><div class="metrics"><div class="metric"><strong>${Number.isFinite(risk) ? `${(risk * 100).toFixed(0)}%` : '—'}</strong><span>graph risk index</span></div><div class="metric"><strong>${direct.length}</strong><span>direct impacts</span></div><div class="metric"><strong>${indirect.length}</strong><span>transitive impacts</span></div></div><div class="grid"><section class="box"><h2>Direct exposure</h2>${impactItems(direct) || '<div class="file">No direct impacts returned.</div>'}</section><section class="box"><h2>Transitive ripple</h2>${impactItems(indirect) || '<div class="file">No transitive impacts returned.</div>'}</section></div><div class="truth"><b>PREDICTED</b> means graph simulation. It is not proof that production will fail. Runtime evidence remains a separate evidence class.</div></div></body></html>`;
}

async function analyzeImpact() {
    const editor = vscode.window.activeTextEditor;
    const root = workspaceRoot();
    if (!editor || !root) {
        vscode.window.showWarningMessage('CKB: Open a source file first.');
        return;
    }
    const file = editor.document.uri.fsPath;
    const line = editor.selection.active.line + 1;
    try {
        let impact: any;
        try {
            impact = await runCliJson(['impact', root, file, String(line), '--format', 'json'], 60_000);
        } catch {
            impact = await fetchApi('/api/v1/impact', 'POST', { path: root, file, line, change_type: 'modify' });
        }
        showImpactReality(impact, file, line);
    } catch (error: any) {
        vscode.window.showErrorMessage(`CKB impact analysis failed: ${error?.message || error}`);
    }
}

async function showStatus() {
    const items = [
        { label: '$(rocket) Open Living Architecture Reality', description: 'Persistent architecture activity + memory cockpit', command: 'ckb.openReality' },
        { label: '$(pulse) Deep Activity Analysis', description: 'Map hotspots, boundaries, runtime coverage and memory priorities', command: 'ckb.deepActivity' },
        { label: '$(database) Query Architecture Memory', description: 'Retrieve bounded model context from the real graph', command: 'ckb.queryMemory' },
        { label: '$(search) Base Scan', description: 'Refresh architecture findings and diagnostics', command: 'ckb.scan' },
        { label: '$(git-compare) Cursor Change Ripple', description: 'Predict direct/transitive impact before editing', command: 'ckb.impact' },
        { label: '$(server) Start MCP Server', description: 'Expose CKB locally to AI/MCP clients', command: 'ckb.startServer' },
    ];
    const selected = await vscode.window.showQuickPick(items, { title: 'CKB Architecture Intelligence', placeHolder: 'Choose an architecture operation' });
    if (selected) await vscode.commands.executeCommand(selected.command);
}

function startMcpServer() {
    const terminal = vscode.window.createTerminal('CKB Reality Server');
    terminal.show();
    terminal.sendText(`${ckbBinary()} serve --cors`);
}

async function setApiKey() {
    const config = vscode.workspace.getConfiguration('ckb');
    const current = config.get<string>('apiKey', '');
    const key = await vscode.window.showInputBox({
        title: 'CKB Server API Key',
        prompt: 'Stored in VS Code settings and sent only to the configured ckb.serverUrl.',
        value: current,
        password: true,
        ignoreFocusOut: true,
    });
    if (key !== undefined) {
        await config.update('apiKey', key.trim(), vscode.ConfigurationTarget.Global);
        vscode.window.showInformationMessage('CKB API key updated.');
    }
}

function onFileChanged(uri: vscode.Uri) {
    if (!vscode.workspace.getConfiguration('ckb').get<boolean>('rescanOnSave', true)) return;
    changedFiles.add(uri.fsPath);
    statusBarItem.text = `$(history) CKB: ${changedFiles.size} changed • memory stale`;
    if (debounceTimer) clearTimeout(debounceTimer);
    const delay = Math.max(350, vscode.workspace.getConfiguration('ckb').get<number>('rescanDebounceMs', 1400));
    debounceTimer = setTimeout(async () => {
        const count = changedFiles.size;
        changedFiles.clear();
        const deepOnSave = vscode.workspace.getConfiguration('ckb').get<boolean>('deepAnalysisOnSave', true);
        if (deepOnSave) await deepAnalyze({ quiet: true, reveal: false });
        else await scanProject({ quiet: true });
        if (count > 1) console.log(`CKB coalesced ${count} file events into one analysis pass.`);
    }, delay);
}

async function openCloudExplorer() {
    const configured = vscode.workspace.getConfiguration('ckb').get<string>('cloudExplorerUrl', 'https://ckb-nu.vercel.app/project/current');
    await vscode.env.openExternal(vscode.Uri.parse(configured));
}

export async function activate(context: vscode.ExtensionContext) {
    extensionContext = context;
    diagnosticCollection = vscode.languages.createDiagnosticCollection('ckb');
    context.subscriptions.push(diagnosticCollection);

    statusBarItem = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 100);
    statusBarItem.text = '$(shield) CKB';
    statusBarItem.tooltip = 'CKB Living Architecture Reality';
    statusBarItem.command = 'ckb.openReality';
    statusBarItem.show();
    context.subscriptions.push(statusBarItem);

    realityProvider = new CkbRealityViewProvider(context);
    context.subscriptions.push(vscode.window.registerWebviewViewProvider(CkbRealityViewProvider.viewType, realityProvider, {
        webviewOptions: { retainContextWhenHidden: true },
    }));

    context.subscriptions.push(
        vscode.commands.registerCommand('ckb.scan', () => scanProject()),
        vscode.commands.registerCommand('ckb.check', checkArchitecture),
        vscode.commands.registerCommand('ckb.impact', analyzeImpact),
        vscode.commands.registerCommand('ckb.deepActivity', () => deepAnalyze()),
        vscode.commands.registerCommand('ckb.queryMemory', (query?: string) => queryMemory(query)),
        vscode.commands.registerCommand('ckb.openReality', () => realityProvider.reveal()),
        vscode.commands.registerCommand('ckb.openArchitectureNode', (node: any) => openNodeInEditor(node)),
        vscode.commands.registerCommand('ckb.openExplorer', openCloudExplorer),
        vscode.commands.registerCommand('ckb.showStatus', showStatus),
        vscode.commands.registerCommand('ckb.startServer', startMcpServer),
        vscode.commands.registerCommand('ckb.setApiKey', setApiKey),
    );

    const watcher = vscode.workspace.createFileSystemWatcher('**/*.{ts,tsx,js,jsx,mjs,py,go,rs,java}');
    watcher.onDidChange(onFileChanged);
    watcher.onDidCreate(onFileChanged);
    watcher.onDidDelete(onFileChanged);
    context.subscriptions.push(watcher);

    const root = workspaceRoot();
    if (root) {
        activeState = restoreIntelligence(context, root);
        realityProvider.setState(activeState);
        updateStatusFromState(activeState);
        if (vscode.workspace.getConfiguration('ckb').get<boolean>('autoScanOnOpen', true)) {
            // Delay until VS Code finishes restoring editor state. The scan runs
            // once, in the background, and file events are debounced separately.
            setTimeout(() => deepAnalyze({ quiet: true, reveal: false }), 700);
        }
    }
}

export function deactivate() {
    if (debounceTimer) clearTimeout(debounceTimer);
    statusBarItem?.dispose();
    diagnosticCollection?.dispose();
}
