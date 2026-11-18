//! 用户级数据目录：所有操作系统统一使用 `~/.starfruit`（HOME / USERPROFILE）。

use std::path::PathBuf;

/// 用户主目录（跨平台：Unix 用 HOME，Windows 用 USERPROFILE）。
pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// 用户级数据根目录 `~/.starfruit`。
pub fn user_data_root() -> PathBuf {
    home_dir().join(".starfruit")
}

/// 主节点默认数据目录 `~/.starfruit/master`。
pub fn default_master_data_dir() -> PathBuf {
    user_data_root().join("master")
}

/// 独立 Cloudflare Tunnel 的公网地址文件（由 scripts/starfruit-tunnel.sh 维护）。
pub fn tunnel_url_file() -> PathBuf {
    user_data_root().join("tunnel").join("url")
}

/// 读取独立 Tunnel 的公网地址（不存在时返回 None；主节点不管理 Tunnel 生命周期）。
pub fn read_tunnel_url() -> Option<String> {
    std::fs::read_to_string(tunnel_url_file())
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 默认数据目录位于用户级_starfruit() {
        let p = default_master_data_dir();
        assert!(p.ends_with(".starfruit/master"));
    }
}