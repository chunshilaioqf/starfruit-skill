//! JWT（HS256）令牌：**仅主节点签发**，执行节点连接时必须携带（公网模式强制要求）。
//!
//! - 算法固定 HS256（HMAC-SHA256），签名密钥由主节点持有：`--jwt-secret` 指定，
//!   或首次启动时在 `data-dir/jwt-secret` 自动生成（0600 权限）。
//! - 载荷：`{"iss":"starfruit-master","sub":"<节点名>","iat":..,"exp":..}`；
//!   校验时要求签名正确、未过期，且 `sub` 与注册名一致（令牌不可跨节点借用）。
//! - 签发方式：主节点 CLI `--issue-token <节点名>`（打印令牌后退出）。

use std::path::Path;

use base64::Engine;
use ring::hmac;

/// 令牌默认有效期（秒）：24 小时。
pub const DEFAULT_TTL_SECS: u64 = 86400;

/// 密钥文件名（位于 data-dir）。
const SECRET_FILE: &str = "jwt-secret";

const B64: base64::engine::general_purpose::GeneralPurpose =
    base64::engine::general_purpose::URL_SAFE_NO_PAD;

const HEADER_JSON: &str = r#"{"alg":"HS256","typ":"JWT"}"#;

/// 令牌签发/校验器。
pub struct TokenAuth {
    key: hmac::Key,
    ttl: u64,
}

impl TokenAuth {
    /// 载入或创建密钥：优先 `--jwt-secret`，否则读/写 `data-dir/jwt-secret`。
    pub fn load_or_create(
        data_dir: &Path,
        secret: Option<&str>,
        ttl: u64,
    ) -> Result<Self, String> {
        let secret_bytes = match secret.map(str::trim).filter(|s| !s.is_empty()) {
            Some(s) => s.as_bytes().to_vec(),
            None => load_or_create_secret_file(data_dir)?,
        };
        if secret_bytes.len() < 16 {
            return Err("jwt 密钥过短（至少 16 字节），请换用更长的 --jwt-secret".into());
        }
        Ok(Self {
            key: hmac::Key::new(hmac::HMAC_SHA256, &secret_bytes),
            ttl: ttl.max(60),
        })
    }

    /// 令牌默认有效期（秒）。
    pub fn ttl(&self) -> u64 {
        self.ttl
    }

    /// 为指定节点名签发令牌。
    pub fn issue(&self, sub: &str) -> String {
        self.issue_with_ttl(sub, self.ttl)
    }

    /// 为指定节点名签发指定有效期的令牌。
    pub fn issue_with_ttl(&self, sub: &str, ttl: u64) -> String {
        let now = now_secs();
        let claims = serde_json::json!({
            "iss": "starfruit-master",
            "sub": sub,
            "iat": now,
            "exp": now + ttl.max(60),
        })
        .to_string();
        let signing_input = format!("{}.{}", B64.encode(HEADER_JSON), B64.encode(&claims));
        let sig = hmac::sign(&self.key, signing_input.as_bytes());
        format!("{signing_input}.{}", B64.encode(sig.as_ref()))
    }

    /// 校验令牌：签名 / 有效期 / 归属（`sub` 必须等于注册名）。
    pub fn verify(&self, token: &str, expect_sub: &str) -> Result<u64, String> {
        let parts: Vec<&str> = token.trim().split('.').collect();
        if parts.len() != 3 {
            return Err("令牌格式非法（应为 header.payload.signature）".into());
        }
        let signing_input = format!("{}.{}", parts[0], parts[1]);
        let sig = B64
            .decode(parts[2])
            .map_err(|_| "令牌签名段不是合法 base64url".to_string())?;
        hmac::verify(&self.key, signing_input.as_bytes(), &sig)
            .map_err(|_| "令牌签名校验失败（可能不是本主节点签发）".to_string())?;

        let payload = B64
            .decode(parts[1])
            .map_err(|_| "令牌载荷段不是合法 base64url".to_string())?;
        let claims: serde_json::Value =
            serde_json::from_slice(&payload).map_err(|_| "令牌载荷不是合法 JSON".to_string())?;

        let sub = claims.get("sub").and_then(|v| v.as_str()).unwrap_or("");
        if sub != expect_sub {
            return Err(format!("令牌归属 {sub} 与注册名 {expect_sub} 不一致"));
        }
        let exp = claims.get("exp").and_then(|v| v.as_u64()).unwrap_or(0);
        let now = now_secs();
        if exp <= now {
            return Err(format!("令牌已过期（exp={exp}，now={now}），请重新签发"));
        }
        Ok(exp)
    }
}

