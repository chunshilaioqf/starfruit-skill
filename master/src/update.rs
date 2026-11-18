//! 更新检查：每周最多一次，比对线上 CHANGELOG.md 的最新版本与当前版本。
//!
//! 仓库固定为 <https://github.com/chunshilaioqf/starfruit-skill>；
//! 检查逻辑 = 拉取 raw CHANGELOG.md，取第一个 `## [x.y.z]` 版本号与本地比较。
//! 节流记录落盘 `<data-dir>/update-check.json`（失败不推进时间戳，下次启动重试）。

use std::path::Path;

use serde_json::Value;

/// 项目仓库地址（Web 面板 / 文档 / 更新检查统一引用）。
pub const REPO_URL: &str = "https://github.com/chunshilaioqf/starfruit-skill";

/// 线上 CHANGELOG（更新检查唯一依据）。
pub const CHANGELOG_URL: &str =
    "https://raw.githubusercontent.com/chunshilaioqf/starfruit-skill/main/CHANGELOG.md";

/// 当前二进制版本（编译期注入）。
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// 检查间隔：一周。
pub const CHECK_INTERVAL_SECS: u64 = 7 * 86400;

const CHECK_FILE: &str = "update-check.json";

/// 从 CHANGELOG 文本解析最新版本号（第一个 `## [x.y.z]`，跳过 Unreleased）。
pub fn parse_latest_version(changelog: &str) -> Option<String> {
    for line in changelog.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("## [") {
            if let Some(ver) = rest.split(']').next() {
                if ver.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                    return Some(ver.to_string());
                }
            }
        }
    }
    None
}

/// 语义化版本比较：latest 是否严格新于 current（按点分数字逐段比较）。
pub fn version_newer(latest: &str, current: &str) -> bool {
    let parse = |s: &str| -> Vec<u64> {
        s.split('.')
            .map(|p| p.trim().parse::<u64>().unwrap_or(0))
            .collect()
    };
    let (a, b) = (parse(latest), parse(current));
    for i in 0..a.len().max(b.len()) {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        if x != y {
            return x > y;
        }
    }
    false
}

/// 组装检查结果 JSON（面板 / 会话提醒共用）。
pub fn result_json(latest: Option<&str>) -> Value {
    let update_available = latest.is_some_and(|l| version_newer(l, CURRENT_VERSION));
    serde_json::json!({
        "current_version": CURRENT_VERSION,
        "latest_version": latest,
        "update_available": update_available,
        "repo_url": REPO_URL,
    })
}

/// 每周节流检查（阻塞调用，放在 spawn_blocking / 线程中执行）。
/// 返回检查结果；网络失败且无缓存时返回 None。
pub fn check_weekly(data_dir: &Path) -> Option<Value> {
    let path = data_dir.join(CHECK_FILE);
    let cached: Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(Value::Null);
    let now = crate::protocol::now_ms();
    let last = cached["ts_ms"].as_u64().unwrap_or(0);
    let cached_latest = cached["latest"].as_str().map(str::to_string);
    if now.saturating_sub(last) < CHECK_INTERVAL_SECS * 1000 && cached_latest.is_some() {
        return Some(result_json(cached_latest.as_deref()));
    }
    match fetch_latest() {
        Ok(latest) => {
            let _ = std::fs::write(
                &path,
                serde_json::json!({ "ts_ms": now, "latest": latest }).to_string(),
            );
            Some(result_json(Some(&latest)))
        }
        Err(e) => {
            eprintln!("[update] 检查更新失败（不影响运行）: {e}");
            cached_latest.as_deref().map(|l| result_json(Some(l)))
        }
    }
}

/// 拉取线上 CHANGELOG 并解析最新版本。
pub fn fetch_latest() -> Result<String, String> {
    let body = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .get(CHANGELOG_URL)
        .call()
        .map_err(|e| format!("GET {CHANGELOG_URL}: {e}"))?
        .into_string()
        .map_err(|e| format!("读取 CHANGELOG 失败: {e}"))?;
    parse_latest_version(&body).ok_or_else(|| "CHANGELOG 中未找到版本号".to_string())
}

/// 数据迁移检查：对比数据目录版本标记与当前版本。
/// 当前各版本数据结构兼容（JSONL 追加式），无需迁移；保留检查点供后续版本扩展。
/// 返回旧版本标记（None 表示全新数据目录）。
pub fn check_migration(data_dir: &Path) -> Option<String> {
    let marker = data_dir.join("data-version");
    let old = std::fs::read_to_string(&marker).ok().map(|s| s.trim().to_string());
    match &old {
        Some(v) if v != CURRENT_VERSION => {
            eprintln!(
                "[migrate] 数据目录版本 {v} → {CURRENT_VERSION}：已检查，当前版本间无数据结构变更，直接沿用。"
            );
        }
        None => {
            eprintln!("[migrate] 全新数据目录，版本标记 {CURRENT_VERSION}。");
        }
        _ => {}
    }
    let _ = std::fs::write(&marker, format!("{CURRENT_VERSION}\n"));
    old.filter(|v| v != CURRENT_VERSION)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 解析_changelog_最新版本_跳过_unreleased() {
        let text = "# 更新日志\n\n## [Unreleased]\n\n## [0.10.0] - 2026-11-18\n### 新增\n\n## [0.9.0] - 2026-11-15\n";
        assert_eq!(parse_latest_version(text).as_deref(), Some("0.10.0"));
        assert_eq!(parse_latest_version("没有任何版本"), None);
    }

    #[test]
    fn 版本比较_逐段数字() {
        assert!(version_newer("0.10.0", "0.9.0"));
        assert!(version_newer("1.0.0", "0.99.99"));
        assert!(!version_newer("0.9.0", "0.9.0"));
        assert!(!version_newer("0.8.0", "0.9.0"));
        assert!(version_newer("0.9.1", "0.9.0"));
    }

    #[test]
    fn 检查结果_json_形状() {
        let v = result_json(Some("99.0.0"));
        assert_eq!(v["update_available"], true);
        assert_eq!(v["repo_url"], REPO_URL);
        let v = result_json(None);
        assert_eq!(v["update_available"], false);
    }
}