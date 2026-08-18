$ErrorActionPreference = 'Stop'

$Root = Resolve-Path (Join-Path $PSScriptRoot '..')
$Ext = Join-Path $Root 'integrations/vscode'
Set-Location $Ext

foreach ($tool in @('node','npm','git')) {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) { throw "$tool is required." }
}

$Pkg = (& node -e "const p=require('./package.json'); process.stdout.write(JSON.stringify({name:p.name,publisher:p.publisher,version:p.version}))") | ConvertFrom-Json
if ($Pkg.name -ne 'ckb-vscode' -or $Pkg.publisher -ne 'TechCodinz') { throw 'Unexpected VS Code extension identity.' }

$Vsix = "ckb-vscode-$($Pkg.version).vsix"
$ChecksumPath = "$Vsix.sha256"
$ManifestPath = "$Vsix.release.json"
foreach ($required in @($Vsix,$ChecksumPath,$ManifestPath)) {
    if (-not (Test-Path $required)) { throw "Missing verified release file: $required. Run .\scripts\release-stable-vscode.ps1 first." }
}

$ExpectedSha = ((Get-Content $ChecksumPath -Raw).Trim().Split(' ')[0]).ToLowerInvariant()
$ActualSha = (Get-FileHash -Algorithm SHA256 $Vsix).Hash.ToLowerInvariant()
if ($ExpectedSha -ne $ActualSha) { throw "VSIX checksum mismatch: expected $ExpectedSha, got $ActualSha" }

$Manifest = Get-Content $ManifestPath -Raw | ConvertFrom-Json
$CurrentCommit = (& git -C $Root rev-parse HEAD).Trim()
$Dirty = [bool]((& git -C $Root status --porcelain) -join '')
if ($Dirty) { throw 'Refusing Marketplace publication from a dirty checkout.' }
if ($Manifest.schema -ne 'ckb-vscode-release-manifest-v1') { throw 'Unexpected release manifest schema.' }
if ($Manifest.publisher -ne $Pkg.publisher -or $Manifest.extension -ne $Pkg.name) { throw 'Release manifest identity mismatch.' }
if ($Manifest.version -ne $Pkg.version -or $Manifest.artifact -ne $Vsix) { throw 'Release manifest version/artifact mismatch.' }
if ($Manifest.sourceDirty) { throw 'Release artifact was built from a dirty worktree.' }
if ($Manifest.sourceCommit -ne $CurrentCommit) { throw "Manifest source commit $($Manifest.sourceCommit) does not match checkout $CurrentCommit." }
if ($Manifest.sha256.ToLowerInvariant() -ne $ExpectedSha) { throw 'Manifest checksum does not match checksum file.' }

Add-Type -AssemblyName System.IO.Compression.FileSystem
$Zip = [System.IO.Compression.ZipFile]::OpenRead((Resolve-Path $Vsix))
try {
    $ManifestEntry = $Zip.Entries | Where-Object { $_.FullName -eq 'extension/package.json' } | Select-Object -First 1
    if (-not $ManifestEntry) { throw 'VSIX does not contain extension/package.json.' }
    $Reader = New-Object System.IO.StreamReader($ManifestEntry.Open())
    try { $Embedded = ($Reader.ReadToEnd() | ConvertFrom-Json) } finally { $Reader.Dispose() }
    if ($Embedded.name -ne $Pkg.name -or $Embedded.publisher -ne $Pkg.publisher -or $Embedded.version -ne $Pkg.version) {
        throw 'VSIX embedded identity/version mismatch.'
    }
} finally {
    $Zip.Dispose()
}

if (($env:CKB_INSTALL_TESTED_SHA256 || '').ToLowerInvariant() -ne $ExpectedSha) {
    throw "Refusing publish: set CKB_INSTALL_TESTED_SHA256=$ExpectedSha only after manually installing/testing this exact VSIX."
}
$ExpectedConfirm = "PUBLISH-$($Pkg.version)"
if ($env:CKB_CONFIRM_PUBLISH -ne $ExpectedConfirm) {
    throw "Refusing publish: set CKB_CONFIRM_PUBLISH=$ExpectedConfirm after install verification."
}

$Pat = if ($env:VSCE_PAT) { $env:VSCE_PAT } else { $env:VSCE_TOKEN }
if (-not $Pat) { throw 'Set VSCE_PAT (preferred) or VSCE_TOKEN in this PowerShell session. Never commit it.' }

Write-Host "Publishing exact verified artifact $Vsix ($ExpectedSha) ..."
& npx --no-install vsce publish --packagePath $Vsix --pat $Pat
if ($LASTEXITCODE -ne 0) {
    throw "Marketplace publish failed with exit code $LASTEXITCODE. If this version already exists, do not overwrite it; bump package.json to the next patch version, rebuild, reinstall-test and publish that new artifact."
}

Write-Host "Published exact verified artifact: $Vsix"
Write-Host "Marketplace identity: $($Pkg.publisher).$($Pkg.name) version $($Pkg.version)"
