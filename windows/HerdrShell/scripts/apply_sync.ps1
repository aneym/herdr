# apply_sync.ps1: mirror stage\windows\HerdrShell -> winshell\src, keeping
# node_modules / dist / target caches in place.
$src = 'C:\Users\aneym\winshell\stage\windows\HerdrShell'
$dst = 'C:\Users\aneym\winshell\src'
if (!(Test-Path $src)) { Write-Error "stage missing: $src"; exit 1 }
robocopy $src $dst /MIR /XD node_modules dist target .vite /XF *.exe Cargo.lock /NFL /NDL /NP /NJH | Out-Null
if ($LASTEXITCODE -gt 7) { exit $LASTEXITCODE }
Remove-Item -Recurse -Force 'C:\Users\aneym\winshell\stage' -ErrorAction SilentlyContinue
exit 0
