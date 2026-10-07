use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Deserialize)]
struct Manifest {
    sha: String,
    installer: PathBuf,
    built_at: String,
}

#[derive(Serialize)]
pub struct Staged {
    sha: String,
    built_at: String,
}

#[derive(Serialize)]
pub struct Previous {
    sha: String,
}

#[derive(Serialize)]
pub struct UpdateStatus {
    current: String,
    staged: Option<Staged>,
    available: bool,
    previous: Option<Previous>,
}

fn winshell_dir() -> Result<PathBuf, String> {
    std::env::var_os("USERPROFILE")
        .filter(|value| !value.is_empty())
        .map(|profile| PathBuf::from(profile).join("winshell"))
        .ok_or_else(|| "USERPROFILE is not set".to_string())
}

fn manifest(name: &str) -> Result<Manifest, String> {
    let path = winshell_dir()?.join("staged").join(name);
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    // PowerShell 5.1 writes UTF-8 with a BOM by default; tolerate one.
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
    serde_json::from_slice(bytes).map_err(|e| format!("{}: {e}", path.display()))
}

fn same_sha(current: &str, staged: &str) -> bool {
    let current = current.strip_suffix("+dirty").unwrap_or(current);
    !current.is_empty() && staged.starts_with(current)
}

#[tauri::command]
pub fn update_status() -> UpdateStatus {
    let current = env!("HERDR_SHELL_COMMIT").to_string();
    let staged = manifest("staged.json").ok();
    let available = staged.as_ref().is_some_and(|build| {
        build.installer.is_absolute()
            && build.installer.is_file()
            && !same_sha(&current, &build.sha)
    });
    UpdateStatus {
        current,
        staged: staged.map(|build| Staged {
            sha: build.sha,
            built_at: build.built_at,
        }),
        available,
        previous: manifest("previous.json")
            .ok()
            .filter(|build| build.installer.is_absolute() && build.installer.is_file())
            .map(|build| Previous { sha: build.sha }),
    }
}

#[tauri::command]
pub fn update_apply(app: tauri::AppHandle) -> Result<(), String> {
    install(app, "staged.json")
}

#[tauri::command]
pub fn update_rollback(app: tauri::AppHandle) -> Result<(), String> {
    install(app, "previous.json")
}

fn install(app: tauri::AppHandle, name: &str) -> Result<(), String> {
    let result = install_inner(app, name);
    if let Err(error) = &result {
        use std::io::Write;
        let logged = (|| -> Result<(), String> {
            let logs = winshell_dir()?.join("logs");
            std::fs::create_dir_all(&logs)
                .map_err(|e| format!("Create update log directory: {e}"))?;
            let mut log = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(logs.join("update.log"))
                .map_err(|e| format!("Open update log: {e}"))?;
            writeln!(
                log,
                "{:?} {name} failed: {error}",
                std::time::SystemTime::now()
            )
            .map_err(|e| format!("Write update log: {e}"))
        })();
        if let Err(log_error) = logged {
            return Err(format!("{error}; {log_error}"));
        }
    }
    result
}

#[cfg(not(windows))]
fn install_inner(_app: tauri::AppHandle, _name: &str) -> Result<(), String> {
    Err("Shell updates are only supported on Windows".into())
}

