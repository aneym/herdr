# apply_sync.ps1: mirror stage\windows\HerdrShell -> winshell\src, keeping
# node_modules / dist / target caches in place. Shared shell lands beside winshell
# so app/src imports of ../../../../shell resolve without a full repo checkout.
$src = 'C:\Users\aneym\winshell\stage\windows\HerdrShell'
$dst = 'C:\Users\aneym\winshell\src'
$sharedSrc = 'C:\Users\aneym\winshell\stage\shell'
$sharedDst = 'C:\Users\aneym\shell'
if (!(Test-Path $src)) { Write-Error "stage missing: $src"; exit 1 }
if (!(Test-Path $sharedSrc)) { Write-Error "stage missing: $sharedSrc"; exit 1 }
robocopy $src $dst /MIR /XD node_modules dist target .vite /XF *.exe Cargo.lock /NFL /NDL /NP /NJH | Out-Null
if ($LASTEXITCODE -gt 7) { exit $LASTEXITCODE }
robocopy $sharedSrc $sharedDst /MIR /NFL /NDL /NP /NJH | Out-Null
if ($LASTEXITCODE -gt 7) { exit $LASTEXITCODE }
Remove-Item -Recurse -Force 'C:\Users\aneym\winshell\stage' -ErrorAction SilentlyContinue
exit 0
