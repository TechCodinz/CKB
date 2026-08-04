import * as vscode from 'vscode';
import { exec } from 'child_process';
import { promisify } from 'util';

const execAsync = promisify(exec);

let statusBarItem: vscode.StatusBarItem;
let diagnosticCollection: vscode.DiagnosticCollection;

export function activate(context: vscode.ExtensionContext) {
    console.log('CKB extension activated');

    // Status bar
    statusBarItem = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 100);
    statusBarItem.text = '$(shield) CKB';
    statusBarItem.tooltip = 'CKB - Architectural Intelligence';
    statusBarItem.command = 'ckb.showStatus';
    statusBarItem.show();
    context.subscriptions.push(statusBarItem);

    // Diagnostics
    diagnosticCollection = vscode.languages.createDiagnosticCollection('ckb');
    context.subscriptions.push(diagnosticCollection);

    // Commands
    context.subscriptions.push(
        vscode.commands.registerCommand('ckb.scan', scanProject),
        vscode.commands.registerCommand('ckb.check', checkArchitecture),
        vscode.commands.registerCommand('ckb.impact', analyzeImpact),
        vscode.commands.registerCommand('ckb.showStatus', showStatus),
        vscode.commands.registerCommand('ckb.startServer', startMcpServer),
    );

    // File watcher for real-time analysis
    const watcher = vscode.workspace.createFileSystemWatcher('**/*.{ts,js,py,go,rs,java}');
    watcher.onDidChange(uri => onFileChange(uri));
    watcher.onDidCreate(uri => onFileChange(uri));
    context.subscriptions.push(watcher);

    // Run initial scan
    const workspaceFolder = vscode.workspace.workspaceFolders?.[0];
    if (workspaceFolder) {
        scanProject();
    }
}

async function fetchApi(endpoint: string, method: string = 'GET', body?: any): Promise<any> {
    const config = vscode.workspace.getConfiguration('ckb');
    // Previously defaulted to a hardcoded external domain
    // (https://ckb-mcp-server.onrender.com) that a remote scan request could
    // never actually work against anyway — that server can't read a path on
    // the user's local machine. Defaulting to localhost means this fallback
    // only ever "succeeds" when the user has deliberately started their own
    // `ckb serve` locally (see startMcpServer()), which is the only
    // configuration where sending a local filesystem path over HTTP
    // actually makes sense.
    const baseUrl = config.get<string>('serverUrl') || 'http://localhost:3000';
    const apiKey = config.get<string>('apiKey');
    const url = `${baseUrl.replace(/\/$/, '')}${endpoint}`;

    return new Promise((resolve, reject) => {
        try {
            const urlObj = new URL(url);
            const transport = urlObj.protocol === 'https:' ? require('https') : require('http');
            const reqData = body ? JSON.stringify(body) : '';

            const headers: Record<string, string> = {
                'Content-Type': 'application/json',
                'Content-Length': String(Buffer.byteLength(reqData)),
            };
            // Previously never sent — every request to a server that
            // actually enforces CKB_API_KEY (or per-user backend auth) would
            // get a silent 401 with no explanation to the user.
            if (apiKey) {
                headers['X-API-Key'] = apiKey;
            }

            const req = transport.request(urlObj, { method, headers }, (res: any) => {
                let data = '';
                res.on('data', (chunk: any) => data += chunk);
                res.on('end', () => {
                    let parsed: any;
                    try {
                        parsed = JSON.parse(data);
                    } catch {
                        parsed = data;
                    }
                    if (res.statusCode && res.statusCode >= 400) {
                        const message = (parsed && parsed.message) || `Server returned HTTP ${res.statusCode}`;
                        reject(new Error(message));
                    } else {
                        resolve(parsed);
                    }
                });
            });

            req.on('error', (err: any) => reject(err));
            if (reqData) req.write(reqData);
            req.end();
        } catch (e) {
            reject(e);
        }
    });
}

