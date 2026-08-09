import { execFile } from 'child_process';
import { promisify } from 'util';
import * as fs from 'fs/promises';
import * as vscode from 'vscode';

const execFileAsync = promisify(execFile);

export interface AgentChangeRequest {
    instruction: string;
    projectId: string;
    target: { id?: string; name?: string; kind?: string; path?: string; line?: number; column?: number };
    patchFile: string;
    validationFile: string;
    stateFile: string;
    baseline?: string;
}

export interface ExactConfirmation {
    capsuleId: string;
    snapshotId: string;
    stagedTreeId: string;
    stateFile: string;
    message: string;
}

async function readState(stateFile: string): Promise<any> {
    return JSON.parse(await fs.readFile(stateFile, 'utf8'));
}

export class CkbTransactionAgent {
    constructor(
        private readonly context: vscode.ExtensionContext,
        private readonly workspaceRoot: () => string,
    ) {}

    private timeoutMs() {
        return Math.max(30_000, vscode.workspace.getConfiguration('ckb').get<number>('analysisTimeoutMs', 120_000));
    }

    private async runLocal(args: string[]): Promise<any> {
        const executable = vscode.workspace.getConfiguration('ckb').get<string>('intelligenceBinary', 'ckb-intelligence').trim() || 'ckb-intelligence';
        const { stdout } = await execFileAsync(executable, args, {
            timeout: this.timeoutMs(),
            maxBuffer: 64 * 1024 * 1024,
            windowsHide: true,
        });
        const body = String(stdout || '').trim();
        if (!body) throw new Error(`${executable} returned no transaction evidence`);
        return JSON.parse(body);
    }

    private async apiKey() {
        const secret = await this.context.secrets.get('ckb.cloudApiKey');
        return secret || vscode.workspace.getConfiguration('ckb').get<string>('apiKey', '').trim();
    }

    private async cloud(path: string, body: Record<string, unknown>): Promise<any> {
        const config = vscode.workspace.getConfiguration('ckb');
        const baseUrl = config.get<string>('cloudApiUrl', 'https://ckb-backend-api.onrender.com').replace(/\/$/, '');
        const apiKey = await this.apiKey();
        if (!apiKey || !apiKey.startsWith('ckb_live_')) {
            throw new Error('A ckb_live_ Cloud API key is required for architecture transactions');
        }
        const url = new URL(`${baseUrl}/api/v1/mcp${path}`);
        const payload = JSON.stringify(body);
        return new Promise((resolve, reject) => {
            const transport = url.protocol === 'https:' ? require('https') : require('http');
            const request = transport.request(url, {
                method: 'POST',
                headers: {
                    'Authorization': `Bearer ${apiKey}`,
                    'Content-Type': 'application/json',
                    'Content-Length': String(Buffer.byteLength(payload)),
                    'User-Agent': 'CKB-VSCode-Transaction-Agent/1.0',
                },
            }, (response: any) => {
                let raw = '';
                response.on('data', (chunk: any) => raw += chunk);
                response.on('end', () => {
                    let parsed: any = raw;
                    try { parsed = raw ? JSON.parse(raw) : {}; } catch { /* retain exact response */ }
                    if (response.statusCode >= 400) {
                        reject(new Error(parsed?.message || parsed || `CKB Cloud returned HTTP ${response.statusCode}`));
                    } else {
                        resolve(parsed);
                    }
                });
            });
            request.on('error', reject);
            request.setTimeout(this.timeoutMs(), () => request.destroy(new Error('CKB Cloud transaction request timed out')));
            request.write(payload);
            request.end();
        });
    }

