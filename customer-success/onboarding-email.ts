export const onboardingEmail = (user: { name: string; plan: string }) => `
Subject: Welcome to CKB! Here's how to get started in 5 minutes

Hi ${user.name},

Welcome to CKB! I'm excited to help you stop architectural drift in your AI-generated code.

Here's your 5-minute onboarding plan:

**Step 1: Install the CLI**
\`\`\`bash
curl -fsSL https://ckb.dev/install.sh | sh
# or
cargo install ckb-cli
\`\`\`

**Step 2: Scan your first project**
\`\`\`bash
cd your-project
ckb scan .
\`\`\`

**Step 3: Install VS Code extension**
- Open VS Code
- Search for "CKB" in extensions
- Click Install

**Step 4: Connect Cursor/Claude (Pro plan)**
Add to your MCP settings:
\`\`\`json
{
  "mcpServers": {
    "ckb": {
      "command": "ckb",
      "args": ["serve"]
    }
  }
}
\`\`\`

**Step 5: View your dashboard**
https://app.ckb.dev/dashboard

---

**Pro tips:**
- Run \`ckb check --strict\` in CI to block PRs with violations
- Set up custom rules in \`.ckb.toml\`
- Join our [Discord](https://discord.gg/ckb) for support

Questions? Just reply to this email—I'm here to help!

Best,
[Founder Name]
Founder, CKB
`;
