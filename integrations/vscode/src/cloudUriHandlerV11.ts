import * as path from 'path';
import * as vscode from 'vscode';

function slash(value: unknown) {
    return String(value || '').replace(/\\/g, '/').replace(/^\.\//, '').replace(/^\/+/, '');
}

function safePositiveInt(value: string | null, fallback: number) {
    const parsed = Number(value);
    return Number.isFinite(parsed) && parsed > 0 ? Math.floor(parsed) : fallback;
}

function workspaceRoots() {
    return (vscode.workspace.workspaceFolders || []).map(folder => path.resolve(folder.uri.fsPath));
}

function isInside(root: string, candidate: string) {
    const relative = path.relative(root, candidate);
    return relative === '' || (!relative.startsWith('..') && !path.isAbsolute(relative));
}

async function resolveWorkspaceFile(raw: string): Promise<vscode.Uri | undefined> {
    const file = slash(raw);
    if (!file || file.includes('\0')) return undefined;

    for (const root of workspaceRoots()) {
        const candidate = path.resolve(root, file);
        // Cloud continuity is navigation context only. Never allow a URI handoff
        // to escape the user's already-open workspace.
        if (!isInside(root, candidate)) continue;
        const uri = vscode.Uri.file(candidate);
        try {
            await vscode.workspace.fs.stat(uri);
            return uri;
        } catch { /* try another workspace root */ }
    }
    return undefined;
}

/**
 * Bidirectional IDE ↔ Cloud source continuity.
 *
 * Supported URI shape:
 * vscode://TechCodinz.ckb-vscode/open?file=src/a.ts&line=12&column=3&depth=symbol
 *
 * The handler accepts navigation metadata only. It does not accept source text,
 * patches, shell commands, repository credentials or arbitrary filesystem paths.
 */
export class CkbCloudUriHandlerV11 implements vscode.UriHandler, vscode.Disposable {
    private disposed = false;

    constructor(private readonly context: vscode.ExtensionContext) {}

    async handleUri(uri: vscode.Uri) {
        if (this.disposed) return;
        const route = slash(uri.path).toLowerCase();
        if (route !== 'open' && route !== 'xray' && route !== 'reality') {
            vscode.window.showWarningMessage('CKB ignored an unsupported Cloud continuity route.');
            return;
        }

        const params = new URLSearchParams(uri.query);
        const file = params.get('file') || '';
        const target = await resolveWorkspaceFile(file);
        if (!target) {
            const choice = await vscode.window.showWarningMessage(
                `CKB Cloud continuity could not resolve ${file || 'the requested file'} inside the currently open workspace.`,
                'Open Invisible Reality',
            );
            if (choice === 'Open Invisible Reality') await vscode.commands.executeCommand('ckb.openReality');
            return;
        }

        const document = await vscode.workspace.openTextDocument(target);
        const editor = await vscode.window.showTextDocument(document, { preview: false });
        const line = Math.min(document.lineCount, safePositiveInt(params.get('line'), 1));
        const rawColumn = safePositiveInt(params.get('column'), 1);
        const lineText = document.lineAt(Math.max(0, line - 1));
        const column = Math.min(lineText.text.length + 1, rawColumn);
        const position = new vscode.Position(Math.max(0, line - 1), Math.max(0, column - 1));
        editor.selection = new vscode.Selection(position, position);
        editor.revealRange(new vscode.Range(position, position), vscode.TextEditorRevealType.InCenter);

        await vscode.commands.executeCommand('ckb.semanticZoomAuto');
        await vscode.commands.executeCommand('ckb.openReality');

        const depth = slash(params.get('depth') || '').toUpperCase();
        const trace = String(params.get('trace') || '').slice(0, 80);
        const step = params.get('step');
        vscode.window.setStatusBarMessage(
            `CKB Cloud → IDE • ${path.basename(target.fsPath)}:${line}${depth ? ` • ${depth}` : ''}${trace ? ` • trace ${trace.slice(0, 10)}${step ? ` step ${Number(step) + 1}` : ''}` : ''}`,
            5000,
        );

        // Give the editor providers a moment to resolve document symbols after
        // the external URI opened a file, then show the cursor Reality picker.
        setTimeout(() => void vscode.commands.executeCommand('ckb.inspectSemanticReality'), 180);
    }

    dispose() {
        this.disposed = true;
    }
}

export function activateCloudUriHandlerV11(context: vscode.ExtensionContext) {
    const handler = new CkbCloudUriHandlerV11(context);
    context.subscriptions.push(vscode.window.registerUriHandler(handler), handler);
    return handler;
}
