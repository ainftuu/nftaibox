//! Download only the official Node.js LTS installer; verify its published SHA-256
//! before opening the system installer. No remote shell scripts are evaluated.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};
use tauri::{Emitter, Manager};

fn quiet(command: &mut Command) {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    #[cfg(not(target_os = "windows"))]
    let _ = command;
}
fn node_candidates() -> Vec<PathBuf> {
    let mut values = vec![PathBuf::from("node")];
    #[cfg(target_os = "windows")]
    if let Some(programs) = std::env::var_os("ProgramFiles") {
        values.push(PathBuf::from(programs).join("nodejs/node.exe"));
    }
    #[cfg(target_os = "macos")]
    values.extend([
        PathBuf::from("/usr/local/bin/node"),
        PathBuf::from("/opt/homebrew/bin/node"),
    ]);
    values
}
fn working_node() -> Option<PathBuf> {
    node_candidates().into_iter().find(|path| {
        let mut command = Command::new(path);
        command.arg("--version");
        quiet(&mut command);
        command
            .output()
            .ok()
            .filter(|o| o.status.success())
            .is_some_and(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .trim()
                    .trim_start_matches('v')
                    .split('.')
                    .next()
                    .and_then(|s| s.parse::<u32>().ok())
                    .is_some_and(|major| major >= 22)
            })
    })
}
fn expose_runtime(path: &Path) {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        let mut paths = vec![parent.to_path_buf()];
        if let Some(value) = std::env::var_os("PATH") {
            paths.extend(std::env::split_paths(&value));
        }
        if let Ok(joined) = std::env::join_paths(paths) {
            std::env::set_var("PATH", joined);
        }
    }
}
fn digest_matches(content: &[u8], sums: &str, file: &str) -> bool {
    let expected = sums.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        let sum = fields.next()?;
        let name = fields.next()?.trim_start_matches('*');
        (name == file && sum.len() == 64).then_some(sum)
    });
    expected.is_some_and(|sum| format!("{:x}", Sha256::digest(content)) == sum)
}
#[tauri::command]
pub async fn nftai_prepare_runtime(app: tauri::AppHandle, tool: String) -> Result<(), String> {
    if !matches!(tool.as_str(), "dsh" | "claude" | "codex" | "opencode") {
        return Err("不支持的工具".into());
    }
    if let Some(path) = tokio::task::spawn_blocking(working_node)
        .await
        .map_err(|_| "检查运行环境失败")?
    {
        expose_runtime(&path);
        return Ok(());
    }
    let progress = |line: &str| {
        let _ = app.emit(
            "tool-lifecycle-progress",
            json!({"tools":[tool],"stage":"output","line":line}),
        );
    };
    progress("正在准备 Node.js LTS 运行环境（从 nodejs.org 下载）…");
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "初始化下载失败")?;
    let releases: Value = http
        .get("https://nodejs.org/dist/index.json")
        .send()
        .await
        .map_err(|_| "无法连接 Node.js 官方下载站")?
        .error_for_status()
        .map_err(|_| "Node.js 下载站返回错误")?
        .json()
        .await
        .map_err(|_| "Node.js 版本列表格式异常")?;
    let version = releases
        .as_array()
        .and_then(|list| list.iter().find(|v| v["lts"].as_str().is_some()))
        .and_then(|v| v["version"].as_str())
        .ok_or("未找到 Node.js LTS")?;
    if !version.starts_with('v') || !version[1..].chars().all(|c| c.is_ascii_digit() || c == '.') {
        return Err("Node.js 版本号异常".into());
    }
    #[cfg(target_os = "windows")]
    let filename = format!(
        "node-{version}-{}.msi",
        if cfg!(target_arch = "aarch64") {
            "arm64"
        } else {
            "x64"
        }
    );
    #[cfg(target_os = "macos")]
    let filename = format!("node-{version}.pkg");
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let filename = String::new();
    if filename.is_empty() {
        return Err("当前系统请手动安装 Node.js LTS".into());
    }
    let base = format!("https://nodejs.org/dist/{version}");
    let sums = http
        .get(format!("{base}/SHASUMS256.txt"))
        .send()
        .await
        .map_err(|_| "下载校验清单失败")?
        .error_for_status()
        .map_err(|_| "下载校验清单失败")?
        .text()
        .await
        .map_err(|_| "读取校验清单失败")?;
    let response = http
        .get(format!("{base}/{filename}"))
        .send()
        .await
        .map_err(|_| "下载 Node.js 安装包失败")?
        .error_for_status()
        .map_err(|_| "下载 Node.js 安装包失败")?;
    const MAX: u64 = 150 * 1024 * 1024;
    if response.content_length().is_some_and(|n| n > MAX) {
        return Err("安装包大小异常".into());
    }
    let mut response = response;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| "安装包下载中断")? {
        if bytes.len() as u64 + chunk.len() as u64 > MAX {
            return Err("安装包大小异常".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    if !digest_matches(&bytes, &sums, &filename) {
        return Err("安装包校验失败，已停止安装，请重试".into());
    }
    let dir = app
        .path()
        .app_cache_dir()
        .map_err(|_| "无法定位下载目录")?
        .join("runtime");
    std::fs::create_dir_all(&dir).map_err(|_| "无法创建下载目录")?;
    let path = dir.join(filename);
    std::fs::write(&path, bytes).map_err(|_| "无法保存安装包")?;
    progress("下载与校验完成。请在系统安装窗口完成 Node.js 安装，工具箱会继续配置。");
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        #[cfg(target_os = "windows")]
        {
            let mut command = Command::new("msiexec");
            command
                .arg("/i")
                .arg(&path)
                .args(["/passive", "/norestart"]);
            quiet(&mut command);
            let status = command.status().map_err(|_| "无法启动 Node.js 安装程序")?;
            if !matches!(status.code(), Some(0) | Some(3010)) {
                return Err("Node.js 安装未完成，请允许系统安装后重试".into());
            }
        }
        #[cfg(target_os = "macos")]
        if !Command::new("open")
            .args(["-W"])
            .arg(&path)
            .status()
            .map_err(|_| "无法打开 Node.js 安装程序")?
            .success()
        {
            return Err("Node.js 安装未完成，请重试".into());
        }
        let node = working_node().ok_or("尚未检测到 Node.js LTS，请完成安装并重新打开工具箱")?;
        expose_runtime(&node);
        Ok(())
    })
    .await
    .map_err(|_| "运行环境安装任务异常")??;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_corrupt_or_wrong_filename_installer() {
        let checksum = format!("{:x}  node.msi", Sha256::digest(b"official"));
        assert!(digest_matches(b"official", &checksum, "node.msi"));
        assert!(!digest_matches(b"tampered", &checksum, "node.msi"));
        assert!(!digest_matches(b"official", &checksum, "other.msi"));
        assert!(!digest_matches(b"official", "", "node.msi"));
    }
}
