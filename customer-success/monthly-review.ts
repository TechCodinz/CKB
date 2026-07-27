export const monthlyReviewEmail = (user: { name: string; stats: any }) => `
Subject: Your CKB Monthly Report: ${user.stats.violationsFixed} violations fixed!

Hi ${user.name},

Here's your CKB activity for the past 30 days:

📊 **Your Impact**
- Projects scanned: ${user.stats.projectsScanned}
- Files analyzed: ${user.stats.filesAnalyzed}
- Violations detected: ${user.stats.violationsDetected}
- Violations fixed: ${user.stats.violationsFixed}
- Estimated debt prevented: ${user.stats.debtPrevented} hours

🏆 **Achievements**
${user.stats.achievements.map((a: string) => `- ${a}`).join('\n')}

🔍 **Top Issues**
${user.stats.topIssues.map((i: any) => `- ${i.message} (${i.severity})`).join('\n')}

💡 **Recommended Actions**
${user.stats.recommendations.map((r: string) => `- ${r}`).join('\n')}

[View full report](https://app.ckb.dev/reports)

Ready to level up? Your team could benefit from CKB Team:
- Share architectural rules across your org
- Team dashboard with aggregated metrics
- Priority support

[Upgrade to Team](https://app.ckb.dev/upgrade)

Keep building great architecture!

Best,
The CKB Team
`;
