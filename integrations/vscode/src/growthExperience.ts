import * as path from 'path';
import * as vscode from 'vscode';

const CACHE_PREFIX = 'ckb.ide.intelligence.v1:';
const WELCOME_KEY = 'ckb.invisibleReality.welcome.v6';
const MILESTONE_KEY = 'ckb.invisibleReality.milestones.v1';

function rootPath() {
    return vscode.workspace.workspaceFolders?.[0]?.uri.fsPath || '';
}

function stateKey(root: string) {
    return `${CACHE_PREFIX}${root.replace(/\\/g, '/').toLowerCase()}`;
}

function currentState(context: vscode.ExtensionContext): any | undefined {
    const root = rootPath();
    return root ? context.workspaceState.get<any>(stateKey(root)) : undefined;
}

function activityOf(state: any) {
    return state?.activity || state?.bundle?.activity || {};
}

function dnaOf(state: any) {
    return state?.dna || state?.bundle?.dna || {};
}

function scanOf(state: any) {
    return state?.scan || state?.bundle?.scan || {};
}

function snapshotId(state: any) {
    return String(scanOf(state)?.snapshot_id || scanOf(state)?.snapshotId || '');
}

function safeNumber(value: unknown, fallback = 0) {
    const number = Number(value);
    return Number.isFinite(number) ? number : fallback;
}

