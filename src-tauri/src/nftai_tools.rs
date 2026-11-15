//! Local installation discovery, deliberately independent of remote update checks.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Desktop {
    name: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    path: PathBuf,
    #[serde(default)]
    app_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct Installation {
    name: String,
    version: Option<String>,
    error: Option<String>,
    installed_but_broken: bool,
    desktop_name: Option<String>,
    desktop_version: Option<String>,
    desktop_configurable: bool,
}

fn status(tool: &str, desktop: Option<&Desktop>, cli: Result<Option<String>, String>) -> Installation {
    let (version, error) = match cli { Ok(v) => (v, None), Err(e) => (None, Some(e)) };
    Installation {
        name: tool.into(), installed_but_broken: error.is_some(), version, error,
        desktop_name: desktop.map(|d| d.name.clone()),
        desktop_version: desktop.and_then(|d| d.version.clone()),
        // Toolbox Claude targets Desktop exclusively; a CLI installation is insufficient.
        desktop_configurable: desktop.is_some() && matches!(tool, "dsh" | "codex" | "claude" | "opencode"),
    }
}

fn dsh_desktop(root: &Path) -> Option<Desktop> {
    let path = root.join("Programs/DeepSeek Harness/DeepSeek Harness.exe");
    path.is_file().then(|| Desktop { name: "DeepSeek Harness 桌面版".into(), version: None, path, app_id: None })
}

fn opencode_desktop(roots: &[PathBuf]) -> Option<Desktop> {
    roots.iter().flat_map(|root| [
        root.join("OpenCode/OpenCode.exe"),
        root.join("OpenCode/opencode.exe"),
        root.join("Programs/OpenCode/OpenCode.exe"),
        root.join("Programs/OpenCode/opencode.exe"),
        root.join("ai.opencode.desktop/OpenCode.exe"),
        root.join("ai.opencode.desktop/opencode.exe"),
    ]).find(|p| p.is_file()).map(|path| Desktop {
        name: "OpenCode 桌面版".into(), version: None, path, app_id: None,
    })
}

