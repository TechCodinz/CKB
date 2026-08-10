import { execFile } from 'child_process';
import { promisify } from 'util';
import * as fs from 'fs/promises';
import * as path from 'path';
import * as vscode from 'vscode';
import { AgentTarget, CkbTransactionAgent } from './transactions';

const execFileAsync = promisify(execFile);
const MAX_PATCH_BYTES = 4 * 1024 * 1024;

type CursorTarget = AgentTarget & {
    id: string;
    name: string;
    kind: string;
    path: string;
    line: number;
    column: number;
    depth: 'line' | 'symbol' | 'file';
};

type GuardedSession = {
    projectId: string;
    instruction: string;
    target: CursorTarget;
    baseline: string;
    capsule: any;
    conversation?: any;
    validation?: { response: any; patchFile: string; validationFile: string; stateFile: string; mode: string };
    commit?: any;
    actual?: any;
    rollback?: any;
    actualError?: string;
};

function slash(value: string) {
    return value.replace(/\\/g, '/').replace(/^\.\//, '').replace(/^\/+/, '');
}

function escapeHtml(value: unknown) {
    return String(value ?? '').replace(/[&<>"']/g, char => ({
        '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;',
    }[char] || char));
}

function compact(value: unknown, size = 12) {
    const text = String(value || '');
    return text ? text.slice(0, size) : '—';
}

export class CkbCursorGuardedReality implements vscode.Disposable {
    private readonly disposables: vscode.Disposable[] = [];
    private panel?: vscode.WebviewPanel;
    private session?: GuardedSession;

    constructor(
        private readonly context: vscode.ExtensionContext,
        private readonly transactions: CkbTransactionAgent,
    ) {
        this.disposables.push(
            vscode.commands.registerCommand('ckb.askRaiziomAtCursor', () => this.ask()),
            vscode.commands.registerCommand('ckb.prepareGuardedChangeAtCursor', () => this.prepare()),
            vscode.commands.registerCommand('ckb.validateGuardedWorkspaceChange', () => this.validate()),
            vscode.commands.registerCommand('ckb.commitGuardedChange', () => this.commit()),
            vscode.commands.registerCommand('ckb.rollbackGuardedChange', () => this.rollback()),
            vscode.commands.registerCommand('ckb.openGuardedChangeReality', () => this.render()),
        );
    }

    private root() {
        return vscode.workspace.workspaceFolders?.[0]?.uri.fsPath || '';
    }

    private projectId() {
        return vscode.workspace.getConfiguration('ckb').get<string>('cloudProjectId', 'current').trim() || 'current';
    }

    private timeoutMs() {
        return Math.max(30_000, vscode.workspace.getConfiguration('ckb').get<number>('analysisTimeoutMs', 120_000));
    }

    private async git(args: string[]) {
        const root = this.root();
        if (!root) throw new Error('Open a Git workspace first');
        const { stdout } = await execFileAsync('git', args, {
            cwd: root,
            timeout: this.timeoutMs(),
            maxBuffer: 32 * 1024 * 1024,
            windowsHide: true,
        });
        return String(stdout || '').trimEnd();
    }

    private async gitRaw(args: string[]) {
        const root = this.root();
        if (!root) throw new Error('Open a Git workspace first');
        const { stdout } = await execFileAsync('git', args, {
            cwd: root,
            timeout: this.timeoutMs(),
            maxBuffer: 32 * 1024 * 1024,
            windowsHide: true,
        });
        return String(stdout || '');
    }

    private async target(): Promise<CursorTarget> {
        const editor = vscode.window.activeTextEditor;
        const root = this.root();
        if (!editor || !root || editor.document.uri.scheme !== 'file') throw new Error('Open a source file inside the workspace first');
        const absolute = editor.document.uri.fsPath;
        const relative = path.relative(root, absolute);
        if (relative.startsWith('..') || path.isAbsolute(relative)) throw new Error('The active source file must be inside the open workspace');
        const file = slash(relative);
        const position = editor.selection.active;
        const wordRange = editor.document.getWordRangeAtPosition(position);
        const word = wordRange ? editor.document.getText(wordRange).trim() : '';
        const name = word || path.basename(file);
        const depth: CursorTarget['depth'] = !editor.selection.isEmpty ? 'line' : word ? 'symbol' : 'file';
        return {
            id: word ? `${file}::${word}` : file,
            name,
            kind: word ? 'symbol' : 'file',
            path: file,
            line: position.line + 1,
            column: position.character + 1,
            depth,
        };
    }

