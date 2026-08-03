use directories::ProjectDirs;
use std::path::PathBuf;

/// Separate from desktop Netvan (`com.Netvan.Netvan`) so both can coexist.
pub fn project_dirs() -> Option<ProjectDirs> {
    ProjectDirs::from("com", "Netvan", "NetvanApi")
}

pub fn data_dir() -> PathBuf {
    // Prefer ProgramData-style path when available; directories crate uses Roaming AppData on Windows.
    // Override with NETVAN_API_DATA_DIR for service installs.
    if let Ok(custom) = std::env::var("NETVAN_API_DATA_DIR") {
        return PathBuf::from(custom);
    }
    let program_data = std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
    let dir = program_data.join("Netvan").join("NetvanApi");
    if dir.exists() || std::fs::create_dir_all(&dir).is_ok() {
        return dir;
    }
    project_dirs()
        .map(|p| p.data_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".").join("netvan-api-data"))
}

pub fn db_path() -> PathBuf {
    data_dir().join("netvan-web.db")
}

pub fn ensure_data_dir() -> std::io::Result<PathBuf> {
    let dir = data_dir();
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub const DEFAULT_BIND: &str = "127.0.0.1:8787";