/// 读取 `data-dir/jwt-secret`；不存在则生成 32 字节随机密钥并落盘（0600）。
fn load_or_create_secret_file(data_dir: &Path) -> Result<Vec<u8>, String> {
    let path = data_dir.join(SECRET_FILE);
    if let Ok(text) = std::fs::read_to_string(&path) {
        let hex = text.trim();
        if !hex.is_empty() {
            return hex_decode(hex).ok_or_else(|| format!("{} 内容不是合法 hex", path.display()));
        }
    }
    let mut raw = [0u8; 32];
    ring::rand::SecureRandom::fill(
        &ring::rand::SystemRandom::new(),
        &mut raw,
    )
    .map_err(|_| "生成 jwt 随机密钥失败".to_string())?;
    std::fs::create_dir_all(data_dir).map_err(|e| format!("创建 data-dir 失败: {e}"))?;
    let hex = hex_encode(&raw);
    std::fs::write(&path, format!("{hex}\n")).map_err(|e| format!("写入 {} 失败: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    eprintln!("[jwt] 已生成令牌签名密钥: {}", path.display());
    Ok(raw.to_vec())
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

/// 当前 UNIX 秒。
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 取监听地址中的端口（无端口时返回 None）。
pub fn port_of(addr: &str) -> Option<u16> {
    addr.rsplit_once(':')
        .and_then(|(_, p)| p.trim_end_matches(']').parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auth() -> TokenAuth {
        TokenAuth::load_or_create(Path::new("."), Some("0123456789abcdef0123456789abcdef"), 3600)
            .expect("构造校验器失败")
    }

    #[test]
    fn 令牌签发与校验_归属一致() {
        let a = auth();
        let t = a.issue("worker-a");
        assert!(a.verify(&t, "worker-a").is_ok());
    }

    #[test]
    fn 令牌不可跨节点借用() {
        let a = auth();
        let t = a.issue("worker-a");
        assert!(a.verify(&t, "worker-b").is_err());
    }

    #[test]
    fn 令牌过期应被拒绝() {
        let a = auth();
        let now = now_secs();
        let claims = serde_json::json!({
            "iss": "starfruit-master", "sub": "worker-a", "iat": now - 100, "exp": now - 1,
        })
        .to_string();
        let signing_input = format!("{}.{}", B64.encode(HEADER_JSON), B64.encode(&claims));
        let sig = hmac::sign(&a.key, signing_input.as_bytes());
        let token = format!("{signing_input}.{}", B64.encode(sig.as_ref()));
        assert!(a.verify(&token, "worker-a").is_err());
    }

    #[test]
    fn 他人密钥签发的令牌应被拒绝() {
        let a = auth();
        let other = TokenAuth::load_or_create(Path::new("."), Some("ffffffffffffffffffffffffffffffff"), 3600)
            .expect("构造校验器失败");
        let t = other.issue("worker-a");
        assert!(a.verify(&t, "worker-a").is_err());
    }

    #[test]
    fn 端口解析() {
        assert_eq!(port_of("127.0.0.1:10001"), Some(10001));
        assert_eq!(port_of("[::1]:10002"), Some(10002));
        assert_eq!(port_of("127.0.0.1"), None);
    }
}
