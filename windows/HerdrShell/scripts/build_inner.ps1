# build_inner.ps1 -Sha <sha>: npm ci (if lock changed) + tauri icon + tauri build.
# Runs inside a job under build.ps1; writes its exit code to cache\build.rc.
param([string]$Sha = 'dev')
$ErrorActionPreference = 'Continue'
$rcFile = 'C:\Users\aneym\winshell\cache\build.rc'
$appDir = 'C:\Users\aneym\winshell\src\app'

Set-Location $appDir
$hashFile = 'C:\Users\aneym\winshell\cache\lock.sha256'
$h = (Get-FileHash (Join-Path $appDir 'package-lock.json') -Algorithm SHA256).Hash
$need = $true
if ((Test-Path $hashFile) -and ((Get-Content $hashFile -Raw).Trim() -eq $h) -and (Test-Path (Join-Path $appDir 'node_modules'))) {
    $need = $false
}
if ($need) {
    Write-Output "npm ci..."
    cmd /c 'npm ci 2>&1' | ForEach-Object { "$_" }
    if ($LASTEXITCODE -ne 0) { Set-Content $rcFile $LASTEXITCODE; exit $LASTEXITCODE }
    Set-Content $hashFile $h
} else {
    Write-Output "npm ci skipped (lock unchanged)"
}

Set-Location (Join-Path $appDir 'src-tauri')
cmd /c 'npx tauri icon icons\source.svg -o icons 2>&1' | ForEach-Object { "$_" }

Set-Location $appDir
Write-Output "tauri build (commit $Sha)..."
cmd /c 'npx tauri build --bundles nsis 2>&1' | ForEach-Object { "$_" }
$rc = $LASTEXITCODE
Set-Content $rcFile $rc
exit $rc
