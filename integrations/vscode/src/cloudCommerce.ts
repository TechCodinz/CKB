import * as vscode from 'vscode';

function cloudBaseUrl() {
    return vscode.workspace
        .getConfiguration('ckb')
        .get<string>('cloudSiteUrl', 'https://ckb-nu.vercel.app')
        .replace(/\/$/, '');
}

async function openCloudPath(pathname: string, params: Record<string, string> = {}) {
    const uri = vscode.Uri.parse(`${cloudBaseUrl()}${pathname.startsWith('/') ? pathname : `/${pathname}`}`);
    const query = new URLSearchParams(uri.query);
    query.set('from', 'vscode');
    query.set(
        'extensionVersion',
        String(vscode.extensions.getExtension('TechCodinz.ckb-vscode')?.packageJSON?.version || 'unknown'),
    );
    for (const [key, value] of Object.entries(params)) {
        if (value) query.set(key, value.slice(0, 180));
    }
    await vscode.env.openExternal(uri.with({ query: query.toString() }));
}

async function showCloudMenu() {
    const selection = await vscode.window.showQuickPick(
        [
            { label: '$(rocket) Explore Plans & Upgrade', description: 'Open CKB Cloud pricing', action: 'upgrade' },
            { label: '$(account) Sign In to Cloud', description: 'Continue with your CKB Cloud account', action: 'sign-in' },
            { label: '$(credit-card) Manage Cloud Subscription', description: 'Open billing and subscription controls', action: 'manage' },
            { label: '$(globe) Visit CKB Cloud', description: 'Open the CKB Cloud experience', action: 'visit' },
        ],
        { title: 'CKB Cloud', placeHolder: 'Choose a Cloud action' },
    );

    switch (selection?.action) {
        case 'upgrade':
            return openCloudPath('/pricing', { intent: 'upgrade' });
        case 'sign-in':
            return openCloudPath('/login', { intent: 'sign-in' });
        case 'manage':
            return openCloudPath('/billing', { intent: 'manage-subscription' });
        case 'visit':
            return openCloudPath('/pricing', { intent: 'explore' });
        default:
            return undefined;
    }
}

export function activateCloudCommerce(context: vscode.ExtensionContext) {
    const status = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Right, 40);
    status.text = '$(rocket) CKB Cloud';
    status.tooltip = 'Open CKB Cloud plans, sign-in and subscription actions';
    status.command = 'ckb.cloudMenu';
    status.show();

    context.subscriptions.push(
        status,
        vscode.commands.registerCommand('ckb.cloudMenu', showCloudMenu),
        vscode.commands.registerCommand('ckb.visitCloudSite', () => openCloudPath('/pricing', { intent: 'explore' })),
        vscode.commands.registerCommand('ckb.upgradePlan', () => openCloudPath('/pricing', { intent: 'upgrade' })),
        vscode.commands.registerCommand('ckb.manageSubscription', () => openCloudPath('/billing', { intent: 'manage-subscription' })),
        vscode.commands.registerCommand('ckb.signInCloud', () => openCloudPath('/login', { intent: 'sign-in' })),
    );

    return { openCloudPath, showCloudMenu };
}
