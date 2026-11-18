#Requires -Version 7.0
[CmdletBinding()]
param([Parameter(Mandatory)][string]$DeploymentScript)

$ErrorActionPreference = 'Stop'
$tokens = $null
$parseErrors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile(
    (Resolve-Path -LiteralPath $DeploymentScript).ProviderPath, [ref]$tokens, [ref]$parseErrors)
if ($parseErrors.Count -gt 0) { throw "Deployment script has parse errors: $parseErrors" }
$definition = $ast.FindAll({
    param($node)
    $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Install-ProductBinary'
}, $false)
if ($definition.Count -ne 1) { throw 'Expected one binary replacement function' }
# Load only the file replacement function; installation and registration stay outside this test.
. ([scriptblock]::Create($definition[0].Extent.Text))

$testRoot = Join-Path ([IO.Path]::GetTempPath()) ("ram-deployment-test-" + [guid]::NewGuid())
$null = New-Item -ItemType Directory -Path $testRoot
$source = Join-Path $testRoot 'source.bin'
$destination = Join-Path $testRoot 'installed.bin'
$script:corruptOnRead = $null

function Get-FileHash {
    param([string]$LiteralPath)
    if ($LiteralPath -eq $script:corruptOnRead) {
        $script:corruptOnRead = $null
        [IO.File]::WriteAllText($LiteralPath, 'corrupted replacement')
    }
    Microsoft.PowerShell.Utility\Get-FileHash -LiteralPath $LiteralPath
}

function Assert-Content([string]$Path, [string]$Expected) {
    if ([IO.File]::ReadAllText($Path) -ne $Expected) { throw "Content mismatch: $Path" }
}

function Assert-CleanReplacement {
    if ((Test-Path -LiteralPath "$destination.pending") -or
        (Test-Path -LiteralPath "$destination.previous")) {
        throw 'Replacement left a temporary binary'
    }
}

function Assert-Failure([scriptblock]$Operation) {
    $failed = $false
    try { & $Operation } catch { $failed = $true }
    if (-not $failed) { throw 'Expected replacement failure' }
}

try {
    [IO.File]::WriteAllText($source, 'release one')
    Install-ProductBinary $source $destination
    Assert-Content $destination 'release one'
    Assert-CleanReplacement

    [IO.File]::WriteAllText($source, 'release two')
    Install-ProductBinary $source $destination
    Assert-Content $destination 'release two'
    Assert-CleanReplacement

    $handle = [IO.File]::Open($destination, 'Open', 'Read', 'Read')
    try { Assert-Failure { Install-ProductBinary $source $destination } } finally { $handle.Dispose() }
    Assert-Content $destination 'release two'
    Assert-CleanReplacement

    [IO.File]::WriteAllText($source, 'release three')
    $script:corruptOnRead = $destination
    Assert-Failure { Install-ProductBinary $source $destination }
    Assert-Content $destination 'release two'
    Assert-CleanReplacement

    [IO.File]::WriteAllText("$destination.pending", 'retained interrupted update')
    Assert-Failure { Install-ProductBinary $source $destination }
    Assert-Content $destination 'release two'
    Assert-Content "$destination.pending" 'retained interrupted update'

    Remove-Item -LiteralPath "$destination.pending", $destination
    $script:corruptOnRead = $destination
    Assert-Failure { Install-ProductBinary $source $destination }
    if (Test-Path -LiteralPath $destination) { throw 'Failed first install left a corrupt binary' }
    Assert-CleanReplacement

    Write-Output 'Deployment tests passed: first install, replacement, locked target, hash-failure rollback, interrupted-update preservation, failed first install cleanup.'
} finally {
    Remove-Item -LiteralPath $testRoot -Recurse -Force
}
