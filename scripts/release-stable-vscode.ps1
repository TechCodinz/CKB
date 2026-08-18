$ErrorActionPreference = 'Stop'

$Root = Resolve-Path (Join-Path $PSScriptRoot '..')
Set-Location $Root

foreach ($tool in @('cargo','node','npm','git')) {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) { throw "$tool is required." }
}

$NodeVersion = (& node -p "process.versions.node").Trim()
$NodeMajor = [int]($NodeVersion.Split('.')[0])
if ($NodeMajor -lt 18) { throw "Node 18+ is required (found v$NodeVersion)." }

$DirtyBefore = [bool]((& git -C $Root status --porcelain) -join '')
if ($DirtyBefore -and $env:CKB_ALLOW_DIRTY_RELEASE -ne '1') {
    throw 'Refusing to build a release artifact from a dirty worktree. Commit/stash changes first.'
}

Write-Host "`n[1/5] Rust workspace check"
& cargo check --workspace --all-targets
if ($LASTEXITCODE -ne 0) { throw 'cargo check failed' }

Write-Host "`n[2/5] Rust workspace tests"
& cargo test --workspace --all-targets
if ($LASTEXITCODE -ne 0) { throw 'cargo test failed' }

$Ext = Join-Path $Root 'integrations/vscode'
Push-Location $Ext
try {
    Write-Host "`n[3/5] Clean VS Code dependency install + TypeScript compile"
    & npm ci
    if ($LASTEXITCODE -ne 0) { throw 'npm ci failed' }
    & npm run compile
    if ($LASTEXITCODE -ne 0) { throw 'VS Code compile failed' }

    $Identity = & node -e "const p=require('./package.json'); process.stdout.write(JSON.stringify({name:p.name,publisher:p.publisher,version:p.version}))"
    $Pkg = $Identity | ConvertFrom-Json
    if ($Pkg.name -ne 'ckb-vscode' -or $Pkg.publisher -ne 'TechCodinz') { throw 'Unexpected VS Code extension identity.' }
    if ($Pkg.version -notmatch '^\d+\.\d+\.\d+([-.][0-9A-Za-z.-]+)?$') { throw "Invalid extension version: $($Pkg.version)" }

    $Vsix = "ckb-vscode-$($Pkg.version).vsix"
    Remove-Item $Vsix,"$Vsix.sha256","$Vsix.release.json" -Force -ErrorAction SilentlyContinue

    Write-Host "`n[4/5] Package exact VSIX using lockfile-installed vsce"
    & npx --no-install vsce package --out $Vsix
    if ($LASTEXITCODE -ne 0) { throw 'VSIX packaging failed' }
    if (-not (Test-Path $Vsix)) { throw 'VSIX was not produced.' }

    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $Zip = [System.IO.Compression.ZipFile]::OpenRead((Resolve-Path $Vsix))
    try {
        if ($Zip.Entries.Count -eq 0) { throw 'VSIX archive is empty.' }
        $ManifestEntry = $Zip.Entries | Where-Object { $_.FullName -eq 'extension/package.json' } | Select-Object -First 1
        if (-not $ManifestEntry) { throw 'VSIX does not contain extension/package.json.' }
        $Reader = New-Object System.IO.StreamReader($ManifestEntry.Open())
        try { $Embedded = ($Reader.ReadToEnd() | ConvertFrom-Json) } finally { $Reader.Dispose() }
        if ($Embedded.name -ne $Pkg.name -or $Embedded.publisher -ne $Pkg.publisher -or $Embedded.version -ne $Pkg.version) {
            throw 'VSIX embedded manifest identity/version does not match source package.json.'
        }
    } finally {
        $Zip.Dispose()
    }

    $Sha256 = (Get-FileHash -Algorithm SHA256 $Vsix).Hash.ToLowerInvariant()
    "$Sha256  $Vsix" | Set-Content -NoNewline "$Vsix.sha256"
    $SourceCommit = (& git -C $Root rev-parse HEAD).Trim()
    $SourceBranch = (& git -C $Root branch --show-current).Trim()
    $DirtyAfter = [bool]((& git -C $Root status --porcelain) -join '')
    if ($DirtyAfter -and $env:CKB_ALLOW_DIRTY_RELEASE -ne '1') { throw 'Worktree became dirty during release preflight.' }

    $Manifest = [ordered]@{
        schema = 'ckb-vscode-release-manifest-v1'
        publisher = $Pkg.publisher
        extension = $Pkg.name
        version = $Pkg.version
        artifact = $Vsix
        sha256 = $Sha256
        sourceCommit = $SourceCommit
        sourceBranch = $SourceBranch
        sourceDirty = $DirtyAfter
        nodeVersion = "v$NodeVersion"
        vsceVersion = (& npx --no-install vsce --version).Trim()
        generatedAt = (Get-Date).ToUniversalTime().ToString('o')
    }
    $Manifest | ConvertTo-Json | Set-Content "$Vsix.release.json"

    Write-Host "`n[5/5] Release artifact verified"
    Write-Host "VSIX: $Ext\$Vsix"
    Write-Host "SHA256: $Sha256"
    Write-Host "Manifest: $Ext\$Vsix.release.json"
    Write-Host "`nMANUAL INSTALL TEST COMMAND:"
    Write-Host "code --install-extension `"$Ext\$Vsix`" --force"
    Write-Host "`nAfter testing the exact artifact, publish with:"
    Write-Host "`$env:CKB_INSTALL_TESTED_SHA256='$Sha256'"
    Write-Host "`$env:CKB_CONFIRM_PUBLISH='PUBLISH-$($Pkg.version)'"
    Write-Host ".\scripts\publish-stable-vscode.ps1"
} finally {
    Pop-Location
}
