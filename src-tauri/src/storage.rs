use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
};

pub fn app_data_dir() -> PathBuf {
    install_data_dir()
}

pub fn config_dir() -> PathBuf {
    install_data_dir().join("config")
}

pub fn database_path() -> PathBuf {
    app_data_dir().join("quicklan.sqlite3")
}

pub fn shared_store_dir() -> PathBuf {
    app_data_dir().join("shared_store")
}

pub fn profile_dir() -> PathBuf {
    app_data_dir().join("profile")
}

pub fn watch_profile_dir() -> PathBuf {
    app_data_dir().join("watch_profile")
}

pub fn avatar_dir() -> PathBuf {
    profile_dir().join("avatar")
}

pub fn current_avatar_path(extension: &str) -> PathBuf {
    avatar_dir().join(format!("current.{extension}"))
}

pub fn migrate_legacy_data() -> Result<(), String> {
    copy_dir_missing(&legacy_app_data_dir(), &app_data_dir())?;
    copy_dir_missing(&legacy_config_dir(), &config_dir())
}

pub fn migrated_app_data_path(path: &Path) -> Option<PathBuf> {
    path.strip_prefix(legacy_app_data_dir())
        .ok()
        .map(|relative| app_data_dir().join(relative))
        .filter(|path| path.exists())
}

pub fn shared_content_path(file_hash: &str) -> PathBuf {
    shared_store_dir().join(file_hash).join("content.bin")
}

pub fn shared_metadata_path(file_hash: &str) -> PathBuf {
    shared_store_dir().join(file_hash).join("metadata.json")
}

pub fn path_file_name(path: &Path) -> Result<String, String> {
    path.file_name()
        .and_then(|value| value.to_str())
        .map(|value| value.to_string())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("无法解析文件名: {}", path.display()))
}

pub async fn ensure_download_dir(dir: PathBuf) -> Result<PathBuf, String> {
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|err| format!("创建保存目录失败: {err}"))?;
    Ok(dir)
}

pub async fn unique_destination(dir: &Path, file_name: &str) -> Result<PathBuf, String> {
    let original = PathBuf::from(file_name);
    let stem = original
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("download");
    let ext = original
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    let mut attempt = 0_u32;
    loop {
        let candidate_name = if attempt == 0 {
            file_name.to_string()
        } else if ext.is_empty() {
            format!("{stem} ({attempt})")
        } else {
            format!("{stem} ({attempt}).{ext}")
        };
        let candidate = dir.join(candidate_name);
        if tokio::fs::metadata(&candidate).await.is_err() {
            return Ok(candidate);
        }
        attempt += 1;
        if attempt > 9_999 {
            return Err("无法生成唯一的保存文件名".to_string());
        }
    }
}

pub fn ensure_app_dirs() -> Result<(), String> {
    fs::create_dir_all(shared_store_dir())
        .map_err(|err| format!("创建 QuickLANData 失败: {err}"))?;
    fs::create_dir_all(profile_dir()).map_err(|err| format!("创建用户资料目录失败: {err}"))?;
    fs::create_dir_all(watch_profile_dir())
        .map_err(|err| format!("创建观影 WebView Profile 目录失败: {err}"))?;
    fs::create_dir_all(avatar_dir()).map_err(|err| format!("创建头像目录失败: {err}"))?;
    Ok(())
}

fn install_data_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("QuickLANData")
}

fn legacy_app_data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("QuickLANData")
}

fn legacy_config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("QuickLAN")
}

fn copy_dir_missing(source: &Path, target: &Path) -> Result<(), String> {
    if source == target || !source.is_dir() {
        return Ok(());
    }
    fs::create_dir_all(target)
        .map_err(|err| format!("创建数据目录失败 {}: {err}", target.display()))?;
    for entry in fs::read_dir(source)
        .map_err(|err| format!("读取旧数据目录失败 {}: {err}", source.display()))?
    {
        let entry = entry.map_err(|err| format!("读取旧数据条目失败: {err}"))?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        if source_path.is_dir() {
            copy_dir_missing(&source_path, &target_path)?;
        } else if source_path.is_file() && !target_path.exists() {
            fs::copy(&source_path, &target_path).map_err(|err| {
                format!(
                    "迁移旧数据失败 {} -> {}: {err}",
                    source_path.display(),
                    target_path.display()
                )
            })?;
        }
    }
    Ok(())
}

pub fn safe_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|ch| match ch {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            control if control.is_control() => '_',
            value => value,
        })
        .collect();
    cleaned.trim().trim_matches('.').to_string()
}

pub fn collect_files(paths: Vec<String>) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    for raw in paths {
        let path = PathBuf::from(raw);
        collect_path(&path, &mut files)?;
    }
    if files.is_empty() {
        return Err("未找到可传输的文件".to_string());
    }
    Ok(files)
}

fn collect_path(path: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    if path.is_file() {
        files.push(path.to_path_buf());
        return Ok(());
    }
    if path.is_dir() {
        for entry in
            fs::read_dir(path).map_err(|err| format!("读取目录失败 {}: {err}", path.display()))?
        {
            let entry = entry.map_err(|err| format!("读取目录条目失败: {err}"))?;
            collect_path(&entry.path(), files)?;
        }
    }
    Ok(())
}

pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("创建数据目录失败 {}: {err}", parent.display()))?;
    }
    let content =
        serde_json::to_string_pretty(value).map_err(|err| format!("序列化数据失败: {err}"))?;
    fs::write(path, content).map_err(|err| format!("写入数据失败 {}: {err}", path.display()))
}

pub fn copy_to_shared_store<T: Serialize>(
    source_path: &Path,
    file_hash: &str,
    metadata: &T,
) -> Result<(), String> {
    let content_path = shared_content_path(file_hash);
    let metadata_path = shared_metadata_path(file_hash);
    if let Some(parent) = content_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("创建共享目录失败 {}: {err}", parent.display()))?;
    }
    fs::copy(source_path, &content_path).map_err(|err| {
        format!(
            "复制共享内容失败 {} -> {}: {err}",
            source_path.display(),
            content_path.display()
        )
    })?;
    write_json(&metadata_path, metadata)
}

pub fn validate_shared_content(file_hash: &str) -> Result<PathBuf, String> {
    let path = shared_content_path(file_hash);
    if !path.is_file() {
        return Err("共享内容不存在".to_string());
    }
    Ok(path)
}