    /** Prepare locally, validate in an isolated worktree, then persist that exact evidence in Cloud. */
    async prepare(request: AgentChangeRequest) {
        const root = this.workspaceRoot();
        if (!root) throw new Error('Open a workspace folder before preparing a transaction');
        const capsule = await this.cloud('/architecture/prepare-change', {
            instruction: request.instruction,
            project_id: request.projectId,
            context: { selectedNode: request.target },
        });
        const local = await this.runLocal([
            'prepare-patch', root, request.patchFile, request.validationFile, request.stateFile,
            '--baseline', request.baseline || 'HEAD',
        ]);
        const transaction = local.transaction;
        const recorded = await this.cloud(`/architecture/transactions/${encodeURIComponent(capsule.capsuleId)}/validation`, {
            snapshotId: capsule.snapshotId,
            baselineCommit: transaction.baseline_commit,
            patchObjectId: transaction.patch_object_id,
            stagedTreeId: transaction.staged_tree_id,
            branchName: transaction.branch_name,
            validationSucceeded: transaction.state === 'validated',
            validation: transaction.validations,
        });
        return { capsule, local, recorded, mutationApplied: false, activeCheckoutModified: false, synthetic: false };
    }

    /** Commit only after the caller supplies the exact Cloud snapshot and locally staged tree. */
    async confirmAndCommit(confirmation: ExactConfirmation) {
        let transaction = await readState(confirmation.stateFile);
        if (transaction.staged_tree_id !== confirmation.stagedTreeId) throw new Error('Confirmation does not match the local staged tree');
        await this.cloud(`/architecture/transactions/${encodeURIComponent(confirmation.capsuleId)}/confirm`, {
            snapshotId: confirmation.snapshotId,
            stagedTreeId: confirmation.stagedTreeId,
        });
        let local: any;
        if (transaction.state === 'committed') {
            local = { committedSha: transaction.committed_sha, resumed: true };
        } else {
            local = await this.runLocal([
                'commit-patch', confirmation.stateFile,
                '--confirm-staged-tree', confirmation.stagedTreeId,
                '--message', confirmation.message,
            ]);
            transaction = await readState(confirmation.stateFile);
        }
        const recorded = await this.cloud(`/architecture/transactions/${encodeURIComponent(confirmation.capsuleId)}/committed`, {
            stagedTreeId: confirmation.stagedTreeId,
            committedSha: transaction.committed_sha || local.committedSha,
        });
        return { local, recorded, merged: false, pushed: false, activeCheckoutModified: false, synthetic: false };
    }

    async rescan(capsuleId: string, stateFile: string) {
        const local = await this.runLocal(['rescan-patch', stateFile]);
        const observedCommitSha = local.rollbackCommittedSha || local.committedSha;
        const recorded = await this.cloud(`/architecture/transactions/${encodeURIComponent(capsuleId)}/rescan`, {
            observedCommitSha,
            snapshotId: local.scan?.snapshot_id,
            validations: local.validations,
            evidence: {
                scan: local.scan,
                activity: local.activity,
                dna: local.dna,
                memory: local.memory,
                evidencePolicy: local.evidencePolicy,
                activeCheckoutModified: false,
                synthetic: false,
            },
        });
        return { local, recorded, synthetic: false };
    }

    async rollback(capsuleId: string, stateFile: string, confirmCommittedSha: string) {
        let transaction = await readState(stateFile);
        let local: any;
        if (transaction.state === 'rolled-back') {
            local = {
                rollbackCommittedSha: transaction.rollback_committed_sha,
                rollbackStagedTreeId: transaction.rollback_staged_tree_id,
                rollbackValidations: transaction.rollback_validations,
                resumed: true,
            };
        } else {
            local = await this.runLocal([
                'rollback-patch', stateFile,
                '--confirm-committed-sha', confirmCommittedSha,
            ]);
            transaction = await readState(stateFile);
        }
        const recorded = await this.cloud(`/architecture/transactions/${encodeURIComponent(capsuleId)}/rollback`, {
            committedSha: confirmCommittedSha,
            rollbackStagedTreeId: transaction.rollback_staged_tree_id || local.rollbackStagedTreeId,
            rollbackCommitSha: transaction.rollback_committed_sha || local.rollbackCommittedSha,
            validations: transaction.rollback_validations || local.rollbackValidations,
        });
        return { local, recorded, merged: false, pushed: false, activeCheckoutModified: false, synthetic: false };
    }
}