#[cfg(windows)]
fn desktops() -> std::collections::HashMap<String, Desktop> {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    let mut result = std::collections::HashMap::new();
    if let Some(root) = std::env::var_os("LOCALAPPDATA") {
        if let Some(d) = dsh_desktop(Path::new(&root)) { result.insert("dsh".into(), d); }
    }
    let mut roots = Vec::new();
    if let Some(root) = std::env::var_os("LOCALAPPDATA") { roots.push(PathBuf::from(root)); }
    if let Some(root) = std::env::var_os("ProgramFiles") { roots.push(PathBuf::from(root)); }
    if let Some(d) = opencode_desktop(&roots) { result.insert("opencode".into(), d); }
    // Query installed package metadata once. Never run desktop binaries with --version.
    // Launch IDs come from each installed package's own manifest, not guessed App IDs.
    let script = r#"$ErrorActionPreference='Stop'; $items=@(Get-AppxPackage | Where-Object { $_.Name -in @('OpenAI.Codex','Claude') } | ForEach-Object { $p=$_; $m=Get-AppxPackageManifest -Package $p.PackageFullName; $a=@($m.Package.Applications.Application)[0]; if($a) { @{name=$p.Name; version=$p.Version.ToString(); path=$p.InstallLocation; app_id=($p.PackageFamilyName+'!'+$a.Id)} } }); ConvertTo-Json -InputObject $items -Compress"#;
    let ps = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
        .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let child = Command::new(ps).args(["-NoProfile", "-NonInteractive", "-Command", script])
        .creation_flags(0x08000000).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn();
    if let Ok(mut child) = child {
        let started = std::time::Instant::now();
        while matches!(child.try_wait(), Ok(None)) {
            if started.elapsed() > std::time::Duration::from_secs(10) { let _ = child.kill(); break; }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        if let Ok(output) = child.wait_with_output() {
            if let Ok(packages) = serde_json::from_slice::<Vec<Desktop>>(&output.stdout) {
                for mut d in packages {
                    let tool = if d.name == "OpenAI.Codex" { "codex" } else { "claude" };
                    d.name = if tool == "codex" { "Codex 桌面版" } else { "Claude 桌面版" }.into();
                    result.insert(tool.into(), d);
                }
            }
        }
    }
    result
}

#[cfg(not(windows))]
fn desktops() -> std::collections::HashMap<String, Desktop> {
    let mut result = std::collections::HashMap::new();
    #[cfg(target_os = "macos")]
    for (tool, label) in [("dsh", "DeepSeek Harness"), ("codex", "Codex"), ("claude", "Claude"), ("opencode", "OpenCode")] {
        let roots = [PathBuf::from("/Applications"), crate::config::get_home_dir().join("Applications")];
        if let Some(path) = roots.iter().map(|r| r.join(format!("{label}.app"))).find(|p| p.is_dir()) {
            result.insert(tool.into(), Desktop { name: format!("{label} 桌面版"), version: None, path, app_id: None });
        }
    }
    result
}

fn detect() -> Vec<Installation> {
    let installed = desktops();
    ["dsh", "claude", "codex", "opencode"].iter().map(|tool| {
        let desktop = installed.get(*tool);
        let cli = Ok(None);
        status(tool, desktop, cli)
    }).collect()
}

#[tauri::command]
pub(crate) async fn nftai_detect_tools() -> Result<Vec<Installation>, String> {
    tauri::async_runtime::spawn_blocking(detect).await.map_err(|e| format!("本机检测失败：{e}"))
}

pub(crate) fn launch(tool: &str) -> Result<(), String> {
    if !matches!(tool, "dsh" | "codex" | "claude" | "opencode") { return Err("不支持的工具".into()); }
    if matches!(tool, "dsh" | "codex" | "claude" | "opencode") {
        if let Some(d) = desktops().remove(tool) {
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                let mut cmd = if let Some(id) = d.app_id {
                    let mut c = std::process::Command::new(std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows")).join("explorer.exe"));
                    c.arg(format!("shell:AppsFolder\\{id}")); c
                } else { std::process::Command::new(d.path) };
                cmd.creation_flags(0x08000000).spawn().map_err(|e| format!("启动桌面版失败：{e}"))?;
                return Ok(());
            }
            #[cfg(target_os = "macos")]
            { std::process::Command::new("/usr/bin/open").arg(d.path).spawn().map_err(|e| format!("启动桌面版失败：{e}"))?; return Ok(()); }
        }
    }
    Err("未检测到桌面版，请先完成安装并重新检测".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn desktop_products_are_not_mistaken_for_missing_cli_tools() {
        let d = Desktop { name: "desktop".into(), version: Some("1.0".into()), path: PathBuf::new(), app_id: None };
        assert!(status("dsh", Some(&d), Ok(None)).desktop_configurable);
        assert!(status("codex", Some(&d), Ok(None)).desktop_configurable);
        let claude = status("claude", Some(&d), Ok(None));
        assert!(claude.desktop_configurable);
        assert!(claude.version.is_none());
        assert!(claude.desktop_name.is_some());
        assert!(status("claude", None, Err("broken".into())).installed_but_broken);
    }
    #[test]
    fn opencode_desktop_requires_gui_at_install_location() {
        let temp = tempfile::tempdir().unwrap();
        assert!(opencode_desktop(&[temp.path().into()]).is_none());
        let gui = temp.path().join("Programs/OpenCode");
        std::fs::create_dir_all(&gui).unwrap();
        std::fs::write(gui.join("OpenCode.exe"), "fixture").unwrap();
        assert!(opencode_desktop(&[temp.path().into()]).is_some());
    }
    #[test]
    fn dsh_requires_an_actual_desktop_executable() {
        let temp = tempfile::tempdir().unwrap();
        assert!(dsh_desktop(temp.path()).is_none());
        let dir = temp.path().join("Programs/DeepSeek Harness");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("DeepSeek Harness.exe"), "fixture").unwrap();
        assert!(dsh_desktop(temp.path()).is_some());
    }
    #[test]
    #[ignore = "manual local installation diagnostic"]
    fn inspect_this_computer() { println!("{}", serde_json::to_string(&detect()).unwrap()); }
}
