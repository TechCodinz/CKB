# Open Source Strategy — What to Publish, What to Keep Private

This is deliberately concrete: exact folders, exact reasoning, exact steps —
not a general "consider open-sourcing" essay. This is your call to make; this
document gives you what you need to make it, not a recommendation you should
take on faith.

## The split

### Open source (public GitHub repo)

| Path | Why |
|---|---|
| `core/` | The analysis engine. This is what builds trust — anyone considering an extension that reads their whole codebase should be able to see exactly what it does with that access. |
| `cli/` | The `ckb-cli` binary. Needs to be public for the release pipeline (`.github/workflows/release.yml`) and `cargo install`/crates.io publishing to work at all. |
| `mcp-server/` | The REST/MCP server. Same trust argument — it's the thing every IDE extension and MCP client talks to. |
| `bindings/` | Node, Python, WASM SDKs. Thin HTTP clients, no business logic — no reason to hide these, and hiding them actively hurts adoption. |
| `integrations/vscode/`, `integrations/jetbrains/` | The IDE extensions themselves. Marketplace review processes (especially VS Code's) go smoother with a public source repo, and — again — this is code that reads a stranger's entire codebase. |
| `.github/` | CI workflows and the `ckb-scan` GitHub Action. The Action specifically **requires** the repo (or at least this path) to be public — `uses: TechCodinz/CKB/.github/actions/ckb-scan@main` from another repo needs public visibility or per-repo permission grants most people won't bother configuring. |
| `LICENSE`, root `README.md`, `.env.example` | Standard OSS repo hygiene. `.env.example` has no real secrets (confirmed during the audit — only placeholders). |
| `AUDIT_REPORT.md`, `FEATURES_ADDED.md`, `BUILD_INSTRUCTIONS.md` | Your call, but I'd lean toward keeping these public. They're evidence of real security/reliability rigor, which is exactly the kind of thing that builds trust for a tool with deep codebase access. If that feels like too much airing of "here's every bug we found," you can trim them to a shorter public SECURITY.md before publishing — but don't just delete the history, since "we audit and fix things" is a better look than silence. |

### Closed source (private repo)

| Path | Why |
|---|---|
| `backend/` | Auth, JWT issuance, Stripe/Flutterwave/Coinbase payment integration, Prisma schema, usage-based billing, the violation feedback loop. This is your actual monetization layer — open-sourcing it hands competitors your exact pricing/billing mechanics and makes auth bypass trivial to find (not because the code is insecure, but because "here's exactly how the paywall works" is free reconnaissance for anyone who wants to route around it). |
| `web/` | The dashboard — login, billing UI, project views. Tied directly to the backend above. Could go either way long-term (Supabase open-sources their dashboard; Sentry doesn't), but there's no urgency to decide this now — keep it closed until the business model is proven, revisit later if you want the "fully open core" trust signal. |
| The `ckb-backend-api` service block in `render.yaml` | Currently defined in the same file as the public `ckb-mcp-server` service — see "Concrete steps" below, this file needs to be split, not just copied. |
| `web/vercel.json`, root `vercel.json` | Deployment config for the closed dashboard. |

## Why this split specifically (not "open everything" or "closed everything")

This is the same pattern Sentry, Supabase, GitLab (CE/EE), n8n, and PostHog
all use: **open the tool people install and run against their own
code/infrastructure, keep closed the hosted service that bills them for it.**
It's well-tested as a distribution strategy for exactly your situation
(dev tool + hosted SaaS + marketplace extension), not something novel I'm
proposing.

The engine/CLI/MCP-server/extensions genuinely benefit from being public:
distribution (GitHub discovery, MCP ecosystem discovery), trust (auditable
code with deep filesystem access), and plain mechanics (the GitHub Action
needs it). The backend genuinely benefits from staying private: it's where
the actual differentiation-that-makes-money lives, and it's the one part
where "anyone can read exactly how this works" is a cost with no
corresponding trust benefit — nobody decides to trust your extension because
your Stripe webhook handler is public.

## Concrete steps to actually do this

1. **Decide on repo names.** Suggested: `TechCodinz/ckb` (public, currently
   what you have) and `TechCodinz/ckb-cloud` (private, new).

2. **`render.yaml` has been split already** — the one in this repo now only
   defines `ckb-mcp-server` (public), with corrected env vars (it was
   missing `CKB_API_KEY`/`CKB_BACKEND_URL`/`CKB_INTERNAL_SECRET`/
   `ANTHROPIC_API_KEY` entirely, had a stray unused `DATABASE_URL`, and was
   missing `CKB_BIND_ALL=1` — without that last one, the service binds
   `127.0.0.1` by default and Render's proxy can't reach it, so the deploy
   would look successful but the health check would fail). The
   `ckb-backend-api` half now lives in a separate file delivered alongside
   this document, `render-backend-private-repo.yaml` — move it into the
   private repo (as `render.yaml`) once that repo exists. It also had a
   real gap: `INTERNAL_API_SECRET` wasn't in the original config at all,
   despite the Coinbase webhook and MCP server auth depending on it.

3. **Use `git filter-repo` to split with history intact**, rather than just
   copying files (which loses all git blame/history). Install it
   (`pip install git-filter-repo` or `brew install git-filter-repo`), then
   from a **fresh clone** (never run filter-repo on your only copy):
   ```bash
   # Public repo — keep only these paths
   git clone <your-repo> ckb-public && cd ckb-public
   git filter-repo --path core/ --path cli/ --path mcp-server/ \
     --path bindings/ --path integrations/ --path .github/ \
     --path LICENSE --path README.md --path .env.example \
     --path AUDIT_REPORT.md --path FEATURES_ADDED.md --path BUILD_INSTRUCTIONS.md \
     --path render.yaml
   # push this to the new/existing public repo

   # Private repo — keep only these paths
   git clone <your-repo> ckb-private && cd ckb-private
   git filter-repo --path backend/ --path web/ --path vercel.json
   # push this to a new private repo
   ```

4. **Before pushing the public repo, scan its FULL HISTORY for secrets, not
   just the current files.** I fixed every hardcoded secret I found in the
   current codebase during the audit, but I have no visibility into your
   actual git commit history — if a real API key, database URL, or secret
   was ever committed and later removed, `git filter-repo` will still carry
   it forward in old commits unless you also purge it. Run something like
   [`gitleaks`](https://github.com/gitleaks/gitleaks) or
   [`trufflehog`](https://github.com/trufflesecurity/trufflehog) against the
   full history before making the public repo public:
   ```bash
   gitleaks detect --source ckb-public --log-opts="--all"
   ```
   If it finds anything, rotate that credential immediately (assume it's
   compromised the moment it was committed, regardless of whether the repo
   was ever public) and use `git filter-repo --path <file> --invert-paths`
   or `--replace-text` to scrub it from history before publishing.

5. **Wire the two together via what's already built.** The MCP server's
   `CKB_BACKEND_URL`/`CKB_INTERNAL_SECRET` (for per-user API key validation
   and usage billing) already assumes exactly this split — the public engine
   calls out to a private backend URL you control. No code changes needed
   for this part; it was built with this separation in mind from the usage-
   billing work.

6. **Add a `SECURITY.md`** to the public repo (VS Code Marketplace and
   general OSS best practice) with a real contact/reporting path — even a
   simple "email X to report a vulnerability" is better than nothing, and
   signals the project is maintained.

## What NOT to do

- **Don't** put both in one repo with mixed licenses (e.g. `backend/` under
  a proprietary LICENSE, everything else MIT). This is legally messier than
  it looks and doesn't actually protect anything — anyone who clones the
  repo has the backend code regardless of what the LICENSE file claims.
- **Don't** open-source before the git-history secret scan in step 4. A
  public repo's history is effectively permanent (forks, clones, and
  archives happen within minutes of publishing) — this is the one step
  where "I'll check later" isn't recoverable.
