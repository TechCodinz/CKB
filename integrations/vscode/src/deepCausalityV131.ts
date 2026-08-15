import * as vscode from 'vscode';
import { execFile } from 'child_process';
import { promisify } from 'util';
import * as path from 'path';

const execFileAsync = promisify(execFile);

type Operation = {
  id: string;
  label: string;
  detail: string;
  args: () => Promise<string[] | undefined>;
};

function root(): string {
  return vscode.workspace.workspaceFolders?.[0]?.uri.fsPath || '';
}

function binary(): string {
  return vscode.workspace.getConfiguration('ckb').get<string>('causalityBinary', 'ckb-causality').trim() || 'ckb-causality';
}

function timeout(): number {
  return Math.max(30_000, vscode.workspace.getConfiguration('ckb').get<number>('analysisTimeoutMs', 120_000));
}

function bundlePath(workspace: string): string {
  return path.join(workspace, '.ckb', 'deep-causality.json');
}

async function run(args: string[], cwd: string): Promise<any> {
  const { stdout } = await execFileAsync(binary(), args, {
    cwd,
    timeout: timeout(),
    maxBuffer: 64 * 1024 * 1024,
    windowsHide: true,
  });
  const text = String(stdout || '').trim();
  return text ? JSON.parse(text) : {};
}

async function input(title: string, prompt: string, placeHolder?: string): Promise<string | undefined> {
  return vscode.window.showInputBox({ title, prompt, placeHolder, ignoreFocusOut: true });
}

async function twoEntities(title: string): Promise<string[] | undefined> {
  const source = await input(title, 'Source causal entity id');
  if (!source?.trim()) return;
  const sink = await input(title, 'Target/sink causal entity id');
  if (!sink?.trim()) return;
  return [source.trim(), sink.trim()];
}

async function oneEntity(title: string, label = 'Causal entity id'): Promise<string[] | undefined> {
  const value = await input(title, label);
  return value?.trim() ? [value.trim()] : undefined;
}

async function filePathArg(title: string, prompt: string): Promise<string[] | undefined> {
  const value = await input(title, prompt, 'Absolute path or workspace-relative JSON file');
  if (!value?.trim()) return;
  const workspace = root();
  const resolved = path.isAbsolute(value.trim()) ? value.trim() : path.join(workspace, value.trim());
  return [resolved];
}