/**
 * Runs a `ckb` CLI command and returns its parsed JSON stdout.
 *
 * Node's `child_process.exec` treats ANY non-zero exit code as an error —
 * including `ckb check --strict`, which deliberately exits 1 when it finds
 * violations (that's the whole point of `--strict`, added so CI can gate on
 * it). That means the naive "try CLI, catch error, fall back to remote"
 * pattern this file used before would treat "the CLI worked and found real
 * violations" identically to "the CLI isn't installed" — always taking the
 * (broken) remote-fallback path whenever there was anything to actually
 * report. This distinguishes the two: a non-zero exit with valid JSON on
 * stdout is a successful run: use it. A missing binary (ENOENT) or
 * unparseable output is a real failure: only then does the caller fall back.
 */
async function runCliJson(command: string, timeoutMs: number): Promise<any> {
    try {
        const { stdout } = await execAsync(command, { timeout: timeoutMs });
        return JSON.parse(stdout);
    } catch (error: any) {
        if (typeof error.stdout === 'string' && error.stdout.trim().length > 0) {
            try {
                return JSON.parse(error.stdout);
            } catch {
                // stdout wasn't valid JSON either — fall through to rethrow.
            }
        }
        throw error;
    }
}

function isCliMissing(error: any): boolean {
    return error?.code === 'ENOENT' || /command not found|is not recognized/i.test(String(error?.message || ''));
}

let cliAvailabilityWarned = false;

/** Shown once per session, not on every failed scan, to avoid notification spam. */
function warnCliUnavailableOnce() {
    if (cliAvailabilityWarned) return;
    cliAvailabilityWarned = true;
    vscode.window.showWarningMessage(
        'CKB: the `ckb` CLI was not found, and no local CKB server is configured (or reachable) at ckb.serverUrl. ' +
        'Install the CLI, or start one with "CKB: Start MCP Server" and set ckb.serverUrl.',
        'Install Instructions'
    ).then(choice => {
        if (choice === 'Install Instructions') {
            vscode.env.openExternal(vscode.Uri.parse('https://github.com/TechCodinz/CKB/releases'));
        }
    });
}

/**
 * Recovers a real filesystem path from a violation's `from`/`to` node ID.
 * CKB's internal NodeId format is always `"{path}::{suffix}"` — for a
 * file-level node the suffix is literally "file", but for a function/class/
 * method-level violation it's the symbol name instead. The previous version
 * only stripped a literal `"::file"` suffix, so it correctly recovered the
 * path for file-level violations but left function/class-level violations
 * as the whole unmodified "path::functionName" string — not a valid file
 * path, so `vscode.Uri.file(...)` on it would point at a
 * nonexistent/mangled location and silently fail to show a diagnostic.
 */
function extractFilePath(nodeId: string | undefined): string {
    if (!nodeId) return '';
    const idx = nodeId.indexOf('::');
    return idx === -1 ? nodeId : nodeId.slice(0, idx);
}

