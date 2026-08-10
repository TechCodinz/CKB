import * as path from 'path';
import * as vscode from 'vscode';
import type { IntelligenceState } from './intelligence';
import { fetchRuntimeReality, type RuntimeRealityFeed, type RuntimeTraceStep } from './runtimeReality';

export type EditorSemanticDepth = 'line' | 'call' | 'symbol' | 'file' | 'subsystem' | 'system';

type SymbolReality = {
    name: string;
    kind: string;
    range: vscode.Range;
    selectionRange: vscode.Range;
};

type ExactHop = {
    traceId: string;
    index: number;
    count: number;
    source: string;
    target: string;
    operation: string;
    flowType: string;
    durationMs: number;
    error: boolean;
    role: 'source' | 'target';
};

export type EditorSemanticRealityContext = {
    version: 'ckb-editor-semantic-reality-v10';
    depth: EditorSemanticDepth;
    depthMode: 'auto' | 'manual';
    system: string;
    subsystem: string;
    file: string;
    line: number;
    column: number;
    word: string;
    symbol?: SymbolReality;
    activity?: {
        role?: string;
        fanIn: number;
        fanOut: number;
        activityIndex: number;
        changeSensitivityIndex: number;
        runtimeObserved: boolean;
    };
    exactHop?: ExactHop;
    runtimeOnline: boolean;
    runtimeObserved: boolean;
    evidencePolicy: 'static-runtime-predicted-separated';
    synthetic: false;
};

const DEPTHS: EditorSemanticDepth[] = ['line', 'call', 'symbol', 'file', 'subsystem', 'system'];

