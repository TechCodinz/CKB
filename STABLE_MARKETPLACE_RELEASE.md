# CKB Stable VS Code Marketplace Release

This release lane is intentionally based on `main`, not V13. It exists so the current stable extension can be compiled, tested, packaged and published without GitHub-hosted Actions.

## Safety rules

- Do not merge or cherry-pick V13 implementation code into this branch for a stable Marketplace release.
- Do not publish from a dirty checkout.
- Do not commit the Visual Studio Marketplace PAT/token.
- Test the exact VSIX SHA-256 that will be published.
- Never overwrite an existing Marketplace version. If `1.9.1` already exists, bump `integrations/vscode/package.json` to the next patch version, rebuild from a clean checkout, install-test the new VSIX, then publish it.

## Windows preflight

Open PowerShell in the repository root:

```powershell
git fetch origin
git checkout release/stable-marketplace
git pull --ff-only origin release/stable-marketplace
powershell -ExecutionPolicy Bypass -File .\scripts\release-stable-vscode.ps1
```

The preflight must complete all of these before it produces a trusted release artifact:

1. `cargo check --workspace --all-targets`
2. `cargo test --workspace --all-targets`
3. clean `npm ci` in `integrations/vscode`
4. TypeScript compile
5. VSIX package using the lockfile-installed `vsce`
6. embedded VSIX publisher/name/version verification
7. SHA-256 + source-commit provenance manifest

Expected stable artifact for the current source version:

```text
integrations/vscode/ckb-vscode-1.9.1.vsix
integrations/vscode/ckb-vscode-1.9.1.vsix.sha256
integrations/vscode/ckb-vscode-1.9.1.vsix.release.json
```

## Manual install verification

Install the exact generated file:

```powershell
code --install-extension .\integrations\vscode\ckb-vscode-1.9.1.vsix --force
```

Then restart/reload VS Code and verify at minimum:

- CKB activates without an extension-host error.
- `CKB: Open Invisible Reality` opens.
- `CKB: Reveal My Architecture` opens/runs without a command-registration error.
- Architecture actions appear in the CKB view and command palette.
- Local/static intelligence does not claim runtime evidence when runtime telemetry is absent.
- Cloud/MCP actions fail clearly when credentials/services are unavailable rather than fabricating success.
- Existing settings remain readable and the extension can be disabled/uninstalled normally.

Do not publish if any of those checks fail.

## Marketplace version checkpoint

The source version is currently `1.9.1`. Before publication, verify the version currently shown for `TechCodinz.ckb-vscode` in the Visual Studio Marketplace.

- Marketplace below `1.9.1` -> keep `1.9.1`.
- Marketplace already `1.9.1` or higher -> do not try to replace it. Bump to the next unused patch version, commit that version update, rerun the complete preflight, and reinstall-test the new artifact.

## Publish the exact tested VSIX

After manual verification, calculate/copy the SHA from the generated checksum file and set it as the explicit install-tested approval:

```powershell
$vsix = '.\integrations\vscode\ckb-vscode-1.9.1.vsix'
$sha = (Get-FileHash -Algorithm SHA256 $vsix).Hash.ToLowerInvariant()
$env:CKB_INSTALL_TESTED_SHA256 = $sha
$env:CKB_CONFIRM_PUBLISH = 'PUBLISH-1.9.1'
$env:VSCE_PAT = '<set only in this PowerShell session>'
.\scripts\publish-stable-vscode.ps1
```

The publisher rechecks the artifact SHA, embedded extension identity/version, clean checkout, provenance manifest and source commit before invoking the locally installed `vsce` with `--packagePath`. It does not rebuild the extension during publication.

After publishing, clear the token from the shell:

```powershell
Remove-Item Env:VSCE_PAT -ErrorAction SilentlyContinue
Remove-Item Env:VSCE_TOKEN -ErrorAction SilentlyContinue
```

## Promotion

This release-tooling PR can be merged to `main` only after its scripts are reviewed. Publishing the extension is a separate explicit action and must never be triggered merely by merging this branch.
