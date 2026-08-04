// Posts (or updates, on re-runs) a single PR comment summarizing a CKB scan's
// architectural drift/violations. Uses only Node's built-in fetch (Node 18+,
// which every GitHub-hosted runner ships) — no extra dependencies to install,
// so this step doesn't need its own `npm install`.
//
// Env vars (all provided by the composite action / GitHub Actions runtime):
//   GITHUB_TOKEN     - token with `pull-requests: write` permission
//   GITHUB_REPOSITORY - "owner/repo"
//   GITHUB_EVENT_PATH - path to the pull_request event payload JSON
//   REPORT_PATH       - path to the ckb-report.json produced by `ckb-cli check`
//   FAIL_ON           - the configured failure threshold, for display only

const fs = require('fs');

const MARKER = '<!-- ckb-architecture-check -->';

function severityEmoji(sev) {
    switch (String(sev).toLowerCase()) {
        case 'critical': return '🟥';
        case 'error': return '🟧';
        case 'warning': return '🟨';
        default: return '⬜';
    }
}

function buildCommentBody(report, failOn) {
    const violations = report.drift || [];
    const counts = { Critical: 0, Error: 0, Warning: 0, Info: 0 };
    for (const v of violations) {
        const sev = v.severity || 'Info';
        if (counts[sev] !== undefined) counts[sev] += 1;
    }

    const lines = [MARKER, '## 🏗️ CKB Architecture Check'];

    lines.push('');
    lines.push(
        `Scanned **${report.files_processed}** files — **${report.nodes}** nodes, ` +
        `**${report.edges}** edges — in ${(report.duration_ms / 1000).toFixed(1)}s.`
    );
    lines.push('');

    if (violations.length === 0) {
        lines.push('✅ No architectural drift detected.');
    } else {
        lines.push(
            `Found **${violations.length}** violation(s): ` +
            `${counts.Critical} critical, ${counts.Error} error, ${counts.Warning} warning, ${counts.Info} info ` +
            `(build fails at **${String(failOn).toLowerCase()}** or above).`
        );
        lines.push('');
        lines.push('| | Severity | Rule | Location | Message |');
        lines.push('|---|---|---|---|---|');

        const sorted = [...violations].sort((a, b) => {
            const order = { Critical: 0, Error: 1, Warning: 2, Info: 3 };
            return (order[a.severity] ?? 4) - (order[b.severity] ?? 4);
        });

        const shown = sorted.slice(0, 25);
        for (const v of shown) {
            const location = v.from || v.boundary || '';
            const message = (v.message || '').replace(/\|/g, '\\|').slice(0, 140);
            lines.push(`| ${severityEmoji(v.severity)} | ${v.severity} | ${v.kind || ''} | \`${location}\` | ${message} |`);
        }
        if (sorted.length > shown.length) {
            lines.push('');
            lines.push(`_...and ${sorted.length - shown.length} more. See the full \`ckb-report.json\` artifact for the complete list._`);
        }
    }

    lines.push('');
    lines.push('_Posted automatically by the CKB Architecture Check GitHub Action._');
    return lines.join('\n');
}

async function githubRequest(url, token, options = {}) {
    const res = await fetch(url, {
        ...options,
        headers: {
            'Authorization': `Bearer ${token}`,
            'Accept': 'application/vnd.github+json',
            'X-GitHub-Api-Version': '2022-11-28',
            'Content-Type': 'application/json',
            ...(options.headers || {}),
        },
    });
    if (!res.ok) {
        const body = await res.text();
        throw new Error(`GitHub API ${options.method || 'GET'} ${url} -> ${res.status}: ${body}`);
    }
    return res.status === 204 ? null : res.json();
}

async function main() {
    const token = process.env.GITHUB_TOKEN;
    const repo = process.env.GITHUB_REPOSITORY; // "owner/repo"
    const eventPath = process.env.GITHUB_EVENT_PATH;
    const reportPath = process.env.REPORT_PATH || 'ckb-report.json';
    const failOn = process.env.FAIL_ON || 'error';

    if (!token || !repo || !eventPath) {
        console.log('Missing required GitHub Actions environment variables — skipping PR comment.');
        return;
    }

    const event = JSON.parse(fs.readFileSync(eventPath, 'utf8'));
    const prNumber = event.pull_request && event.pull_request.number;
    if (!prNumber) {
        console.log('Not a pull_request event (no PR number found) — skipping PR comment.');
        return;
    }

    let report;
    try {
        report = JSON.parse(fs.readFileSync(reportPath, 'utf8'));
    } catch (err) {
        console.log(`Could not read ${reportPath} (${err.message}) — skipping PR comment.`);
        return;
    }

    const body = buildCommentBody(report, failOn);
    const base = `https://api.github.com/repos/${repo}`;

    // Find an existing CKB comment on this PR to update, rather than
    // stacking a new comment on every push — keeps the PR readable across
    // many commits/re-runs.
    const comments = await githubRequest(`${base}/issues/${prNumber}/comments?per_page=100`, token);
    const existing = comments.find(c => typeof c.body === 'string' && c.body.includes(MARKER));

    if (existing) {
        await githubRequest(`${base}/issues/comments/${existing.id}`, token, {
            method: 'PATCH',
            body: JSON.stringify({ body }),
        });
        console.log(`Updated existing CKB comment (id ${existing.id}) on PR #${prNumber}.`);
    } else {
        await githubRequest(`${base}/issues/${prNumber}/comments`, token, {
            method: 'POST',
            body: JSON.stringify({ body }),
        });
        console.log(`Posted new CKB comment on PR #${prNumber}.`);
    }
}

main().catch(err => {
    // A failure to post the comment shouldn't fail the whole workflow on its
    // own — the actual pass/fail gate is the "Fail if CKB check failed" step
    // in action.yml, which reads the real check exit code independently.
    console.error('Failed to post CKB PR comment:', err.message);
});
