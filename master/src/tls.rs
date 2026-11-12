//! TLS 支持：加载用户证书或首次启动生成自签证书（LAN 场景）。

use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use rustls::pki_types::{CertificateDer, PrivateKeyDer};

/// 构建 TLS acceptor：优先用户提供的 cert/key；缺省时在 data-dir 生成/复用自签证书。
pub fn build_acceptor(
    cert_path: Option<&Path>,
    key_path: Option<&Path>,
    data_dir: &Path,
) -> Result<tokio_rustls::TlsAcceptor, String> {
    // ring 后端，构建产物小、无需 cmake。
    let _ = rustls::crypto::ring::default_provider().install_default();

    let (cert_pem, key_pem) = match (cert_path, key_path) {
        (Some(c), Some(k)) => (
            std::fs::read_to_string(c).map_err(|e| format!("读取证书失败 {}: {e}", c.display()))?,
            std::fs::read_to_string(k).map_err(|e| format!("读取私钥失败 {}: {e}", k.display()))?,
        ),
        (None, None) => load_or_generate_self_signed(data_dir)?,
        _ => return Err("--cert 与 --key 必须同时提供".into()),
    };

    let certs: Vec<CertificateDer> = rustls_pemfile::certs(&mut cert_pem.as_bytes())
        .collect::<Result<_, _>>()
        .map_err(|e| format!("解析证书失败: {e}"))?;
    let key: PrivateKeyDer = rustls_pemfile::private_key(&mut key_pem.as_bytes())
        .map_err(|e| format!("解析私钥失败: {e}"))?
        .ok_or_else(|| "私钥为空".to_string())?;

    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| format!("构建 TLS 配置失败: {e}"))?;
    Ok(tokio_rustls::TlsAcceptor::from(Arc::new(config)))
}

/// 自签证书：data-dir 已有 `starfruit-cert.pem`/`starfruit-key.pem` 则复用（指纹稳定），
/// 否则用 rcgen 生成。
fn load_or_generate_self_signed(data_dir: &Path) -> Result<(String, String), String> {
    let cert_file = data_dir.join("starfruit-cert.pem");
    let key_file = data_dir.join("starfruit-key.pem");
    if cert_file.exists() && key_file.exists() {
        let cert = std::fs::read_to_string(&cert_file).map_err(|e| e.to_string())?;
        let key = std::fs::read_to_string(&key_file).map_err(|e| e.to_string())?;
        eprintln!("[tls] 复用自签证书 {}", cert_file.display());
        return Ok((cert, key));
    }

    let CertifiedKey { cert, key_pair } = rcgen::generate_simple_self_signed(vec![
        "localhost".into(),
        "starfruit".into(),
    ])
    .map_err(|e| format!("生成自签证书失败: {e}"))?;
    let cert_pem = cert.pem();
    let key_pem = key_pair.serialize_pem();

    std::fs::create_dir_all(data_dir).map_err(|e| e.to_string())?;
    write_private(&cert_file, cert_pem.as_bytes())?;
    write_private(&key_file, key_pem.as_bytes())?;
    eprintln!(
        "[tls] 已生成自签证书 {}（子节点默认信任；如需 CA 校验请使用 --cert/--key）",
        cert_file.display()
    );
    Ok((cert_pem, key_pem))
}

/// 写入私钥类文件：权限收紧到 0600。
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut f = std::fs::File::create(path).map_err(|e| e.to_string())?;
    f.write_all(bytes).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

use rcgen::CertifiedKey;
