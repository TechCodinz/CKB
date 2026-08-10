import * as vscode from 'vscode';
import * as path from 'path';

const TASKS = ['understand', 'explain', 'change', 'debug', 'review', 'migrate', 'optimize', 'security'] as const;
type FabricTask = typeof TASKS[number];

function slash(value: string) {
    return value.replace(/\\/g, '/').replace(/^\.\//, '');
}

export class CkbModelIntelligenceV13 implements vscode.Disposable {
    private readonly disposables: vscode.Disposable[] = [];
    private readonly output = vscode.window.createOutputChannel('CKB Architecture Intelligence V13');

    constructor(private readonly context: vscode.ExtensionContext) {
        this.disposables.push(
            this.output,
            vscode.commands.registerCommand('ckb.compileArchitectureContextAtCursor', () => this.compileAtCursor()),
            vscode.commands.registerCommand('ckb.showObservedModelRegistry', () => this.showObservedRegistry()),
            vscode.commands.registerCommand('ckb.showArchitectureConstitution', () => this.showConstitution()),
        );
    }

    private config() { return vscode.workspace.getConfiguration('ckb'); }

    private projectId() {
        return this.config().get<string>('cloudProjectId', 'current').trim() || 'current';
    }

    private timeoutMs() {
        return Math.max(30_000, this.config().get<number>('analysisTimeoutMs', 120_000));
    }

    private async apiKey() {
        return (await this.context.secrets.get('ckb.cloudApiKey')) || this.config().get<string>('apiKey', '').trim();
    }

    private async ensureKey() {
        const existing = await this.apiKey();
        if (existing?.startsWith('ckb_live_')) return existing;
        const entered = await vscode.window.showInputBox({
            title: 'CKB Cloud API Key',
            prompt: 'Enter a ckb_live_ key. It is stored in VS Code SecretStorage.',
            placeHolder: 'ckb_live_…',
            password: true,
            ignoreFocusOut: true,
        });
        if (!entered) return undefined;
        const key = entered.trim();
        if (!key.startsWith('ckb_live_')) throw new Error('CKB Cloud API keys must begin with ckb_live_');
        await this.context.secrets.store('ckb.cloudApiKey', key);
        return key;
    }

    private async request(method: 'GET' | 'POST', route: string, body?: Record<string, unknown>) {
        const key = await this.ensureKey();
        if (!key) throw new Error('A CKB Cloud API key is required');
        const base = this.config().get<string>('cloudApiUrl', 'https://ckb-backend-api.onrender.com').replace(/\/$/, '');
        const url = new URL(`${base}/api/v1/mcp${route}`);
        const payload = body ? JSON.stringify(body) : '';
        return new Promise<any>((resolve, reject) => {
            const transport = url.protocol === 'https:' ? require('https') : require('http');
            const req = transport.request(url, {
                method,
                headers: {
                    Authorization: `Bearer ${key}`,
                    'Content-Type': 'application/json',
                    ...(payload ? { 'Content-Length': String(Buffer.byteLength(payload)) } : {}),
                    'User-Agent': 'CKB-VSCode-Intelligence-Fabric/13',
                },
            }, (response: any) => {
                let raw = '';
                response.on('data', (chunk: any) => raw += chunk);
                response.on('end', () => {
                    let parsed: any = raw;
                    try { parsed = raw ? JSON.parse(raw) : {}; } catch { /* retain raw */ }
                    if (response.statusCode >= 400) reject(new Error(parsed?.message || parsed || `HTTP ${response.statusCode}`));
                    else resolve(parsed);
                });
            });
            req.on('error', reject);
            req.setTimeout(this.timeoutMs(), () => req.destroy(new Error('CKB V13 request timed out')));
            if (payload) req.write(payload);
            req.end();
        });
    }

    private cursorIdentity() {
        const editor = vscode.window.activeTextEditor;
        const root = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
        if (!editor || !root || editor.document.uri.scheme !== 'file') throw new Error('Open a source file inside a workspace first');
        const relative = path.relative(root, editor.document.uri.fsPath);
        if (!relative || relative.startsWith('..') || path.isAbsolute(relative)) throw new Error('The active source file must be inside the open workspace');
        const position = editor.selection.active;
        const range = editor.document.getWordRangeAtPosition(position);
        const symbol = range ? editor.document.getText(range).trim() : '';
        return {
            path: slash(relative),
            symbol: symbol || undefined,
            line: position.line + 1,
            column: position.character + 1,
        };
    }

    private async task(): Promise<FabricTask | undefined> {
        const picked = await vscode.window.showQuickPick(TASKS.map(task => ({ label: task.toUpperCase(), task })), {
            title: 'CKB V13 • Architecture Task',
            placeHolder: 'Choose how CKB should compile architecture memory for the model/agent',
        });
        return picked?.task;
    }

    private async compileAtCursor() {
        try {
            const identity = this.cursorIdentity();
            const task = await this.task();
            if (!task) return;
            const query = await vscode.window.showInputBox({
                title: `CKB V13 • Compile ${task.toUpperCase()} Context`,
                prompt: 'Describe the task. CKB will retrieve a bounded evidence package, not dump the repository.',
                value: `${task} ${identity.symbol || path.basename(identity.path)} at ${identity.path}:${identity.line}`,
                ignoreFocusOut: true,
            });
            if (!query?.trim()) return;
            const result = await vscode.window.withProgress({
                location: vscode.ProgressLocation.Notification,
                title: 'CKB: compiling model-neutral architecture context…',
            }, () => this.request('POST', '/architecture/context/compile', {
                project_id: this.projectId(),
                query: query.trim(),
                task,
                depth: task === 'change' || task === 'debug' || task === 'security' ? 3 : 2,
                limit: 120,
                budget: { maxChars: 48_000, maxNodes: 80, maxEdges: 160 },
            }));
            const context = result?.context || {};
            this.output.clear();
            this.output.appendLine('CKB ARCHITECTURE INTELLIGENCE FABRIC V13');
            this.output.appendLine(`Project: ${this.projectId()}`);
            this.output.appendLine(`Task: ${context.task || task}`);
            this.output.appendLine(`Memory version: ${context.sourceMemoryVersion || 'unknown'}`);
            this.output.appendLine(`Roots: ${(context.sourceRootIds || []).join(', ') || 'none resolved'}`);
            this.output.appendLine(`Evidence: ${context.evidenceLedger?.length || 0} provenance records`);
            this.output.appendLine(`Runtime evidence: ${context.runtimeEvidenceRecords || 0}`);
            this.output.appendLine(`Predicted evidence: ${context.predictedEvidenceRecords || 0}`);
            this.output.appendLine(`Nodes/edges: ${context.includedNodes || 0}/${context.includedEdges || 0}`);
            this.output.appendLine(`Truncated: ${context.truncated === true ? 'YES — request more deliberately' : 'NO'}`);
            this.output.appendLine('');
            for (const section of Array.isArray(context.sections) ? context.sections : []) {
                this.output.appendLine(`── ${section.id} [${section.evidenceClass || 'contract'}] ──`);
                this.output.appendLine(String(section.content || ''));
                this.output.appendLine('');
            }
            this.output.show(true);
            vscode.window.showInformationMessage(`CKB V13 compiled ${context.includedNodes || 0} symbols / ${context.includedEdges || 0} relationships with provenance.`);
        } catch (error: any) {
            vscode.window.showErrorMessage(`CKB V13 context compilation failed: ${error?.message || error}`);
        }
    }

    private async showObservedRegistry() {
        try {
            const task = await this.task();
            if (!task) return;
            const encodedProject = encodeURIComponent(this.projectId());
            const result = await this.request('GET', `/architecture/models/observed-registry?project_id=${encodedProject}&task=${encodeURIComponent(task)}`);
            const registry = Array.isArray(result?.registry) ? result.registry : [];
            if (!registry.length) {
                vscode.window.showInformationMessage('CKB has no active model capability profiles yet. Models remain unranked until profiles and observed validations exist.');
                return;
            }
            const pick = await vscode.window.showQuickPick(registry.map((item: any) => ({
                label: `${item.provider}/${item.model}`,
                description: item.observedScore == null ? 'unranked — no observed validation evidence' : `${(item.observedScore * 100).toFixed(1)}% observed score • ${item.observations} checks`,
                detail: `rollback ${item.rollbackRate == null ? 'unobserved' : `${(item.rollbackRate * 100).toFixed(1)}%`} • task ${task}`,
                item,
            })), {
                title: `CKB V13 • Observed Model Registry • ${task.toUpperCase()}`,
                placeHolder: 'Scores summarize this project’s recorded validation outcomes only',
            });
            if (pick) {
                const doc = await vscode.workspace.openTextDocument({ language: 'json', content: JSON.stringify(pick.item, null, 2) });
                await vscode.window.showTextDocument(doc, { preview: true, viewColumn: vscode.ViewColumn.Beside });
            }
        } catch (error: any) {
            vscode.window.showErrorMessage(`CKB V13 model registry unavailable: ${error?.message || error}`);
        }
    }

    private async showConstitution() {
        try {
            const result = await this.request('GET', '/architecture/constitution');
            const doc = await vscode.workspace.openTextDocument({ language: 'json', content: JSON.stringify(result, null, 2) });
            await vscode.window.showTextDocument(doc, { preview: true, viewColumn: vscode.ViewColumn.Beside });
        } catch (error: any) {
            vscode.window.showErrorMessage(`CKB Architecture Constitution unavailable: ${error?.message || error}`);
        }
    }

    dispose() {
        for (const disposable of this.disposables) disposable.dispose();
    }
}

export function activateModelIntelligenceV13(context: vscode.ExtensionContext) {
    return new CkbModelIntelligenceV13(context);
}
