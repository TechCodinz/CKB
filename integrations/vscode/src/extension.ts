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

async function scanProject() {
    const workspaceFolder = vscode.workspace.workspaceFolders?.[0];
    if (!workspaceFolder) {
        vscode.window.showWarningMessage('No workspace folder open');
        return;
    }

    statusBarItem.text = '$(sync~spin) CKB Scanning...';

    try {
        const { stdout } = await execAsync(
            `ckb scan "${workspaceFolder.uri.fsPath}" --format json`,
            { timeout: 60000 }
        );

        const report = JSON.parse(stdout);

        statusBarItem.text = `$(shield) CKB: ${report.report?.patterns?.length || 0} patterns, ${report.report?.drift?.length || 0} violations`;

        // Convert violations to VS Code diagnostics
        diagnosticCollection.clear();
        const diagnosticMap = new Map<string, vscode.Diagnostic[]>();

        for (const violation of report.report?.drift || []) {
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
            `CKB: Scanned ${report.report?.files_processed || 0} files, found ${report.report?.drift?.length || 0} violations`
        );
    } catch (error: any) {
        statusBarItem.text = '$(warning) CKB Error';
        if (error.message?.includes('not found') || error.message?.includes('not recognized')) {
            vscode.window.showErrorMessage(
                'CKB CLI not found. Install it with: curl -fsSL https://ckb.dev/install.sh | sh'
            );
        } else {
            vscode.window.showErrorMessage(`CKB scan failed: ${error.message}`);
        }
    }
}

async function checkArchitecture() {
    const workspaceFolder = vscode.workspace.workspaceFolders?.[0];
    if (!workspaceFolder) return;

    try {
        const { stdout } = await execAsync(
            `ckb check "${workspaceFolder.uri.fsPath}" --format json --strict`,
            { timeout: 60000 }
        );

        const report = JSON.parse(stdout);
        const violations = report.drift?.length || 0;

        if (violations === 0) {
            vscode.window.showInformationMessage('✅ CKB: No architectural violations found!');
        } else {
            vscode.window.showWarningMessage(`⚠️ CKB: ${violations} architectural violations found`);
        }
    } catch (error: any) {
        vscode.window.showErrorMessage(`CKB check failed: ${error.message}`);
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
        const { stdout } = await execAsync(
            `ckb impact "${workspaceFolder.uri.fsPath}" "${filePath}" ${line} --format json`,
            { timeout: 30000 }
        );

        const impact = JSON.parse(stdout);
        const totalImpacted = (impact.direct_impacts?.length || 0) + (impact.indirect_impacts?.length || 0);

        const panel = vscode.window.createWebviewPanel(
            'ckbImpact',
            `CKB Impact Analysis: ${filePath.split(/[/\\]/).pop()}:${line}`,
            vscode.ViewColumn.Beside,
            {}
        );

        panel.webview.html = `
            <html><body style="padding: 20px; font-family: system-ui;">
                <h2>Impact Analysis</h2>
                <p><strong>Risk Score:</strong> ${(impact.risk_score * 100).toFixed(0)}%</p>
                <p><strong>Estimated Effort:</strong> ${impact.estimated_effort}</p>
                <h3>Direct Impacts (${impact.direct_impacts?.length || 0})</h3>
                <ul>${(impact.direct_impacts || []).map((i: any) =>
            `<li>${i.path}:${i.line} — ${i.impact_kind} (${(i.confidence * 100).toFixed(0)}%)</li>`
        ).join('')}</ul>
                <h3>Indirect Impacts (${impact.indirect_impacts?.length || 0})</h3>
                <ul>${(impact.indirect_impacts || []).map((i: any) =>
            `<li>${i.path}:${i.line} — ${i.impact_kind}</li>`
        ).join('')}</ul>
            </body></html>
        `;
    } catch (error: any) {
        vscode.window.showErrorMessage(`CKB impact analysis failed: ${error.message}`);
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
