// Transport failure descriptions for CKB HTTP requests.
//
// Node reports connection problems as codes like ECONNREFUSED / ENOTFOUND,
// which mean nothing to someone who has just installed the extension from the
// marketplace. The most common first-run situation — no local CKB CLI and no
// configured server — produced "connect ECONNREFUSED 127.0.0.1:3000", which
// says nothing about what to do next.
//
// Kept free of any `vscode` import so it can be unit tested directly.

export function describeRequestFailure(error: any, baseUrl: string, apiKeyConfigured: boolean): string {
    const code = String(error?.code || '');
    const message = String(error?.message || '');

    if (code === 'ECONNREFUSED' || code === 'EHOSTUNREACH' || code === 'ENETUNREACH') {
        return `No CKB server is reachable at ${baseUrl}. Start a local CKB server, or set "ckb.serverUrl" to your CKB Cloud URL.`;
    }
    if (code === 'ENOTFOUND' || code === 'EAI_AGAIN') {
        return `The CKB server address ${baseUrl} could not be resolved. Check "ckb.serverUrl" for a typo or a DNS problem.`;
    }
    if (code === 'CERT_HAS_EXPIRED' || /certificate|self.signed/i.test(message)) {
        return `The CKB server at ${baseUrl} presented a TLS certificate that could not be verified.`;
    }
    if (/timed out/i.test(message) || code === 'ETIMEDOUT') {
        return `The CKB server at ${baseUrl} did not respond in time. It may be starting up — free hosting tiers can take a minute to wake.`;
    }
    if (/HTTP 401|HTTP 403|unauthor/i.test(message)) {
        return apiKeyConfigured
            ? `${baseUrl} rejected the stored CKB API key. Run "CKB: Set API Key" to replace it.`
            : `${baseUrl} requires authentication. Run "CKB: Set API Key" to store your key.`;
    }
    if (code === 'ECONNRESET' || code === 'EPIPE') {
        return `The connection to ${baseUrl} was closed before CKB finished reading the response.`;
    }
    return message || `The CKB request to ${baseUrl} failed.`;
}
