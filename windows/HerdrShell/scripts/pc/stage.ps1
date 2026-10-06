# Stage an installer for later user-controlled installation, preserving rollback.
param([Parameter(Mandatory = $true)][ValidatePattern('^[0-9a-f]{40}$')][string]$Sha)
$ErrorActionPreference = 'Stop'
$root = 'C:\Users\aneym\winshell'
$staged = Join-Path $root 'staged'
$name = "HerdrShell-setup-$Sha.exe"
$source = Join-Path (Join-Path $root 'out') $name
if (!(Test-Path -LiteralPath $source -PathType Leaf)) { throw 'built installer is missing' }
New-Item -ItemType Directory -Force -Path $staged | Out-Null
$currentPath = Join-Path $staged 'staged.json'
$previousPath = Join-Path $staged 'previous.json'
$installer = Join-Path $staged $name
$incoming = Join-Path $staged "$name.tmp"
Copy-Item -LiteralPath $source -Destination $incoming -Force
if (Test-Path -LiteralPath $currentPath) {
    $current = Get-Content -LiteralPath $currentPath -Raw | ConvertFrom-Json
    if ($current.sha -ne $Sha) {
        if (!(Test-Path -LiteralPath $current.installer -PathType Leaf)) { throw 'current installer is missing' }
        # Retain the old installer in place; snapshot its metadata before publication.
        Move-Item -LiteralPath $currentPath -Destination $previousPath -Force
    }
}
Move-Item -LiteralPath $incoming -Destination $installer -Force
$metadata = @{ sha = $Sha; installer = $installer; built_at = [DateTime]::UtcNow.ToString('o') }
$temporary = Join-Path $staged 'staged.json.tmp'
# No BOM: the app parses this with serde_json.
[IO.File]::WriteAllText($temporary, ($metadata | ConvertTo-Json -Compress), (New-Object Text.UTF8Encoding($false)))
Move-Item -LiteralPath $temporary -Destination $currentPath -Force
Write-Output "STAGED: $Sha"