    private async ensureCloudKey() {
        if (await this.transactions.hasCloudApiKey()) return true;
        const key = await vscode.window.showInputBox({
            title: 'CKB Cloud API Key',
            prompt: 'Enter a ckb_live_ key. Guarded Change stores it in VS Code SecretStorage.',
            placeHolder: 'ckb_live_…',
            password: true,
            ignoreFocusOut: true,
        });
        if (!key) return false;
        await this.transactions.setCloudApiKey(key);
        return true;
    }

    private async ask() {
        try {
            const target = await this.target();
            if (!await this.ensureCloudKey()) return;
            const question = await vscode.window.showInputBox({
                title: `Ask Raiziom • ${target.name}`,
                prompt: 'Ask about CURRENT architecture, observed runtime, predicted impact, or a safe change strategy.',
                value: `Explain the CURRENT architecture/runtime context around ${target.name} and the safest change surface.`,
                ignoreFocusOut: true,
            });
            if (!question?.trim()) return;
            const response = await vscode.window.withProgress({
                location: vscode.ProgressLocation.Notification,
                title: 'CKB: grounding Raiziom in architecture evidence…',
            }, () => this.transactions.converse(question.trim(), this.projectId(), target, {
                semanticDepth: target.depth,
                surface: 'vscode-guarded-change-v1',
            }));
            this.session = this.session || {
                projectId: this.projectId(), instruction: '', target, baseline: '', capsule: null,
            };
            this.session.target = target;
            this.session.conversation = { question: question.trim(), response };
            this.render();
        } catch (error: any) {
            const choice = await vscode.window.showErrorMessage(`CKB Raiziom unavailable: ${error?.message || error}`, 'Continue in Cloud');
            if (choice === 'Continue in Cloud') await vscode.commands.executeCommand('ckb.askRaiziomAboutCursor');
        }
    }

    private async prepare() {
        try {
            const target = await this.target();
            if (!await this.ensureCloudKey()) return;
            const instruction = await vscode.window.showInputBox({
                title: `Prepare Guarded Change • ${target.name}`,
                prompt: 'Describe the intended outcome. CKB prepares evidence and gates; it does not silently edit source.',
                ignoreFocusOut: true,
            });
            if (!instruction?.trim()) return;
            const baseline = (await this.git(['rev-parse', '--verify', 'HEAD'])).trim();
            const capsule = await vscode.window.withProgress({
                location: vscode.ProgressLocation.Notification,
                title: 'CKB: preparing CURRENT → PROPOSED architecture capsule…',
            }, () => this.transactions.prepareCapsule(instruction.trim(), this.projectId(), target));
            this.session = {
                projectId: this.projectId(),
                instruction: instruction.trim(),
                target,
                baseline,
                capsule,
                conversation: this.session?.conversation,
            };
            this.render();
            vscode.window.showInformationMessage('CKB state is PROPOSED. Make the local implementation, then run Validate Guarded Workspace Change.');
        } catch (error: any) {
            vscode.window.showErrorMessage(`CKB could not prepare guarded change: ${error?.message || error}`);
        }
    }

    private async storage(capsuleId: string) {
        const folder = vscode.Uri.joinPath(this.context.globalStorageUri, 'guarded-change', capsuleId);
        await vscode.workspace.fs.createDirectory(folder);
        return {
            patchFile: vscode.Uri.joinPath(folder, 'workspace.patch').fsPath,
            validationFile: vscode.Uri.joinPath(folder, 'validation.json').fsPath,
            stateFile: vscode.Uri.joinPath(folder, 'transaction.json').fsPath,
        };
    }

    private async validationPlan(generatedPath: string) {
        const projectPlan = path.join(this.root(), '.ckb', 'validation.json');
        try {
            await fs.access(projectPlan);
            return { file: projectPlan, mode: '.ckb/validation.json' };
        } catch { /* optional */ }
        const choice = await vscode.window.showWarningMessage(
            'No .ckb/validation.json exists. Continue with structural Git validation only? This does NOT prove compilation or tests.',
            { modal: true },
            'Structural Validation Only',
        );
        if (choice !== 'Structural Validation Only') return undefined;
        await fs.writeFile(generatedPath, JSON.stringify([{
            label: 'Git staged diff check (structural only)',
            program: 'git',
            args: ['diff', '--cached', '--check'],
        }], null, 2));
        return { file: generatedPath, mode: 'structural-git-only' };
    }