async function scanProject() {
    const workspaceFolder = vscode.workspace.workspaceFolders?.[0];
    if (!workspaceFolder) {
        vscode.window.showWarningMessage('No workspace folder open');
        return;
    }

    statusBarItem.text = '$(sync~spin) CKB Scanning...';

    try {
        let report: any = null;

        // 1. Try local CLI first — this is the only path that can reliably
        //    read the user's actual workspace, since a remote server has no
        //    access to their local filesystem.
        try {
            const parsed = await runCliJson(
                `ckb scan "${workspaceFolder.uri.fsPath}" --format json`,
                60000
            );
            report = parsed.report || parsed;
        } catch (cliError: any) {
            if (!isCliMissing(cliError)) {
                // The CLI exists but genuinely failed (bad path, parse error,
                // etc.) — that's worth knowing about, not silently masking.
                console.warn('CKB CLI scan failed:', cliError);
            }
            // 2. Fall back to a configured CKB server. Only useful if the
            //    user has actually started one (locally, or a real reachable
            //    deployment) — see the note on ckb.serverUrl's default.
            try {
                await fetchApi('/api/v1/scan', 'POST', { path: workspaceFolder.uri.fsPath });
                report = await fetchApi('/api/v1/report', 'GET');
            } catch (remoteError: any) {
                statusBarItem.text = '$(shield) CKB: Unavailable';
                warnCliUnavailableOnce();
                return;
            }
        }

        const patternsCount = report?.patterns?.length || 0;
        const driftList = report?.drift || [];

        statusBarItem.text = `$(shield) CKB: ${patternsCount} patterns, ${driftList.length} violations`;

        // Convert violations to VS Code diagnostics
        diagnosticCollection.clear();
        const diagnosticMap = new Map<string, vscode.Diagnostic[]>();

        for (const violation of driftList) {
            const filePath = extractFilePath(violation.from);
            if (!filePath) continue;
            const uri = vscode.Uri.file(filePath);
            const range = new vscode.Range(0, 0, 0, 100);

            const severity = violation.severity === 'Critical' || violation.severity === 'Error'
                ? vscode.DiagnosticSeverity.Error
                : violation.severity === 'Warning'
                    ? vscode.DiagnosticSeverity.Warning
                    : vscode.DiagnosticSeverity.Information;

            const diagnostic = new vscode.Diagnostic(range, violation.message, severity);
            diagnostic.source = 'CKB';
            diagnostic.code = violation.kind;

            const existing = diagnosticMap.get(uri.toString()) || [];
            existing.push(diagnostic);
            diagnosticMap.set(uri.toString(), existing);
        }

        const showDiagnostics = vscode.workspace.getConfiguration('ckb').get<boolean>('showDiagnostics', true);
        if (showDiagnostics) {
            for (const [uriStr, diagnostics] of diagnosticMap) {
                diagnosticCollection.set(vscode.Uri.parse(uriStr), diagnostics);
            }
        }

        vscode.window.showInformationMessage(
            `CKB: Scanned ${report?.files_processed || 0} files, found ${driftList.length} violations`
        );
    } catch (error: any) {
        statusBarItem.text = '$(shield) CKB: Error';
        vscode.window.showErrorMessage(`CKB scan failed: ${error?.message || error}`);
    }
}

async function checkArchitecture() {
    const workspaceFolder = vscode.workspace.workspaceFolders?.[0];
    if (!workspaceFolder) return;

    try {
        let violations = 0;
        let ran = false;

        try {
            // --strict makes this deliberately exit non-zero when violations
            // are found — that's a successful run with real output, not a
            // failure, so runCliJson() (not the raw execAsync/catch pattern)
            // handles it correctly.
            const report = await runCliJson(
                `ckb check "${workspaceFolder.uri.fsPath}" --format json --strict`,
                60000
            );
            violations = report.drift?.length || 0;
            ran = true;
        } catch (cliError: any) {
            if (!isCliMissing(cliError)) {
                console.warn('CKB CLI check failed:', cliError);
            }
            try {
                const report = await fetchApi('/api/v1/report', 'GET');
                violations = report.drift?.length || 0;
                ran = true;
            } catch {
                warnCliUnavailableOnce();
            }
        }

        if (!ran) return;

        if (violations === 0) {
            vscode.window.showInformationMessage('✅ CKB: No architectural violations found!');
        } else {
            vscode.window.showWarningMessage(`⚠️ CKB: ${violations} architectural violations found`);
        }
    } catch (error: any) {
        vscode.window.showErrorMessage(`CKB check failed: ${error?.message || error}`);
    }
}

