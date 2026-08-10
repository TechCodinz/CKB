$ErrorActionPreference = 'Stop'

$Root = Resolve-Path (Join-Path $PSScriptRoot '..')
$Ext = Join-Path $Root 'integrations/vscode'
Set-Location $Ext

if (-not (Get-Command node -ErrorAction SilentlyContinue)) { throw 'Node.js is required.' }
if (-not (Get-Command npm -ErrorAction SilentlyContinue)) { throw 'npm is required.' }

$NodeVersion = (& node -p "process.versions.node").Trim()
$NodeMajor = [int]($NodeVersion.Split('.')[0])
if ($NodeMajor -lt 22) { throw "Node 22+ is required by current @vscode/vsce (found v$NodeVersion)." }

$Version = (& node -p "require('./package.json').version").Trim()
$Vsix = "ckb-vscode-$Version.vsix"
$Checksum = "$Vsix.sha256"

if (-not (Test-Path $Vsix) -or -not (Test-Path $Checksum)) {
    throw 'Verified release artifact missing. Run the preflight verifier first.'
}

$Expected = ((Get-Content $Checksum -Raw).Trim().Split(' ')[0]).ToLowerInvariant()
$Actual = (Get-FileHash -Algorithm SHA256 $Vsix).Hash.ToLowerInvariant()
if ($Expected -ne $Actual) { throw "VSIX checksum mismatch: expected $Expected, got $Actual" }

$Pat = if ($env:VSCE_PAT) { $env:VSCE_PAT } else { $env:VSCE_TOKEN }
if (-not $Pat) { throw 'Set VSCE_PAT (preferred) or VSCE_TOKEN in this PowerShell session. Never commit it.' }

$ExpectedConfirm = "PUBLISH-$Version"
if ($env:CKB_CONFIRM_PUBLISH -ne $ExpectedConfirm) {
    throw "Refusing to publish. After installing/testing this exact VSIX, set `$env:CKB_CONFIRM_PUBLISH='$ExpectedConfirm'."
}

& npx --yes @vscode/vsce@latest publish --packagePath $Vsix --pat $Pat
if ($LASTEXITCODE -ne 0) { throw "Marketplace publish failed with exit code $LASTEXITCODE" }
Write-Host "Published exact verified artifact: $Vsix"