    private async capturePatch(session: GuardedSession, patchFile: string) {
        await vscode.workspace.saveAll(false);
        const currentHead = (await this.git(['rev-parse', '--verify', 'HEAD'])).trim();
        if (currentHead !== session.baseline) throw new Error('HEAD changed after PROPOSED evidence was prepared; prepare a new guarded change');
        const untracked = (await this.git(['ls-files', '--others', '--exclude-standard'])).split(/\r?\n/).filter(Boolean);
        if (untracked.length) {
            const choice = await vscode.window.showWarningMessage(
                `CKB found ${untracked.length} untracked file(s). Exact tracked-diff validation cannot include them.`,
                { modal: true, detail: untracked.slice(0, 20).join('\n') },
                'Continue Tracked Only',
            );
            if (choice !== 'Continue Tracked Only') return undefined;
        }
        const patchText = await this.gitRaw(['diff', '--binary', '--no-ext-diff', session.baseline, '--']);
        if (!patchText.trim()) {
            vscode.window.showInformationMessage('CKB found no tracked workspace change against the PROPOSED baseline.');
            return undefined;
        }
        if (Buffer.byteLength(patchText) > MAX_PATCH_BYTES) throw new Error('Tracked patch exceeds the 4 MiB guarded transaction limit');
        const changed = (await this.git(['diff', '--name-only', session.baseline, '--'])).split(/\r?\n/).filter(Boolean);
        const choice = await vscode.window.showWarningMessage(
            `Validate this exact ${changed.length}-file diff in an isolated CKB worktree?`,
            { modal: true, detail: changed.slice(0, 40).join('\n') },
            'Validate Exact Diff',
        );
        if (choice !== 'Validate Exact Diff') return undefined;
        await fs.writeFile(patchFile, patchText, 'utf8');
        return changed;
    }

    private async validate() {
        const session = this.session;
        if (!session?.capsule?.capsuleId) {
            vscode.window.showWarningMessage('Prepare a guarded change first.');
            return;
        }
        const existing = session.validation?.response?.local?.transaction;
        if (existing?.state === 'validated' || session.commit) {
            const action = await vscode.window.showInformationMessage('This capsule is already locked to an exact validated tree.', 'Prepare New Change');
            if (action === 'Prepare New Change') await this.prepare();
            return;
        }
        try {
            if (session.validation?.stateFile) {
                try { await this.transactions.cleanup(session.validation.stateFile, true); } catch { /* best effort */ }
                session.validation = undefined;
            }
            const files = await this.storage(session.capsule.capsuleId);
            if (!await this.capturePatch(session, files.patchFile)) return;
            const plan = await this.validationPlan(files.validationFile);
            if (!plan) return;
            const response = await vscode.window.withProgress({
                location: vscode.ProgressLocation.Notification,
                title: 'CKB: validating exact diff in isolated worktree…',
            }, () => this.transactions.validatePreparedCapsule(session.capsule, {
                instruction: session.instruction,
                projectId: session.projectId,
                target: session.target,
                patchFile: files.patchFile,
                validationFile: plan.file,
                stateFile: files.stateFile,
                baseline: session.baseline,
            }));
            session.validation = { response, ...files, validationFile: plan.file, mode: plan.mode };
            this.render();
            const transaction = response?.local?.transaction;
            if (transaction?.state === 'validated') {
                vscode.window.showInformationMessage(`CKB VALIDATED staged tree ${compact(transaction.staged_tree_id)}. Nothing was merged or pushed.`);
            } else {
                vscode.window.showWarningMessage('CKB validation did not pass. Nothing was committed.');
            }
        } catch (error: any) {
            vscode.window.showErrorMessage(`CKB guarded validation failed: ${error?.message || error}`);
            this.render();
        }
    }