async function analyzeImpact() {
    const editor = vscode.window.activeTextEditor;
    if (!editor) {
        vscode.window.showWarningMessage('No active editor');
        return;
    }

    const workspaceFolder = vscode.workspace.workspaceFolders?.[0];
    if (!workspaceFolder) return;

    const filePath = editor.document.uri.fsPath;
    const line = editor.selection.active.line + 1;

    try {
        let impact: any = null;

        try {
            const { stdout } = await execAsync(
                `ckb impact "${workspaceFolder.uri.fsPath}" "${filePath}" ${line} --format json`,
                { timeout: 30000 }
            );
            impact = JSON.parse(stdout);
        } catch {
            impact = await fetchApi('/api/v1/impact', 'POST', {
                path: workspaceFolder.uri.fsPath,
                file: filePath,
                line,
                change_type: 'modify'
            });
        }

        const direct = impact.directly_affected || impact.direct_impacts || [];
        const indirect = impact.transitively_affected || impact.indirect_impacts || [];
        const risk = impact.risk_score !== undefined ? impact.risk_score : 0;

        const panel = vscode.window.createWebviewPanel(
            'ckbImpact',
            `CKB Impact Analysis: ${filePath.split(/[/\\]/).pop()}:${line}`,
            vscode.ViewColumn.Beside,
            {}
        );

        panel.webview.html = `
            <html><body style="padding: 20px; font-family: system-ui; background: #0d1117; color: #c9d1d9;">
                <h2 style="color: #58a6ff;">CKB Impact Analysis</h2>
                <p><strong>Risk Score:</strong> ${(risk * 100).toFixed(0)}%</p>
                <p><strong>Estimated Effort:</strong> ${impact.estimated_effort || (risk > 0.5 ? 'High' : 'Low')}</p>
                <h3 style="color: #d29922;">Direct Impacts (${direct.length})</h3>
                <ul>${direct.map((i: any) =>
            `<li>${typeof i === 'string' ? i : i.path || JSON.stringify(i)}</li>`
        ).join('')}</ul>
                <h3 style="color: #388bfd;">Transitive Impacts (${indirect.length})</h3>
                <ul>${indirect.map((i: any) =>
            `<li>${typeof i === 'string' ? i : i.path || JSON.stringify(i)}</li>`
        ).join('')}</ul>
            </body></html>
        `;
    } catch (error: any) {
        vscode.window.showErrorMessage(`CKB impact analysis failed: ${error.message || error}`);
    }
}

async function showStatus() {
    const workspaceFolder = vscode.workspace.workspaceFolders?.[0];
    if (!workspaceFolder) {
        vscode.window.showInformationMessage('CKB: No workspace open');
        return;
    }

    const items = [
        { label: '$(search) Scan Project', description: 'Full codebase scan', command: 'ckb.scan' },
        { label: '$(checklist) Check Architecture', description: 'Check for violations', command: 'ckb.check' },
        { label: '$(pulse) Analyze Impact', description: 'Analyze change impact at cursor', command: 'ckb.impact' },
        { label: '$(server) Start MCP Server', description: 'Start for AI integration', command: 'ckb.startServer' },
    ];

    const selected = await vscode.window.showQuickPick(items, { placeHolder: 'CKB Actions' });
    if (selected) {
        vscode.commands.executeCommand(selected.command);
    }
}

async function startMcpServer() {
    const terminal = vscode.window.createTerminal('CKB MCP Server');
    terminal.show();
    terminal.sendText('ckb serve --cors');
    vscode.window.showInformationMessage('CKB MCP Server starting on port 3000');
}

let debounceTimer: ReturnType<typeof setTimeout> | undefined;

function onFileChange(uri: vscode.Uri) {
    // Previously this only spun the status bar icon for 2 seconds and did
    // nothing else — no rescan ever actually happened, despite the README
    // documenting "Re-checks architecture when source files change
    // (debounced)" as a real feature. This is a full rescan (not true
    // incremental analysis — the engine has an incremental scan path, but
    // it isn't wired up to the CLI yet, see FEATURES_ADDED.md), debounced so
    // rapid saves/edits don't trigger a scan storm.
    const liveRescan = vscode.workspace.getConfiguration('ckb').get<boolean>('rescanOnSave', true);
    if (!liveRescan) return;

    statusBarItem.text = '$(sync~spin) CKB';
    if (debounceTimer) clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => {
        scanProject();
    }, 2000);
}

export function deactivate() {
    if (statusBarItem) statusBarItem.dispose();
    if (diagnosticCollection) diagnosticCollection.dispose();
}
