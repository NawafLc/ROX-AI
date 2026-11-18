//! Process listing / liveness / killing.

#[derive(Clone, Debug)]
pub struct ProcInfo {
    pub pid: u32,
    pub name: String,
}

#[cfg(windows)]
pub fn list() -> Vec<ProcInfo> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    let mut out = vec![];
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return out;
        }
        let mut e: PROCESSENTRY32W = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        if Process32FirstW(snap, &mut e) != 0 {
            loop {
                let len = e
                    .szExeFile
                    .iter()
                    .position(|c| *c == 0)
                    .unwrap_or(e.szExeFile.len());
                out.push(ProcInfo {
                    pid: e.th32ProcessID,
                    name: String::from_utf16_lossy(&e.szExeFile[..len]),
                });
                if Process32NextW(snap, &mut e) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snap);
    }
    out
}

#[cfg(not(windows))]
pub fn list() -> Vec<ProcInfo> {
    let mut out = vec![];
    if let Ok(rd) = std::fs::read_dir("/proc") {
        for e in rd.flatten() {
            if let Ok(pid) = e.file_name().to_string_lossy().parse::<u32>() {
                let name = std::fs::read_to_string(e.path().join("comm"))
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                out.push(ProcInfo { pid, name });
            }
        }
    }
    out
}

#[cfg(windows)]
pub fn pid_exists(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return false;
        }
        let mut code = 0u32;
        let ok = GetExitCodeProcess(h, &mut code) != 0;
        CloseHandle(h);
        ok && code == STILL_ACTIVE as u32
    }
}

#[cfg(not(windows))]
pub fn pid_exists(pid: u32) -> bool {
    std::path::Path::new(&format!("/proc/{pid}")).exists()
}

/// Kill a process and all its children (used to stop a runaway Claude run).
pub fn kill_tree(pid: u32) {
    #[cfg(windows)]
    {
        let _ = crate::util::hidden_command("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .output();
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("pkill")
            .args(["-TERM", "-P", &pid.to_string()])
            .output();
        let _ = std::process::Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .output();
    }
}

pub fn kill_by_name(names: &[&str]) -> usize {
    let mut n = 0;
    for p in list() {
        if names.iter().any(|x| x.eq_ignore_ascii_case(&p.name)) {
            n += 1;
            #[cfg(windows)]
            {
                let _ = crate::util::hidden_command("taskkill")
                    .args(["/F", "/T", "/PID", &p.pid.to_string()])
                    .output();
            }
            #[cfg(not(windows))]
            {
                let _ = std::process::Command::new("kill")
                    .arg(p.pid.to_string())
                    .output();
            }
        }
    }
    n
}
