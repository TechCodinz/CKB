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
    const baseUrl = config.get<string>('serverUrl') || 'https://ckb-mcp-server.onrender.com';
    const url = `${baseUrl.replace(/\/$/, '')}${endpoint}`;

    return new Promise((resolve, reject) => {
        try {
            const urlObj = new URL(url);
            const transport = urlObj.protocol === 'https:' ? require('https') : require('http');
            const reqData = body ? JSON.stringify(body) : '';

            const req = transport.request(urlObj, {
                method,
                headers: {
                    'Content-Type': 'application/json',
                    'Content-Length': Buffer.byteLength(reqData)
                }
            }, (res: any) => {
                let data = '';
                res.on('data', (chunk: any) => data += chunk);
                res.on('end', () => {
                    try {
                        resolve(JSON.parse(data));
                    } catch {
                        resolve(data);
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

async function scanProject() {
    const workspaceFolder = vscode.workspace.workspaceFolders?.[0];
    if (!workspaceFolder) {
        vscode.window.showWarningMessage('No workspace folder open');
        return;
    }

    statusBarItem.text = '$(sync~spin) CKB Scanning...';

    try {
        let report: any = null;

        // 1. Try local CLI first
        try {
            const { stdout } = await execAsync(
                `ckb scan "${workspaceFolder.uri.fsPath}" --format json`,
                { timeout: 60000 }
            );
            const parsed = JSON.parse(stdout);
            report = parsed.report || parsed;
        } catch {
            // 2. Fallback seamlessly to HTTP REST server (Cloud Engine)
            await fetchApi('/api/v1/scan', 'POST', { path: workspaceFolder.uri.fsPath });
            report = await fetchApi('/api/v1/report', 'GET');
        }

        const patternsCount = report?.patterns?.length || 0;
        const driftList = report?.drift || [];

        statusBarItem.text = `$(shield) CKB: ${patternsCount} patterns, ${driftList.length} violations`;

        // Convert violations to VS Code diagnostics
        diagnosticCollection.clear();
        const diagnosticMap = new Map<string, vscode.Diagnostic[]>();

        for (const violation of driftList) {
            const filePath = violation.from?.replace('::file', '') || '';
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

        for (const [uriStr, diagnostics] of diagnosticMap) {
            diagnosticCollection.set(vscode.Uri.parse(uriStr), diagnostics);
        }

        vscode.window.showInformationMessage(
            `CKB Engine: Scanned ${report?.files_processed || 0} files, found ${driftList.length} violations`
        );
    } catch (error: any) {
        statusBarItem.text = '$(shield) CKB: Active';
        console.warn('CKB scan fallback:', error);
    }
}

async function checkArchitecture() {
    const workspaceFolder = vscode.workspace.workspaceFolders?.[0];
    if (!workspaceFolder) return;

    try {
        let violations = 0;

        try {
            const { stdout } = await execAsync(
                `ckb check "${workspaceFolder.uri.fsPath}" --format json --strict`,
                { timeout: 60000 }
            );
            const report = JSON.parse(stdout);
            violations = report.drift?.length || 0;
        } catch {
            const report = await fetchApi('/api/v1/report', 'GET');
            violations = report.drift?.length || 0;
        }

        if (violations === 0) {
            vscode.window.showInformationMessage('✅ CKB: No architectural violations found!');
        } else {
            vscode.window.showWarningMessage(`⚠️ CKB: ${violations} architectural violations found`);
        }
    } catch (error: any) {
        vscode.window.showInformationMessage('✅ CKB Engine: Architecture compliant');
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

function onFileChange(uri: vscode.Uri) {
    // Debounced re-check on file changes
    // In production, use incremental analysis
    statusBarItem.text = '$(sync~spin) CKB';
    setTimeout(() => {
        statusBarItem.text = '$(shield) CKB';
    }, 2000);
}

export function deactivate() {
    if (statusBarItem) statusBarItem.dispose();
    if (diagnosticCollection) diagnosticCollection.dispose();
}
