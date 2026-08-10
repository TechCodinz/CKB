import * as vscode from 'vscode';
import * as core from './extension';
import { activateGrowthExperience } from './growthExperience';
import { activateEditorSemanticRealityV10 } from './editorSemanticRealityV10';
import { activateCloudUriHandlerV11 } from './cloudUriHandlerV11';
import { activateCursorGuardedChangeReality } from './cursorChangeReality';
import { activateModelIntelligenceV13 } from './modelIntelligenceV13';
import { activateDeepCausalityV131 } from './deepCausalityV131';
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

    const guardedChangeReality = activateCursorGuardedChangeReality(context, api.transactions);
    const modelIntelligenceV13 = activateModelIntelligenceV13(context);
    const deepCausalityV131 = activateDeepCausalityV131(context);
    context.subscriptions.push(semanticReality, guardedChangeReality, modelIntelligenceV13, deepCausalityV131);
    return {
        ...api,
        editorSemanticReality: semanticReality,
        cloudUriHandler,
        guardedChangeReality,
        modelIntelligenceV13,
        deepCausalityV131,
    };
}

export function deactivate() {
    return core.deactivate();
}
