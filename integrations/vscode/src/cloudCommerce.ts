import * as vscode from 'vscode';

function cloudBaseUrl() {
    return vscode.workspace.getConfiguration('ckb').get<string>('cloudSiteUrl', 'https://ckb-nu.vercel.app').replace(/\/$/, '');
}

async function openCloudPath(pathname: string, params: Record<string, string> = {}) {
    const uri = vscode.Uri.parse(`${cloudBaseUrl()}${pathname.startsWith('/') ? pathname : `/${pathname}`}`);
    const query = new URLSearchParams(uri.query);
    query.set('from', 'vscode');
    query.set('extensionVersion', String(vscode.extensions.getExtension('TechCodinz.ckb-vscode')?.packageJSON?.version || 'unknown'));
    for (const [key, value] of Object.entries(params)) {
        if (value) query.set(key, value.slice(0, 180));
    }
    await vscode.env.openExternal(uri.with({ query: query.toString() }));
}

export function activateCloudCommerce(context: vscode.ExtensionContext) {
    context.subscriptions.push(
        vscode.commands.registerCommand('ckb.visitCloudSite', () => openCloudPath('/pricing', { intent: 'explore' })),
        vscode.commands.registerCommand('ckb.upgradePlan', () => openCloudPath('/pricing', { intent: 'upgrade' })),
        vscode.commands.registerCommand('ckb.manageSubscription', () => openCloudPath('/billing', { intent: 'manage-subscription' })),
        vscode.commands.registerCommand('ckb.signInCloud', () => openCloudPath('/login', { intent: 'sign-in' })),
    );

    return { openCloudPath };
}
