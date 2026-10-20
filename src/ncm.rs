use crate::settings::Settings;
use std::{os::windows::process::CommandExt, process::Command};

fn ps_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}
fn powershell(script: &str) -> Result<String, String> {
    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            script,
        ])
        .creation_flags(0x08000000)
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err("网易云连接操作失败，请检查路径、端口或进程状态".into());
    }
    String::from_utf8(output.stdout)
        .map(|s| s.trim().trim_start_matches('\u{feff}').to_string())
        .map_err(|e| e.to_string())
}
pub fn detect() -> Result<String, String> {
    let mut paths: Vec<String> = processes().into_iter().map(|(_, path)| path).collect();
    paths.sort_by_key(|p| p.to_lowercase());
    paths.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    if paths.len() != 1 {
        return Err("请手动填写网易云路径，或先打开网易云再查找".into());
    }
    let value = paths.remove(0);
    let settings = Settings {
        ncm_path: value.clone(),
        ..Settings::default()
    };
    Settings::parse(&settings.json())?;
    Ok(value)
}
// Native snapshots avoid starting PowerShell merely to check whether NCM is running.
pub fn processes() -> Vec<(u32, String)> {
    use windows_sys::Win32::{
        Foundation::*,
        System::{Diagnostics::ToolHelp::*, Threading::*},
    };
    let mut result = Vec::new();
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return result;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of_val(&entry) as u32;
        let mut available = Process32FirstW(snapshot, &mut entry);
        let mut candidates = Vec::new();
        while available != 0 {
            let end = entry
                .szExeFile
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(entry.szExeFile.len());
            if String::from_utf16_lossy(&entry.szExeFile[..end])
                .eq_ignore_ascii_case("cloudmusic.exe")
            {
                candidates.push((entry.th32ProcessID, entry.th32ParentProcessID));
            }
            available = Process32NextW(snapshot, &mut entry);
        }
        CloseHandle(snapshot);
        for (pid, parent) in &candidates {
            if candidates.iter().any(|(id, _)| id == parent) {
                continue;
            }
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, *pid);
            if process.is_null() {
                continue;
            }
            let mut path = vec![0u16; 32768];
            let mut length = path.len() as u32;
            if QueryFullProcessImageNameW(process, 0, path.as_mut_ptr(), &mut length) != 0 {
                result.push((*pid, String::from_utf16_lossy(&path[..length as usize])));
            }
            CloseHandle(process);
        }
    }
    result.sort_by_key(|(pid, _)| *pid);
    result
}
pub fn connect(settings: &Settings, only_if_needed: bool) -> Result<(), String> {
    if !std::path::Path::new(&settings.ncm_path).is_file() {
        return Err("请先填写有效的 cloudmusic.exe 路径".into());
    }
    powershell(&connect_script(settings, only_if_needed))?;
    Ok(())
}
fn connect_script(settings: &Settings, only_if_needed: bool) -> String {
    format!(
        r#"$ErrorActionPreference='Stop'
$exe={path}
$all=@(Get-CimInstance Win32_Process -Filter "Name = 'cloudmusic.exe'")
$mains=@($all | Where-Object {{$_.ExecutablePath -eq $exe -and $_.CommandLine -notmatch '(?i)(?:^|\s)--type(?:=|\s)'}})
if($mains.Count -gt 1){{exit 1}}
$main=$mains | Select-Object -First 1
if({only} -and -not $main){{exit 0}}
if({only} -and $main -and $main.CommandLine -match '(?i)(?:^|\s)--remote-debugging-port(?:=|\s+){port}(?:\s|$)'){{exit 0}}
$listeners=@(Get-NetTCPConnection -LocalPort {port} -State Listen -ErrorAction SilentlyContinue)
if(@($listeners | Where-Object {{-not $main -or $_.OwningProcess -ne $main.ProcessId}}).Count){{exit 1}}
$arguments=''
if($main){{
 $arguments=$main.CommandLine -replace '^\s*(?:"[^"]+"|\S+)\s*',''
 $arguments=$arguments -replace '(?i)(?:^|\s)--remote-debugging-port(?:=|\s+)\d+',''
 $arguments=$arguments -replace '(?i)(?:^|\s)--remote-debugging-address(?:=|\s+)\S+',''
 $ids=New-Object 'System.Collections.Generic.HashSet[uint32]';[void]$ids.Add([uint32]$main.ProcessId)
 do{{$changed=$false;foreach($p in $all){{if($p.ExecutablePath -eq $exe -and $ids.Contains([uint32]$p.ParentProcessId)){{if($ids.Add([uint32]$p.ProcessId)){{$changed=$true}}}}}}}}while($changed)
 $current=Get-CimInstance Win32_Process -Filter "ProcessId = $($main.ProcessId)"
 if($current.ExecutablePath -eq $exe){{Stop-Process -Id $main.ProcessId}}
 # Include renderers respawned between snapshot and main-process exit.
 $all=@(Get-CimInstance Win32_Process -Filter "Name = 'cloudmusic.exe'")
 do{{$changed=$false;foreach($p in $all){{if($p.ExecutablePath -eq $exe -and $ids.Contains([uint32]$p.ParentProcessId)){{if($ids.Add([uint32]$p.ProcessId)){{$changed=$true}}}}}}}}while($changed)
 foreach($id in $ids){{$p=Get-CimInstance Win32_Process -Filter "ProcessId = $id";if($p -and $p.ExecutablePath -eq $exe){{Stop-Process -Id $id -ErrorAction SilentlyContinue}}}}
 for($i=0;$i -lt 30;$i++){{$remaining=@(Get-CimInstance Win32_Process -Filter "Name = 'cloudmusic.exe'" | Where-Object {{$_.ExecutablePath -eq $exe -and $ids.Contains([uint32]$_.ProcessId)}});if(-not $remaining.Count){{break}};Start-Sleep -Milliseconds 100}}
 if($remaining.Count){{exit 1}}
}}
$arguments=($arguments.Trim()+' --remote-debugging-port={port} --remote-debugging-address=127.0.0.1').Trim()
Start-Process -FilePath $exe -ArgumentList $arguments -WorkingDirectory ([IO.Path]::GetDirectoryName($exe)) -WindowStyle Hidden
"#,
        path = ps_quote(&settings.ncm_path),
        port = settings.debug_port,
        only = if only_if_needed { "$true" } else { "$false" }
    )
}
#[cfg(test)]
mod tests {
    #[test]
    fn connection_script_parses_without_execution() {
        use std::io::Write;
        use std::os::windows::process::CommandExt;
        use std::process::Stdio;
        let settings = crate::Settings {
            ncm_path: "C:\\Cloud Music\\O'Brien\\cloudmusic.exe".into(),
            debug_port: 9333,
            ..Default::default()
        };
        let script = super::connect_script(&settings, false);
        assert!(script.contains("--remote-debugging-port=9333"));
        assert!(script.contains("O''Brien"));
        let watched = super::connect_script(&settings, true);
        assert!(
            watched.contains("if($true -and -not $main){exit 0}"),
            "Monitoring must not reopen a process that exited after its snapshot"
        );
        assert!(watched.contains("if($true -and $main -and $main.CommandLine"));
        let mut child=std::process::Command::new("powershell.exe").args(["-NoProfile","-NonInteractive","-Command","$s=[Console]::In.ReadToEnd();$t=$null;$e=$null;[System.Management.Automation.Language.Parser]::ParseInput($s,[ref]$t,[ref]$e)|Out-Null;if($e.Count){exit 1}"]).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).creation_flags(0x08000000).spawn().unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(script.as_bytes())
            .unwrap();
        assert!(child.wait().unwrap().success());
    }
    #[test]
    fn quoting() {
        assert_eq!(
            super::ps_quote("C:\\O'Brien\\cloudmusic.exe"),
            "'C:\\O''Brien\\cloudmusic.exe'"
        );
    }
}