#[cfg(windows)]
fn install_inner(app: tauri::AppHandle, name: &str) -> Result<(), String> {
    use std::io::Write;
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    use windows_sys::Win32::System::Threading::{
        CREATE_BREAKAWAY_FROM_JOB, CREATE_NO_WINDOW, DETACHED_PROCESS,
    };

    let build = manifest(name)?;
    if !build.installer.is_absolute() || !build.installer.is_file() {
        return Err(format!(
            "Installer is missing: {}",
            build.installer.display()
        ));
    }
    let local = std::env::var_os("LOCALAPPDATA")
        .filter(|value| !value.is_empty())
        .ok_or("LOCALAPPDATA is not set")?;
    let exe = PathBuf::from(local)
        .join("Herdr Shell")
        .join("HerdrShell.exe");
    let logs = winshell_dir()?.join("logs");
    std::fs::create_dir_all(&logs).map_err(|e| format!("Create update log directory: {e}"))?;
    let quote = |path: &std::path::Path| -> Result<String, String> {
        let text = path.to_str().ok_or("Update path is not valid Unicode")?;
        Ok(format!("'{}'", text.replace('\'', "''")))
    };
    let script = format!(
        r#"$ErrorActionPreference = 'Stop'
$log = {log}
try {{
    $parent = Get-Process -Id {pid} -ErrorAction SilentlyContinue
    if ($parent -and -not $parent.WaitForExit(20000)) {{
        throw 'Herdr Shell did not exit within 20 seconds'
    }}
    $installer = Start-Process -FilePath {installer} -ArgumentList '/S' -PassThru -Wait
    Add-Content -LiteralPath $log -Value "$(Get-Date -Format o) installer exit code: $($installer.ExitCode)"
    Start-Process -FilePath {exe}
}} catch {{
    Add-Content -LiteralPath $log -Value "$(Get-Date -Format o) update failed: $_"
}} finally {{
    Remove-Item -LiteralPath $PSCommandPath -ErrorAction SilentlyContinue
}}
"#,
        log = quote(&logs.join("update.log"))?,
        pid = std::process::id(),
        installer = quote(&build.installer)?,
        exe = quote(&exe)?,
    );
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "herdr-shell-update-{}-{stamp}.ps1",
        std::process::id()
    ));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| format!("Create update script: {e}"))?;
    // Windows PowerShell 5.1 needs a BOM to interpret non-ASCII user paths as UTF-8.
    if let Err(error) = file
        .write_all(b"\xef\xbb\xbf")
        .and_then(|_| file.write_all(script.as_bytes()))
    {
        drop(file);
        let _ = std::fs::remove_file(&path);
        return Err(format!("Write update script: {error}"));
    }
    drop(file);
    // A shell launched by a scheduled task lives in that task's job, which ends
    // a child helper together with the app. A task of its own outlives both.
    let register = r#"$ErrorActionPreference = 'Stop'
$arguments = '-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File "' + $env:HERDR_SHELL_UPDATE_SCRIPT + '"'
$action = New-ScheduledTaskAction -Execute 'powershell.exe' -Argument $arguments
$principal = New-ScheduledTaskPrincipal -UserId $env:USERNAME -LogonType Interactive -RunLevel Limited
$settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit ([TimeSpan]::Zero) -MultipleInstances Parallel
Register-ScheduledTask -TaskName HerdrShellUpdate -Action $action -Principal $principal -Settings $settings -Force | Out-Null
Start-ScheduledTask -TaskName HerdrShellUpdate"#;
    let scheduled = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            register,
        ])
        .env("HERDR_SHELL_UPDATE_SCRIPT", &path)
        .stdin(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|error| error.to_string())
        .and_then(|output| {
            if output.status.success() {
                Ok(())
            } else {
                Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
            }
        });
    let spawned = scheduled.or_else(|task_error| {
        let mut command = Command::new("powershell.exe");
        command
            .args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-WindowStyle",
                "Hidden",
                "-File",
            ])
            .arg(&path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS | CREATE_BREAKAWAY_FROM_JOB);
        command
            .spawn()
            .or_else(|breakaway_error| {
                command
                    .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
                    .spawn()
                    .map_err(|error| format!("breakaway: {breakaway_error}; fallback: {error}"))
            })
            .map(|_| ())
            .map_err(|error| format!("scheduled task: {task_error}; {error}"))
    });
    if let Err(error) = spawned {
        let _ = std::fs::remove_file(&path);
        return Err(format!("Start update installer handoff: {error}"));
    }
    app.exit(0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::same_sha;

    // Pure prefix algorithm: guards short/dirty build identity without launching an installer.
    #[test]
    fn build_identity_compares_full_staged_sha_to_current_prefix() {
        for (current, staged, equal) in [
            ("abcdef123456", "abcdef123456", true),
            ("abcdef1", "abcdef123456", true),
            ("abcdef1+dirty", "abcdef123456", true),
            ("abcdef123456+dirty", "abcdef123456", true),
            ("abcdef123456", "abcdef1", false),
            ("abcdef123456", "abcdef199999", false),
            ("fedcba1", "abcdef123456", false),
            ("", "abcdef123456", false),
            ("+dirty", "abcdef123456", false),
        ] {
            assert_eq!(same_sha(current, staged), equal, "{current} vs {staged}");
        }
    }
}
