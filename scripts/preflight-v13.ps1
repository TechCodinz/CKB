$ErrorActionPreference = 'Stop'

$Root = Resolve-Path (Join-Path $PSScriptRoot '..')
Set-Location $Root

foreach ($tool in @('cargo','node','npm','git')) {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) { throw "$tool is required." }
}

$NodeMajor = [int]((& node -p "process.versions.node.split('.')[0]").Trim())
if ($NodeMajor -lt 22) { throw "Node 22+ is required by current @vscode/vsce (found $(& node -v))." }

Write-Host "`n[1/6] Rust workspace check"
& cargo check --workspace --all-targets
if ($LASTEXITCODE -ne 0) { throw 'cargo check failed' }

Write-Host "`n[2/6] Rust workspace tests"
& cargo test --workspace --all-targets
if ($LASTEXITCODE -ne 0) { throw 'cargo test failed' }

$Ext = Join-Path $Root 'integrations/vscode'
Push-Location $Ext
try {
    Write-Host "`n[3/6] VS Code install/compile"
    & npm ci
    if ($LASTEXITCODE -ne 0) { throw 'npm ci failed' }
    & npm run compile
    if ($LASTEXITCODE -ne 0) { throw 'VS Code compile failed' }

    Write-Host "`n[4/6] VSIX package + provenance manifest"
    $Version = (& node -p "require('./package.json').version").Trim()
    $Out = "ckb-vscode-$Version.vsix"
    & npx --yes @vscode/vsce@latest package --out $Out
    if ($LASTEXITCODE -ne 0) { throw 'VSIX packaging failed' }

    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [System.IO.Compression.ZipFile]::OpenRead((Resolve-Path $Out))
    try {
        if ($zip.Entries.Count -eq 0) { throw 'VSIX archive is empty' }
    } finally {
        $zip.Dispose()
    }

    $Sha256 = (Get-FileHash -Algorithm SHA256 $Out).Hash.ToLowerInvariant()
    "$Sha256  $Out" | Set-Content -NoNewline "$Out.sha256"
    $SourceCommit = (& git -C $Root rev-parse HEAD).Trim()
    $Dirty = [bool]((& git -C $Root status --porcelain) -join '')
    $Manifest = [ordered]@{
        schema = 'ckb-vscode-release-manifest-v1'
        publisher = 'TechCodinz'
        extension = 'ckb-vscode'
        version = $Version
        artifact = $Out
        sha256 = $Sha256
        sourceCommit = $SourceCommit
        sourceDirty = $Dirty
        generatedAt = (Get-Date).ToUniversalTime().ToString('o')
    }
    $Manifest | ConvertTo-Json | Set-Content "$Out.release.json"
    if ($Dirty -and $env:CKB_ALLOW_DIRTY_RELEASE -ne '1') { throw 'Refusing release artifact from a dirty worktree. Commit/stash changes and rerun.' }
} finally {
    Pop-Location
}

Write-Host "`n[5/6] Protocol/schema sanity"
Get-ChildItem (Join-Path $Root 'schemas') -Filter '*.json' | ForEach-Object {
    Get-Content $_.FullName -Raw | ConvertFrom-Json | Out-Null
    Write-Host "valid JSON: $($_.Name)"
}

Write-Host "`n[6/6] Optional JetBrains build"
$GradleBat = Join-Path $Root 'integrations/jetbrains/gradlew.bat'
if (Test-Path $GradleBat) {
    Push-Location (Split-Path $GradleBat)
    try {
        & .\gradlew.bat buildPlugin --stacktrace
        if ($LASTEXITCODE -ne 0) { throw 'JetBrains plugin build failed' }
    } finally { Pop-Location }
} else {
    Write-Host 'JetBrains Windows wrapper not present; skipped.'
}

Write-Host "`nPRE-VPS PREFLIGHT PASSED"
Write-Host "VSIX: integrations/vscode/$Out"
Write-Host "SHA256: integrations/vscode/$Out.sha256"
Write-Host "Manifest: integrations/vscode/$Out.release.json"
