import * as vscode from 'vscode';
import * as core from './extension';
import { activateGrowthExperience } from './growthExperience';
import { activateEditorSemanticRealityV10 } from './editorSemanticRealityV10';
import { restoreIntelligence } from './intelligence';

export async function activate(context: vscode.ExtensionContext) {
    const api = await core.activate(context);
    await activateGrowthExperience(context);
    const semanticReality = activateEditorSemanticRealityV10(context, () => {
        const root = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath || '';
        return root ? restoreIntelligence(context, root) : undefined;
    });
    context.subscriptions.push(semanticReality);
    return {
        ...api,
        editorSemanticReality: semanticReality,
    };
}

export function deactivate() {
    return core.deactivate();
}