    private async commit() {
        const session = this.session;
        const transaction = session?.validation?.response?.local?.transaction;
        if (!session?.validation || transaction?.state !== 'validated') {
            vscode.window.showWarningMessage('CKB requires an exact VALIDATED staged tree before commit.');
            return;
        }
        const confirm = await vscode.window.showWarningMessage(
            `Commit exact staged tree ${compact(transaction.staged_tree_id)} on isolated branch ${transaction.branch_name}?`,
            { modal: true, detail: `Snapshot: ${session.capsule.snapshotId}\nBaseline: ${transaction.baseline_commit}\n\nNo merge. No push. Active checkout remains unchanged.` },
            'Confirm Exact Isolated Commit',
        );
        if (confirm !== 'Confirm Exact Isolated Commit') return;
        const message = await vscode.window.showInputBox({
            title: 'CKB Guarded Change • Isolated Commit Message',
            value: `ckb: ${session.instruction}`.slice(0, 180),
            ignoreFocusOut: true,
        });
        if (!message?.trim()) return;
        try {
            session.commit = await this.transactions.confirmAndCommit({
                capsuleId: session.capsule.capsuleId,
                snapshotId: session.capsule.snapshotId,
                stagedTreeId: transaction.staged_tree_id,
                stateFile: session.validation.stateFile,
                message: message.trim(),
            });
            try {
                session.actual = await this.transactions.rescan(session.capsule.capsuleId, session.validation.stateFile);
                session.actualError = undefined;
                vscode.window.showInformationMessage('CKB ACTUAL evidence is ready: isolated commit + post-change rescan. Merge/push remain explicit separate actions.');
            } catch (error: any) {
                session.actualError = String(error?.message || error);
                vscode.window.showWarningMessage(`Isolated commit exists, but ACTUAL post-change rescan is pending: ${session.actualError}`);
            }
            this.render();
        } catch (error: any) {
            vscode.window.showErrorMessage(`CKB exact commit was not created: ${error?.message || error}`);
        }
    }

    private async rollback() {
        const session = this.session;
        if (!session?.validation?.stateFile || !session.commit) {
            vscode.window.showWarningMessage('CKB has no committed guarded transaction to roll back.');
            return;
        }
        const state = await this.transactions.transactionState(session.validation.stateFile);
        const committedSha = state?.committed_sha || session.commit?.local?.committedSha;
        if (!committedSha) return;
        const confirm = await vscode.window.showWarningMessage(
            `Create a validated rollback commit for ${compact(committedSha)}?`,
            { modal: true, detail: 'Rollback remains isolated; the active checkout is not modified.' },
            'Confirm Exact Rollback',
        );
        if (confirm !== 'Confirm Exact Rollback') return;
        try {
            session.rollback = await this.transactions.rollback(session.capsule.capsuleId, session.validation.stateFile, committedSha);
            session.actual = await this.transactions.rescan(session.capsule.capsuleId, session.validation.stateFile);
            session.actualError = undefined;
            this.render();
        } catch (error: any) {
            vscode.window.showErrorMessage(`CKB rollback failed: ${error?.message || error}`);
        }
    }

    private ensurePanel() {
        if (this.panel) {
            this.panel.reveal(vscode.ViewColumn.Beside, true);
            return this.panel;
        }
        this.panel = vscode.window.createWebviewPanel('ckbGuardedChangeReality', 'CKB Guarded Change Reality', vscode.ViewColumn.Beside, {
            enableScripts: true,
            retainContextWhenHidden: true,
        });
        this.panel.onDidDispose(() => { this.panel = undefined; });
        this.panel.webview.onDidReceiveMessage(async message => {
            if (message?.action === 'ask') await this.ask();
            if (message?.action === 'prepare') await this.prepare();
            if (message?.action === 'validate') await this.validate();
            if (message?.action === 'commit') await this.commit();
            if (message?.action === 'rollback') await this.rollback();
            if (message?.action === 'cloud') await vscode.commands.executeCommand('ckb.continueSemanticRealityInCloud');
        });
        return this.panel;
    }