function slash(value: unknown) {
    return String(value || '').replace(/\\/g, '/').replace(/^\.\//, '').replace(/^\/+/, '');
}

function sameFile(a: unknown, b: unknown) {
    const x = slash(a).toLowerCase();
    const y = slash(b).toLowerCase();
    if (!x || !y) return false;
    return x === y || x.endsWith(`/${y}`) || y.endsWith(`/${x}`);
}

function identityFile(value: unknown) {
    return slash(String(value || '').split('::')[0]);
}

function identitySymbol(value: unknown) {
    const text = String(value || '');
    return text.includes('::') ? text.slice(text.lastIndexOf('::') + 2) : '';
}

function inferFlowType(step: RuntimeTraceStep) {
    const text = `${step.flowType || ''} ${step.operation || ''} ${step.protocol || ''} ${step.dbSystem || ''} ${step.messagingSystem || ''}`.toLowerCase();
    if (/websocket|\bws\b|\bwss\b/.test(text)) return 'websocket';
    if (/redis|cache/.test(text)) return 'cache';
    if (/postgres|mysql|sqlite|mongo|prisma|database|\bsql\b/.test(text)) return 'database';
    if (/queue|kafka|rabbit|bull|sqs|pubsub|message/.test(text)) return 'queue';
    if (/event/.test(text)) return 'event';
    if (/http|rpc|fetch|request|response/.test(text)) return 'http';
    return 'function';
}

function subsystemFor(relativeFile: string) {
    const parts = slash(relativeFile).split('/').filter(Boolean);
    const ignored = new Set(['src', 'app', 'apps', 'lib', 'libs', 'packages', 'pkg', 'source']);
    const meaningful = parts.filter((part, index) => index < parts.length - 1 && !ignored.has(part.toLowerCase()));
    if (!meaningful.length) return parts.length > 1 ? parts[0] : 'workspace-root';
    return meaningful.slice(0, 2).join('/');
}

function symbolKind(kind: vscode.SymbolKind) {
    return vscode.SymbolKind[kind] || 'Symbol';
}

function flattenDocumentSymbols(rows: Array<vscode.DocumentSymbol | vscode.SymbolInformation> | undefined): SymbolReality[] {
    const output: SymbolReality[] = [];
    const walk = (row: vscode.DocumentSymbol | vscode.SymbolInformation) => {
        if ('location' in row) {
            output.push({
                name: row.name,
                kind: symbolKind(row.kind),
                range: row.location.range,
                selectionRange: row.location.range,
            });
            return;
        }
        output.push({
            name: row.name,
            kind: symbolKind(row.kind),
            range: row.range,
            selectionRange: row.selectionRange,
        });
        for (const child of row.children || []) walk(child);
    };
    for (const row of rows || []) walk(row);
    return output;
}

function narrowestSymbol(symbols: SymbolReality[], position: vscode.Position) {
    return symbols
        .filter(symbol => symbol.range.contains(position))
        .sort((a, b) => {
            const aLines = a.range.end.line - a.range.start.line;
            const bLines = b.range.end.line - b.range.start.line;
            return aLines - bLines;
        })[0];
}

function hotspotFor(state: IntelligenceState | undefined, relativeFile: string, symbol?: SymbolReality) {
    const activity = state?.activity || state?.bundle?.activity;
    const hotspots: any[] = Array.isArray(activity?.hotspots) ? activity.hotspots : [];
    return hotspots.find(node => {
        const nodePath = slash(node?.path || String(node?.id || '').split('::')[0]);
        const nodeName = String(node?.name || identitySymbol(node?.id) || '');
        if (!sameFile(nodePath, relativeFile)) return false;
        if (!symbol) return true;
        return nodeName === symbol.name || String(node?.id || '').endsWith(`::${symbol.name}`);
    }) || hotspots.find(node => sameFile(node?.path || String(node?.id || '').split('::')[0], relativeFile));
}

function exactHopFor(runtime: RuntimeRealityFeed | undefined, relativeFile: string, symbol?: SymbolReality): ExactHop | undefined {
    if (!(runtime?.replaySafe && runtime.traceSemantics === 'exact-observed-span-instances')) return undefined;
    const symbolName = symbol?.name || '';
    const candidates: ExactHop[] = [];
    for (const [traceId, rawSteps] of Object.entries(runtime.traces || {})) {
        const steps = Array.isArray(rawSteps) ? rawSteps : [];
        steps.forEach((step, index) => {
            const source = String(step?.source || '');
            const target = String(step?.target || '');
            const sourceFile = identityFile(source);
            const targetFile = identityFile(target);
            const sourceSymbol = identitySymbol(source);
            const targetSymbol = identitySymbol(target);
            const sourceMatches = sameFile(sourceFile, relativeFile) && (!symbolName || sourceSymbol === symbolName || !sourceSymbol);
            const targetMatches = sameFile(targetFile, relativeFile) && (!symbolName || targetSymbol === symbolName || !targetSymbol);
            if (!sourceMatches && !targetMatches) return;
            candidates.push({
                traceId,
                index,
                count: steps.length,
                source,
                target,
                operation: String(step?.operation || 'observed transition'),
                flowType: inferFlowType(step),
                durationMs: Number(step?.durationMs || 0),
                error: step?.error === true,
                role: targetMatches ? 'target' : 'source',
            });
        });
    }
    return candidates.sort((a, b) => Number(b.error) - Number(a.error) || b.durationMs - a.durationMs)[0];
}

function autoDepth(editor: vscode.TextEditor, symbol?: SymbolReality): EditorSemanticDepth {
    if (!editor.selection.isEmpty) return 'line';
    const visibleLines = editor.visibleRanges.reduce((sum, range) => sum + Math.max(1, range.end.line - range.start.line + 1), 0);
    const word = editor.document.getText(editor.document.getWordRangeAtPosition(editor.selection.active));
    if (symbol && word && visibleLines <= 38) return 'call';
    if (symbol && visibleLines <= 90) return 'symbol';
    if (visibleLines <= 220) return 'file';
    if (visibleLines <= 520) return 'subsystem';
    return 'system';
}

export class CkbEditorSemanticRealityV10 implements vscode.Disposable {
    private readonly disposables: vscode.Disposable[] = [];
    private readonly status: vscode.StatusBarItem;
    private readonly semanticDecoration: vscode.TextEditorDecorationType;
    private readonly runtimeDecoration: vscode.TextEditorDecorationType;
    private runtime?: RuntimeRealityFeed;
    private runtimeTimer?: ReturnType<typeof setTimeout>;
    private disposed = false;
    private refreshToken = 0;
    private manualDepth: EditorSemanticDepth | undefined;
    private current?: EditorSemanticRealityContext;

    constructor(
        private readonly context: vscode.ExtensionContext,
        private readonly getState: () => IntelligenceState | undefined,
    ) {
        this.status = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Right, 88);
        this.status.command = 'ckb.inspectSemanticReality';
        this.status.name = 'CKB Semantic Editor Reality';
        this.status.show();

        this.semanticDecoration = vscode.window.createTextEditorDecorationType({
            isWholeLine: false,
            borderWidth: '0 0 1px 0',
            borderStyle: 'solid',
            borderColor: new vscode.ThemeColor('editorInfo.foreground'),
            overviewRulerColor: new vscode.ThemeColor('editorInfo.foreground'),
            overviewRulerLane: vscode.OverviewRulerLane.Right,
        });
        this.runtimeDecoration = vscode.window.createTextEditorDecorationType({
            isWholeLine: true,
            backgroundColor: new vscode.ThemeColor('editor.findMatchHighlightBackground'),
            borderWidth: '0 0 0 3px',
            borderStyle: 'solid',
            borderColor: new vscode.ThemeColor('debugIcon.startForeground'),
            overviewRulerColor: new vscode.ThemeColor('debugIcon.startForeground'),
            overviewRulerLane: vscode.OverviewRulerLane.Full,
        });

        this.disposables.push(
            this.status,
            this.semanticDecoration,
            this.runtimeDecoration,
            vscode.window.onDidChangeActiveTextEditor(() => void this.refresh()),
            vscode.window.onDidChangeTextEditorSelection(event => {
                if (event.textEditor === vscode.window.activeTextEditor) void this.refresh();
            }),
            vscode.window.onDidChangeTextEditorVisibleRanges(event => {
                if (!this.manualDepth && event.textEditor === vscode.window.activeTextEditor) void this.refresh();
            }),
            vscode.workspace.onDidChangeConfiguration(event => {
                if (event.affectsConfiguration('ckb.editorSemanticReality')) void this.refresh();
            }),
            vscode.commands.registerCommand('ckb.inspectSemanticReality', () => this.inspect()),
            vscode.commands.registerCommand('ckb.semanticZoomIn', () => this.shiftDepth(-1)),
            vscode.commands.registerCommand('ckb.semanticZoomOut', () => this.shiftDepth(1)),
            vscode.commands.registerCommand('ckb.semanticZoomAuto', () => {
                this.manualDepth = undefined;
                void this.refresh();
            }),
        );
        context.subscriptions.push(...this.disposables);
        void this.refreshRuntime();
        void this.refresh();
    }

    private workspaceRoot() {
        return vscode.workspace.workspaceFolders?.[0]?.uri.fsPath || '';
    }

    private scheduleRuntime() {
        if (this.runtimeTimer) clearTimeout(this.runtimeTimer);
        if (this.disposed) return;
        const interval = Math.max(1_500, Math.min(vscode.workspace.getConfiguration('ckb').get<number>('runtimePollIntervalMs', 2500), 30_000));
        this.runtimeTimer = setTimeout(() => void this.refreshRuntime(), this.runtime?.observed ? interval : Math.max(interval, 5_000));
    }

    private async refreshRuntime() {
        const root = this.workspaceRoot();
        if (!root || this.disposed) return;
        try {
            this.runtime = await fetchRuntimeReality(root);
            await this.refresh();
        } finally {
            this.scheduleRuntime();
        }
    }

    private relativeFile(editor: vscode.TextEditor) {
        const root = this.workspaceRoot();
        if (!root) return slash(editor.document.uri.fsPath);
        const relative = path.relative(root, editor.document.uri.fsPath);
        return slash(relative.startsWith('..') ? editor.document.uri.fsPath : relative);
    }

    private async symbols(editor: vscode.TextEditor) {
        try {
            const rows = await vscode.commands.executeCommand<Array<vscode.DocumentSymbol | vscode.SymbolInformation>>(
                'vscode.executeDocumentSymbolProvider',
                editor.document.uri,
            );
            return flattenDocumentSymbols(rows);
        } catch {
            return [];
        }
    }

    private shiftDepth(delta: number) {
        const currentDepth = this.current?.depth || autoDepth(vscode.window.activeTextEditor!, undefined);
        const index = Math.max(0, DEPTHS.indexOf(currentDepth));
        this.manualDepth = DEPTHS[Math.max(0, Math.min(DEPTHS.length - 1, index + delta))];
        void this.refresh();
    }

    private clearEditor(editor?: vscode.TextEditor) {
        if (!editor) return;
        editor.setDecorations(this.semanticDecoration, []);
        editor.setDecorations(this.runtimeDecoration, []);
    }

    private async refresh() {
        const token = ++this.refreshToken;
        const editor = vscode.window.activeTextEditor;
        const enabled = vscode.workspace.getConfiguration('ckb').get<boolean>('editorSemanticReality', true);
        if (!editor || !enabled || editor.document.uri.scheme !== 'file') {
            this.status.text = '$(symbol-namespace) CKB Reality';
            this.status.tooltip = 'Open a source file to enter cursor-driven semantic reality.';
            this.current = undefined;
            this.clearEditor(editor);
            return;
        }

        const symbols = await this.symbols(editor);
        if (token !== this.refreshToken || this.disposed) return;
        const position = editor.selection.active;
        const symbol = narrowestSymbol(symbols, position);
        const relativeFile = this.relativeFile(editor);
        const hotspot = hotspotFor(this.getState(), relativeFile, symbol);
        const exactHop = exactHopFor(this.runtime, relativeFile, symbol);
        const wordRange = editor.document.getWordRangeAtPosition(position);
        const word = wordRange ? editor.document.getText(wordRange) : '';
        const depth = this.manualDepth || autoDepth(editor, symbol);
        const system = path.basename(this.workspaceRoot()) || 'workspace';
        const subsystem = subsystemFor(relativeFile);

        this.current = {
            version: 'ckb-editor-semantic-reality-v10',
            depth,
            depthMode: this.manualDepth ? 'manual' : 'auto',
            system,
            subsystem,
            file: relativeFile,
            line: position.line + 1,
            column: position.character + 1,
            word,
            symbol,
            activity: hotspot ? {
                role: hotspot?.role,
                fanIn: Number(hotspot?.fanIn || 0),
                fanOut: Number(hotspot?.fanOut || 0),
                activityIndex: Number(hotspot?.activityIndex || 0),
                changeSensitivityIndex: Number(hotspot?.changeSensitivityIndex || 0),
                runtimeObserved: hotspot?.runtimeObserved === true,
            } : undefined,
            exactHop,
            runtimeOnline: this.runtime?.online === true,
            runtimeObserved: this.runtime?.observed === true,
            evidencePolicy: 'static-runtime-predicted-separated',
            synthetic: false,
        };

        const semanticRange = symbol?.range || wordRange;
        editor.setDecorations(this.semanticDecoration, semanticRange ? [{
            range: semanticRange,
            hoverMessage: new vscode.MarkdownString([
                `**CKB ${depth.toUpperCase()} Reality**`,
                `System: \`${system}\``,
                `Subsystem: \`${subsystem}\``,
                `File: \`${relativeFile}\``,
                symbol ? `Symbol: **${symbol.name}** (${symbol.kind})` : `Cursor: line ${position.line + 1}`,
                hotspot ? `Static graph: ${Number(hotspot?.fanIn || 0)} incoming / ${Number(hotspot?.fanOut || 0)} outgoing` : 'Static activity node: not resolved at this cursor',
            ].join('  \n')),
        }] : []);

        if (exactHop) {
            const runtimeRange = symbol?.selectionRange || wordRange || new vscode.Range(position.line, 0, position.line, Math.max(1, editor.document.lineAt(position.line).text.length));
            const hover = new vscode.MarkdownString([
                `**CKB EXACT OBSERVED ${exactHop.role.toUpperCase()} HOP**`,
                `Trace: \`${exactHop.traceId}\` • step ${exactHop.index + 1}/${exactHop.count}`,
                `Flow: **${exactHop.flowType.toUpperCase()}** • ${exactHop.durationMs.toFixed(2)} ms${exactHop.error ? ' • **ERROR OBSERVED**' : ''}`,
                `\`${exactHop.source}\` → \`${exactHop.target}\``,
                '',
                'This highlight comes from exact observed parent/child runtime evidence. Static dependencies are never promoted to runtime execution.',
            ].join('  \n'));
            editor.setDecorations(this.runtimeDecoration, [{ range: runtimeRange, hoverMessage: hover }]);
        } else {
            editor.setDecorations(this.runtimeDecoration, []);
        }

        const label = depth === 'system'
            ? system
            : depth === 'subsystem'
                ? subsystem
                : depth === 'file'
                    ? path.basename(relativeFile)
                    : symbol?.name || word || `line ${position.line + 1}`;
        this.status.text = `${exactHop ? '$(pulse)' : '$(symbol-namespace)'} CKB ${depth.toUpperCase()}: ${label}`;
        this.status.tooltip = new vscode.MarkdownString([
            '**CKB Cursor-Driven Semantic Reality V10**',
            `Depth: **${depth.toUpperCase()}** (${this.manualDepth ? 'manual' : 'auto from selection + editor viewport'})`,
            `Path: ${system} → ${subsystem} → ${relativeFile}${symbol ? ` → ${symbol.name}` : ''}`,
            hotspot ? `Architecture: ${Number(hotspot?.fanIn || 0)} in / ${Number(hotspot?.fanOut || 0)} out • change sensitivity ${(Number(hotspot?.changeSensitivityIndex || 0) * 100).toFixed(0)}%` : 'Architecture activity: unresolved at this exact cursor',
            exactHop ? `Runtime: exact observed ${exactHop.flowType} hop • ${exactHop.durationMs.toFixed(2)} ms${exactHop.error ? ' • error' : ''}` : this.runtime?.observed ? 'Runtime: observed elsewhere, no exact hop maps to this cursor' : 'Runtime: no exact observed execution attached',
            '',
            'Use **CKB: Semantic Zoom In/Out** to traverse LINE → CALL → SYMBOL → FILE → SUBSYSTEM → SYSTEM. Use Auto to let selection and visible editor scale choose the depth.',
        ].join('  \n'));
    }

    private async inspect() {
        const context = this.current;
        if (!context) {
            vscode.window.showInformationMessage('CKB: Open a source file to inspect semantic reality.');
            return;
        }
        const items: Array<vscode.QuickPickItem & { command?: string }> = [
            { label: `$(symbol-namespace) ${context.depth.toUpperCase()} • ${context.symbol?.name || context.word || path.basename(context.file)}`, description: `${context.system} → ${context.subsystem} → ${context.file}` },
            { label: '$(zoom-in) Semantic Zoom In', description: 'Descend toward line/call reality', command: 'ckb.semanticZoomIn' },
            { label: '$(zoom-out) Semantic Zoom Out', description: 'Ascend toward file/subsystem/system reality', command: 'ckb.semanticZoomOut' },
            { label: '$(sparkle) Return to Auto Semantic Zoom', description: 'Let selection + visible editor scale resolve the depth', command: 'ckb.semanticZoomAuto' },
            { label: '$(git-compare) Analyze Cursor Ripple', description: 'Predict direct/transitive graph impact from this source location', command: 'ckb.impact' },
            { label: '$(database) Query Architecture Memory', description: 'Ask CKB about this symbol/file/subsystem', command: 'ckb.queryMemory' },
            { label: '$(telescope) Open Invisible Reality', description: 'Open the molecular + live architecture cockpit', command: 'ckb.openReality' },
        ];
        if (context.exactHop) {
            items.splice(1, 0, {
                label: `$(pulse) EXACT RUNTIME • ${context.exactHop.flowType.toUpperCase()} • ${context.exactHop.durationMs.toFixed(2)} ms`,
                description: `${context.exactHop.source} → ${context.exactHop.target}`,
            });
        }
        const choice = await vscode.window.showQuickPick(items, {
            title: 'CKB Cursor-Driven Semantic Reality V10',
            placeHolder: 'Traverse the current source reality',
        });
        if (choice?.command) await vscode.commands.executeCommand(choice.command);
    }

    dispose() {
        this.disposed = true;
        if (this.runtimeTimer) clearTimeout(this.runtimeTimer);
        this.runtimeTimer = undefined;
        for (const disposable of this.disposables) disposable.dispose();
    }
}

export function activateEditorSemanticRealityV10(
    context: vscode.ExtensionContext,
    getState: () => IntelligenceState | undefined,
) {
    return new CkbEditorSemanticRealityV10(context, getState);
}
