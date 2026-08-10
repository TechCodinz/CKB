import * as vscode from 'vscode';
import * as core from './extension';
import { activateGrowthExperience } from './growthExperience';
import { activateEditorSemanticRealityV10 } from './editorSemanticRealityV10';
import { activateCloudUriHandlerV11 } from './cloudUriHandlerV11';
import { restoreIntelligence } from './intelligence';

export async function activate(context: vscode.ExtensionContext) {
    const api = await core.activate(context);
    await activateGrowthExperience(context);
    const semanticReality = activateEditorSemanticRealityV10(context, () => {
        const root = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath || '';
        return root ? restoreIntelligence(context, root) : undefined;
    });
    const cloudUriHandler = activateCloudUriHandlerV11(context);

    // Internal, read-only context bridge used by IDE → Cloud continuity. The
    // returned object contains navigation/evidence metadata only; no source text
    // is serialized into the handoff URL.
    context.subscriptions.push(vscode.commands.registerCommand('ckb.getSemanticRealityContext', () => {
        return (semanticReality as any).current;
    }));

    context.subscriptions.push(semanticReality);
    return {
        ...api,
        editorSemanticReality: semanticReality,
        cloudUriHandler,
    };
}

export function deactivate() {
    return core.deactivate();
}