    private render() {
        const panel = this.ensurePanel();
        const session = this.session;
        const transaction = session?.validation?.response?.local?.transaction;
        const answer = session?.conversation?.response?.answer;
        const risk = session?.capsule?.summary?.predictedRiskScore;
        const runtimeObserved = session?.capsule?.summary?.runtimeObserved === true;
        const validations = Array.isArray(transaction?.validations) ? transaction.validations : [];
        const commitSha = session?.commit?.local?.committedSha || transaction?.committed_sha;
        const actualReady = Boolean(commitSha && session?.actual);
        const nonce = `${Date.now()}${Math.random().toString(36).slice(2)}`;
        panel.webview.html = `<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; script-src 'nonce-${nonce}'"><style>
body{background:#05070d;color:#e7edf7;font:13px/1.5 system-ui;margin:0;padding:18px}.hero,.card{background:#0a0f1b;border:1px solid #263043;border-radius:14px;padding:15px;margin-bottom:10px}.hero{background:radial-gradient(circle at 0 0,#132b3a,#0a0f1b 45%)}h1{margin:0 0 4px;font-size:22px}.muted{color:#8da0ba}.grid{display:grid;grid-template-columns:repeat(4,1fr);gap:8px}.stage{border:1px solid #273247;border-radius:12px;padding:12px;background:#090e18}.stage b{display:block;color:#67e8f9;font-size:11px;letter-spacing:1px}.ok{color:#86efac}.warn{color:#fbbf24}.bad{color:#fca5a5}code{word-break:break-all;color:#cbd5e1}.actions{display:flex;gap:7px;flex-wrap:wrap;margin-top:12px}button{background:#101827;color:#e7edf7;border:1px solid #33415b;border-radius:9px;padding:8px 10px;cursor:pointer}.truth{font-size:11px;color:#9aa9bd}.truth strong{color:#d8b4fe}@media(max-width:800px){.grid{grid-template-columns:1fr 1fr}}@media(max-width:480px){.grid{grid-template-columns:1fr}}
</style></head><body><section class="hero"><h1>CKB Guarded Change Reality</h1><div class="muted">Cursor → evidence → Raiziom → PROPOSED → exact diff → VALIDATED → explicit isolated commit → ACTUAL rescan.</div><div class="actions"><button data-a="ask">Ask Raiziom</button><button data-a="prepare">Prepare Change</button><button data-a="validate">Validate Workspace Diff</button><button data-a="commit">Commit Validated Tree</button><button data-a="rollback">Rollback</button><button data-a="cloud">Cloud Reality</button></div></section>
<section class="grid"><div class="stage"><b>CURRENT</b><div>${session?.target ? `${escapeHtml(session.target.path)}:${session.target.line}<br><code>${escapeHtml(session.target.name)}</code>` : 'Select source and ask/prepare.'}</div></div><div class="stage"><b>PROPOSED</b><div>${session?.capsule ? `Capsule <code>${escapeHtml(compact(session.capsule.capsuleId))}</code><br>${typeof risk === 'number' ? `Predicted risk ${(risk * 100).toFixed(0)}%` : 'Predicted impact prepared'}<br>${runtimeObserved ? '<span class="ok">Runtime evidence attached</span>' : '<span class="muted">No runtime evidence claimed</span>'}` : 'No proposal yet.'}</div></div><div class="stage"><b>VALIDATED</b><div>${transaction ? `State <span class="${transaction.state === 'validated' ? 'ok' : 'warn'}">${escapeHtml(transaction.state)}</span><br>Tree <code>${escapeHtml(compact(transaction.staged_tree_id))}</code><br>${escapeHtml(session?.validation?.mode || '')}` : 'No exact isolated validation yet.'}</div></div><div class="stage"><b>ACTUAL</b><div>${actualReady ? `<span class="ok">Isolated commit + rescan verified</span><br><code>${escapeHtml(compact(commitSha))}</code><br>MERGED: NO • PUSHED: NO` : commitSha ? `<span class="warn">Commit exists; rescan pending</span><br>${escapeHtml(session?.actualError || '')}` : 'Not actual until explicit commit + post-change rescan.'}</div></div></section>
${answer ? `<section class="card"><b>RAIZIOM • EVIDENCE-GROUNDED</b><p>${escapeHtml(answer)}</p></section>` : ''}
${validations.length ? `<section class="card"><b>VALIDATION EVIDENCE</b>${validations.map((row: any) => `<p><span class="${row.success ? 'ok' : 'bad'}">${row.success ? 'PASS' : 'FAIL'}</span> ${escapeHtml(row.label)}<br><code>${escapeHtml(row.program)} ${escapeHtml((row.args || []).join(' '))}</code></p>`).join('')}</section>` : ''}
<section class="card truth"><strong>TRUTH CONTRACT</strong><br>STATIC = source/AST/architecture evidence. RUNTIME = exact observed telemetry only. PREDICTED = simulated/proposed impact. Structural Git validation is not compiler/test verification. A proposed change is never labeled ACTUAL before the exact isolated commit exists and CKB completes its post-change rescan. Runtime retrace is only shown when telemetry actually observes it.</section>
<script nonce="${nonce}">const vscode=acquireVsCodeApi();document.querySelectorAll('button[data-a]').forEach(b=>b.addEventListener('click',()=>vscode.postMessage({action:b.dataset.a})));</script></body></html>`;
    }

    dispose() {
        this.panel?.dispose();
        for (const disposable of this.disposables) disposable.dispose();
    }
}

export function activateCursorGuardedChangeReality(context: vscode.ExtensionContext, transactions: CkbTransactionAgent) {
    return new CkbCursorGuardedReality(context, transactions);
}
