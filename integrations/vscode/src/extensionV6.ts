import * as core from './extension';
import { activateGrowthExperience } from './growthExperience';

export async function activate(context: import('vscode').ExtensionContext) {
    const api = await core.activate(context);
    await activateGrowthExperience(context);
    return api;
}

export function deactivate() {
    return core.deactivate();
}
