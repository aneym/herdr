function Stop-OrphanWebView {
    $live = @{}
    Get-CimInstance Win32_Process | ForEach-Object { $live[[int]$_.ProcessId] = $true }
    $orphans = @(Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" | Where-Object {
        $_.CommandLine -like '*com.aneyman.herdr-shell*' -and -not $live.ContainsKey([int]$_.ParentProcessId)
    })
    foreach ($o in $orphans) { & taskkill.exe /T /F /PID $o.ProcessId 2>&1 | Out-Null }
    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    while ([DateTime]::UtcNow -lt $deadline -and @($orphans | Where-Object { Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue }).Count) {
        Start-Sleep -Milliseconds 100
    }
    $orphans.Count
}
