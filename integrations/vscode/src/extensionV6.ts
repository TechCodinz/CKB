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
