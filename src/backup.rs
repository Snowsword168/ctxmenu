use std::io;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::registry::{Hive, MenuItem, CLASSES, SCOPES};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn timestamp() -> String {
    chrono::Local::now().format("%Y%m%d_%H%M%S").to_string()
}

/// 备份目录：exe 所在目录下的 ctxmenu_backups
pub fn backup_root() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("ctxmenu_backups")
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn export_key(full_path: &str, out: &Path) -> io::Result<()> {
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let status = Command::new("reg")
        .arg("export")
        .arg(full_path)
        .arg(out)
        .arg("/y")
        .creation_flags(CREATE_NO_WINDOW)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::Other,
            format!("reg export failed: {}", full_path),
        ))
    }
}

/// 删除单项前的自动备份，返回 .reg 文件路径
pub fn backup_item(item: &MenuItem) -> io::Result<PathBuf> {
    let file = backup_root().join("deleted").join(format!(
        "{}_{}_{}.reg",
        timestamp(),
        item.hive.reg_prefix(),
        sanitize(&item.key_name)
    ));
    export_key(&item.reg_path_full(), &file)?;
    Ok(file)
}

/// 修改“打开方式”条目前备份其父键，返回 .reg 文件路径
pub fn backup_key_named(full_path: &str, hint: &str) -> io::Result<PathBuf> {
    let file = backup_root().join("deleted").join(format!(
        "{}_{}.reg",
        timestamp(),
        sanitize(hint)
    ));
    export_key(full_path, &file)?;
    Ok(file)
}

/// 全量备份所有扫描位置，返回 (目录, 导出文件数)
pub fn backup_all() -> io::Result<(PathBuf, usize)> {
    let dir = backup_root().join(format!("full_{}", timestamp()));
    std::fs::create_dir_all(&dir)?;
    let mut count = 0usize;
    for hive in [Hive::Hklm, Hive::Hkcu] {
        for (scope, _, _) in SCOPES {
            for (suffix, tag) in [(r"shell", "shell"), (r"shellex\ContextMenuHandlers", "shellex")]
            {
                let rel = format!(r"{}\{}\{}", CLASSES, scope, suffix);
                if hive.key().open_subkey(&rel).is_err() {
                    continue;
                }
                let full = format!(r"{}\{}", hive.reg_prefix(), rel);
                let file = dir.join(format!(
                    "{}_{}_{}.reg",
                    hive.reg_prefix(),
                    sanitize(scope),
                    tag
                ));
                if export_key(&full, &file).is_ok() {
                    count += 1;
                }
            }
        }
    }
    Ok((dir, count))
}