function slash(value: string) {
    return value.replace(/\\/g, '/').replace(/^\.\//, '').replace(/^\/+/, '');
}

function activeSourceContext() {
    const editor = vscode.window.activeTextEditor;
    const root = rootPath();
    if (!editor || editor.document.uri.scheme !== 'file') return undefined;
    const absolute = editor.document.uri.fsPath;
    const relative = root ? path.relative(root, absolute) : absolute;
    const file = slash(relative.startsWith('..') ? absolute : relative);
    const position = editor.selection.active;
    const wordRange = editor.document.getWordRangeAtPosition(position);
    const symbol = wordRange ? editor.document.getText(wordRange).trim() : '';
    return {
        file,
        line: position.line + 1,
        column: position.character + 1,
        symbol,
        selected: !editor.selection.isEmpty,
    };
}

function snapshotMarkdown(state: any) {
    const activity = activityOf(state);
    const dna = dnaOf(state);
    const scan = scanOf(state);
    const hotspots = Array.isArray(activity?.hotspots) ? activity.hotspots.slice(0, 5) : [];
    const runtimeCoverage = safeNumber(activity?.runtimeCoveragePct);
    const dnaHealth = Number.isFinite(Number(dna?.overallHealth)) ? `${Number(dna.overallHealth).toFixed(1)}%` : 'not available';
    const nodes = activity?.nodesAnalyzed ?? scan?.nodes ?? 0;
    const edges = activity?.edgesAnalyzed ?? scan?.edges ?? 0;
    const lines = [
        '# CKB Invisible Reality Snapshot',
        '',
        `- Architecture symbols: ${nodes}`,
        `- Relationships: ${edges}`,
        `- Runtime coverage: ${runtimeCoverage.toFixed(1)}%`,
        `- Code DNA health: ${dnaHealth}`,
        `- Evidence policy: STATIC / RUNTIME / PREDICTED remain separated`,
        snapshotId(state) ? `- Snapshot: ${snapshotId(state)}` : '',
        '',
        '## Highest-priority architecture signals',
        ...hotspots.map((node: any, index: number) => {
            const name = String(node?.name || node?.id || 'symbol');
            const role = String(node?.role || 'architecture-symbol');
            const activityIndex = safeNumber(node?.activityIndex);
            const change = safeNumber(node?.changeSensitivityIndex);
            const evidence = node?.runtimeObserved ? 'RUNTIME OBSERVED' : 'STATIC';
            return `${index + 1}. **${name}** — ${role} — activity ${(activityIndex * 100).toFixed(0)} — change sensitivity ${(change * 100).toFixed(0)} — ${evidence}`;
        }),
        '',
        '> Generated locally by CKB. Runtime claims appear only where telemetry was actually observed.',
    ];
    return lines.filter(Boolean).join('\n');
}

async function revealArchitecture() {
    await vscode.commands.executeCommand('ckb.deepActivity');
    await vscode.commands.executeCommand('ckb.openReality');
}

async function shareSnapshot(context: vscode.ExtensionContext) {
    const state = currentState(context);
    if (!state) {
        const choice = await vscode.window.showInformationMessage(
            'CKB needs an architecture analysis before it can create a truthful share snapshot.',
            'Reveal My Architecture',
        );
        if (choice === 'Reveal My Architecture') await revealArchitecture();
        return;
    }
    await vscode.env.clipboard.writeText(snapshotMarkdown(state));
    const choice = await vscode.window.showInformationMessage(
        'CKB copied an evidence-backed architecture snapshot to your clipboard.',
        'Open Invisible Reality',
        'Open Cloud Universe',
    );
    if (choice === 'Open Invisible Reality') await vscode.commands.executeCommand('ckb.openReality');
    if (choice === 'Open Cloud Universe') await openCloudContinuity(context);
}

async function semanticContext() {
    try {
        return await vscode.commands.executeCommand<any>('ckb.getSemanticRealityContext');
    } catch {
        return undefined;
    }
}

async function openCloudContinuity(
    context: vscode.ExtensionContext,
    options: { cursorReality?: boolean; raiziom?: boolean } = {},
) {
    const configured = vscode.workspace.getConfiguration('ckb').get<string>('cloudExplorerUrl', 'https://ckb-nu.vercel.app/project/current');
    const state = currentState(context);
    const cursor = options.cursorReality === false ? undefined : activeSourceContext();
    const semantic = cursor ? await semanticContext() : undefined;
    const uri = vscode.Uri.parse(configured);
    const params = new URLSearchParams(uri.query);
    params.set('from', 'vscode');
    params.set('experience', cursor ? 'semantic-editor-v11' : 'invisible-reality-v11');
    if (state) {
        const activity = activityOf(state);
        const dna = dnaOf(state);
        params.set('symbols', String(activity?.nodesAnalyzed ?? scanOf(state)?.nodes ?? 0));
        params.set('relations', String(activity?.edgesAnalyzed ?? scanOf(state)?.edges ?? 0));
        params.set('runtimeCoverage', safeNumber(activity?.runtimeCoveragePct).toFixed(1));
        if (Number.isFinite(Number(dna?.overallHealth))) params.set('dna', Number(dna.overallHealth).toFixed(1));
        if (snapshotId(state)) params.set('snapshot', snapshotId(state));
    }
    if (cursor) {
        const file = String(semantic?.file || cursor.file);
        const line = Math.max(1, safeNumber(semantic?.line, cursor.line));
        const column = Math.max(1, safeNumber(semantic?.column, cursor.column));
        const symbol = String(semantic?.symbol?.name || semantic?.word || cursor.symbol || '').trim();
        const depth = String(semantic?.depth || (cursor.selected ? 'line' : 'symbol')).toLowerCase();
        params.set('tab', '0');
        params.set('file', slash(file));
        params.set('line', String(Math.round(line)));
        params.set('column', String(Math.round(column)));
        params.set('depth', depth.slice(0, 40));
        if (symbol) params.set('symbol', symbol.slice(0, 180));
        params.set('resume', 'xray');

        const hop = semantic?.exactHop;
        if (hop?.traceId) {
            params.set('trace', String(hop.traceId).slice(0, 180));
            params.set('step', String(Math.max(0, safeNumber(hop.index, 0))));
            if (hop.flowType) params.set('flow', String(hop.flowType).slice(0, 80));
            params.set('runtimeRole', String(hop.role || '').slice(0, 24));
        }
    }
    if (options.raiziom) params.set('raiziom', '1');
    await vscode.env.openExternal(uri.with({ query: params.toString() }));
}

async function showValueBridge(context: vscode.ExtensionContext) {
    const state = currentState(context);
    const activity = activityOf(state);
    const runtime = safeNumber(activity?.runtimeCoveragePct);
    const message = state
        ? `Your local CKB Reality currently maps ${activity?.nodesAnalyzed ?? scanOf(state)?.nodes ?? 0} symbols. Cloud continuity adds the full Living Universe, cross-session architecture history, collaborative exploration and deeper Reality surfaces while preserving evidence classes.`
        : 'Reveal your architecture locally first, then continue into the Cloud Living Universe when you need deeper visual exploration, history and collaboration.';
    const actions = runtime > 0 ? ['Open Cloud Universe', 'Share Snapshot'] : ['Reveal My Architecture', 'Open Cloud Universe'];
    const selected = await vscode.window.showInformationMessage(message, ...actions);
    if (selected === 'Reveal My Architecture') await revealArchitecture();
    if (selected === 'Open Cloud Universe') await openCloudContinuity(context);
    if (selected === 'Share Snapshot') await shareSnapshot(context);
}

async function maybeWelcome(context: vscode.ExtensionContext) {
    if (context.globalState.get<boolean>(WELCOME_KEY)) return;
    await context.globalState.update(WELCOME_KEY, true);
    const selected = await vscode.window.showInformationMessage(
        'CKB Invisible Reality can reveal hidden architecture, change pressure and observed runtime paths inside this workspace.',
        'Reveal My Architecture',
        'Not Now',
    );
    if (selected === 'Reveal My Architecture') await revealArchitecture();
}

async function maybeMilestone(context: vscode.ExtensionContext) {
    const state = currentState(context);
    if (!state) return;
    const activity = activityOf(state);
    const seen = new Set<string>(context.globalState.get<string[]>(MILESTONE_KEY, []));
    const milestones: Array<{ id: string; condition: boolean; text: string }> = [
        { id: 'first-map', condition: safeNumber(activity?.nodesAnalyzed ?? scanOf(state)?.nodes) > 0, text: 'CKB has reconstructed your first software reality.' },
        { id: 'deep-system', condition: safeNumber(activity?.nodesAnalyzed) >= 250, text: 'CKB is now mapping a deep system: 250+ architecture symbols are visible.' },
        { id: 'runtime-seen', condition: safeNumber(activity?.runtimeCoveragePct) > 0, text: 'Runtime telemetry is attached. CKB can now separate observed execution from static possibility.' },
        { id: 'hotspots', condition: Array.isArray(activity?.hotspots) && activity.hotspots.length >= 5, text: 'CKB found multiple high-priority architecture signals worth exploring.' },
    ];
    const next = milestones.find(item => item.condition && !seen.has(item.id));
    if (!next) return;
    seen.add(next.id);
    await context.globalState.update(MILESTONE_KEY, [...seen]);
    const selected = await vscode.window.showInformationMessage(next.text, 'Open Reality', 'Share Snapshot');
    if (selected === 'Open Reality') await vscode.commands.executeCommand('ckb.openReality');
    if (selected === 'Share Snapshot') await shareSnapshot(context);
}

export async function activateGrowthExperience(context: vscode.ExtensionContext) {
    context.subscriptions.push(
        vscode.commands.registerCommand('ckb.revealArchitecture', revealArchitecture),
        vscode.commands.registerCommand('ckb.shareRealitySnapshot', () => shareSnapshot(context)),
        vscode.commands.registerCommand('ckb.openCloudContinuity', () => openCloudContinuity(context)),
        vscode.commands.registerCommand('ckb.continueSemanticRealityInCloud', () => openCloudContinuity(context, { cursorReality: true })),
        vscode.commands.registerCommand('ckb.askRaiziomAboutCursor', () => openCloudContinuity(context, { cursorReality: true, raiziom: true })),
        vscode.commands.registerCommand('ckb.explainCloudValue', () => showValueBridge(context)),
    );

    const guidance = vscode.workspace.getConfiguration('ckb').get<boolean>('productGuidance', true);
    if (!guidance) return;

    setTimeout(() => void maybeWelcome(context), 1200);
    setTimeout(() => void maybeMilestone(context), 5000);
    context.subscriptions.push(vscode.window.onDidChangeWindowState(event => {
        if (event.focused) void maybeMilestone(context);
    }));
}