function operations(): Operation[] {
  return [
    { id: 'data-flow', label: '$(git-compare) Data Flow', detail: 'Interprocedural value/data path', args: async () => twoEntities('CKB Data Flow') },
    { id: 'taint', label: '$(shield) Taint + Trust Boundaries', detail: 'Unsanitized source → sink paths', args: async () => {
      const source = await input('CKB Taint', 'Comma-separated source entity ids'); if (!source?.trim()) return;
      const sink = await input('CKB Taint', 'Comma-separated sink entity ids'); if (!sink?.trim()) return;
      return [`--sources=${source.trim()}`, `--sinks=${sink.trim()}`];
    }},
    { id: 'reachable', label: '$(symbol-boolean) Path-Sensitive Reachability', detail: 'Reachability under recorded conditions', args: async () => {
      const pair = await twoEntities('CKB Path-Sensitive Reachability'); if (!pair) return;
      const conditions = await input('CKB Reachability', 'Optional comma-separated exact conditions', 'authenticated,role==admin');
      return [...pair, ...(conditions?.trim() ? [`--conditions=${conditions.trim()}`] : [])];
    }},
    { id: 'constraints', label: '$(symbol-operator) Symbolic Constraints', detail: 'Equality, inequality, numeric ranges', args: async () => {
      const value = await input('CKB Symbolic Constraints', 'Comma-separated constraints', 'age>=18,age<65,active==true');
      return value?.trim() ? [`--constraints=${value.trim()}`] : undefined;
    }},
    { id: 'concurrency', label: '$(sync) Concurrency Hazards', detail: 'Multi-writers, locks, deadlock cycles', args: async () => [] },
    { id: 'schema-impact', label: '$(database) Schema + Migration Impact', detail: 'DB/schema blast radius', args: async () => oneEntity('CKB Schema Impact') },
    { id: 'infra-impact', label: '$(server) Infrastructure Impact', detail: 'IaC/deployment blast radius', args: async () => oneEntity('CKB Infrastructure Impact') },
    { id: 'config-impact', label: '$(settings-gear) Config + Feature Flag Causality', detail: 'Configuration dependents', args: async () => oneEntity('CKB Configuration Impact') },
    { id: 'distributed-flow', label: '$(radio-tower) Distributed/Event Flow', detail: 'Queue, topic, event, job, service flow', args: async () => twoEntities('CKB Distributed Flow') },
    { id: 'contract-diff', label: '$(diff) API/Schema Evolution', detail: 'Backward compatibility classification', args: async () => {
      const before = await filePathArg('CKB Contract Diff', 'Before ApiContract JSON file'); if (!before) return;
      const after = await filePathArg('CKB Contract Diff', 'After ApiContract JSON file'); if (!after) return;
      return [before[0], after[0]];
    }},
    { id: 'tests', label: '$(beaker) Behavioral Test Selection', detail: 'Tests connected to changed entities', args: async () => {
      const changed = await input('CKB Tests for Change', 'Comma-separated changed entity ids');
      return changed?.trim() ? [`--changed=${changed.trim()}`] : undefined;
    }},
    { id: 'policy', label: '$(law) Architecture Invariants', detail: 'Executable architecture policy rules', args: async () => filePathArg('CKB Architecture Policy', 'ArchitectureRule[] JSON file') },
    { id: 'drift-forecast', label: '$(graph-line) Drift Forecast', detail: 'Bounded structural trend forecast (PREDICTED)', args: async () => {
      const counts = await input('CKB Drift Forecast', 'Historical relation counts, comma-separated', '120,128,137,149');
      return counts?.trim() ? [`--edge-counts=${counts.trim()}`] : undefined;
    }},
    { id: 'simulate', label: '$(preview) Proposed Change Simulation', detail: 'Pre-edit impact, always PREDICTED', args: async () => filePathArg('CKB Change Simulation', 'ChangeOperation[] JSON file') },
    { id: 'hotspots', label: '$(flame) Runtime Resource Intelligence', detail: 'Observed CPU/memory/latency/error hotspots', args: async () => [] },
    { id: 'failure-propagation', label: '$(warning) Failure Propagation', detail: 'Cascading dependency impact', args: async () => oneEntity('CKB Failure Propagation', 'Failed dependency/resource entity id') },
    { id: 'temporal-diff', label: '$(history) Temporal Architecture', detail: 'Architecture evidence diff across snapshots', args: async () => filePathArg('CKB Temporal Architecture', 'Older DeepCausalityEngine bundle') },
    { id: 'cross-repo', label: '$(repo) Cross-Repository Architecture', detail: 'Causal path across repo boundaries', args: async () => twoEntities('CKB Cross-Repository Path') },
    { id: 'ownership', label: '$(organization) Ownership + Bus Factor', detail: 'Socio-technical ownership risk', args: async () => [] },
    { id: 'quality', label: '$(dashboard) Architecture Quality Metrics', detail: 'Evidence-derived coupling/cycles/instability', args: async () => [] },
  ];
}

async function ensureBundle(workspace: string): Promise<string> {
  const bundle = bundlePath(workspace);
  await vscode.window.withProgress({
    location: vscode.ProgressLocation.Notification,
    title: 'CKB: building Deep Software Causality evidence…',
    cancellable: false,
  }, async () => {
    await run(['build', workspace, '--output', '.ckb/deep-causality.json'], workspace);
  });
  return bundle;
}

async function showJson(title: string, result: any) {
  const document = await vscode.workspace.openTextDocument({
    language: 'json',
    content: JSON.stringify({ title, generatedAt: new Date().toISOString(), result }, null, 2),
  });
  await vscode.window.showTextDocument(document, { preview: true, viewColumn: vscode.ViewColumn.Beside });
}

export function activateDeepCausalityV131(context: vscode.ExtensionContext) {
  const disposable = vscode.commands.registerCommand('ckb.deepCausality', async () => {
    const workspace = root();
    if (!workspace) {
      vscode.window.showWarningMessage('CKB: Open a workspace folder first.');
      return;
    }
    try {
      const bundle = await ensureBundle(workspace);
      const chosen = await vscode.window.showQuickPick(operations(), {
        title: 'CKB V13.1 • Deep Software Causality',
        placeHolder: 'Choose an evidence-backed architecture/software intelligence operation',
        matchOnDetail: true,
      });
      if (!chosen) return;
      const args = await chosen.args();
      if (!args) return;
      const result = await run(['--bundle', bundle, chosen.id, ...args], workspace);
      await showJson(chosen.label.replace(/^\$\([^)]*\)\s*/, ''), result);
    } catch (error: any) {
      const message = String(error?.message || error);
      if (/enoent|not recognized|command not found/i.test(message)) {
        vscode.window.showErrorMessage('CKB Deep Causality binary is unavailable. Install/build the V13.1 CKB CLI and set ckb.causalityBinary if it is not on PATH.');
      } else {
        vscode.window.showErrorMessage(`CKB Deep Causality failed: ${message}`);
      }
    }
  });
  context.subscriptions.push(disposable);
  return disposable;
}
