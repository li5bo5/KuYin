use std::collections::BTreeMap;
use std::io::Read;

use aes::{Aes128, Aes256};
use base64::prelude::*;
use cbc::cipher::{block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use flate2::read::ZlibDecoder;
use md5::{Digest, Md5};
use num_bigint::BigUint;
use rsa::pkcs8::DecodePublicKey;
use rsa::traits::PublicKeyParts;
use rsa::RsaPublicKey;
use sha1::Sha1;
use uuid::Uuid;

use super::config::{random_alnum_lower, LITE_APPID, LITE_CLIENTVER};

pub const ANDROID_SALT: &str = "LnT6xpN3khm36zse0QzvmgTZ3waWdRSA";
pub const WEB_SALT: &str = "NVPh5oo715z5DIWAeQlhMDsWXXQV4hwt";
pub const KRC_XOR_KEY: [u8; 16] = [
    64, 71, 97, 119, 94, 50, 116, 71, 81, 54, 49, 45, 206, 210, 110, 105,
];

/// 酷狗概念版 (Lite) 官方 RSA 1024-bit 公钥（严格遵循 RFC 7468 每行 64 字符 PEM 换行）
pub const PUBLIC_LITE_RSA_KEY: &str = "-----BEGIN PUBLIC KEY-----\n\
MIGfMA0GCSqGSIb3DQEBAQUAA4GNADCBiQKBgQDECi0Np2UR87scwrvTr72L6oO0\n\
1rBbbBPriSDFPxr3Z5syug0O24QyQO8bg27+0+4kBzTBTBOZ/WWU0WryL1JSXRTX\n\
LgFVxtzIY41Pe7lPOgsfTCn5kZcvKhYKJesKnnJDNr5/abvTGf+rHG3YRwsCHcQ0\n\
8/q6ifSioBszvb3QiwIDAQAB\n\
-----END PUBLIC KEY-----";

type Aes128CbcEnc = cbc::Encryptor<Aes128>;
type Aes128CbcDec = cbc::Decryptor<Aes128>;
type Aes256CbcEnc = cbc::Encryptor<Aes256>;
type Aes256CbcDec = cbc::Decryptor<Aes256>;

/// 字节数组转小写十六进制字符串
pub fn bytes_to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

/// 十六进制字符串转字节数组
pub fn hex_to_bytes(hex: &str) -> Result<Vec<u8>, String> {
    let clean = hex.trim();
    if !clean.len().is_multiple_of(2) {
        return Err("十六进制字符串长度必须为偶数".to_string());
    }
    let mut out = Vec::with_capacity(clean.len() / 2);
    for i in (0..clean.len()).step_by(2) {
        let byte = u8::from_str_radix(&clean[i..i + 2], 16)
            .map_err(|e| format!("十六进制解析失败: {e}"))?;
        out.push(byte);
    }
    Ok(out)
}

/// MD5 小写十六进制哈希
pub fn md5_hex<T: AsRef<[u8]>>(data: T) -> String {
    let mut hasher = Md5::new();
    hasher.update(data);
    bytes_to_hex(&hasher.finalize())
}

/// SHA1 小写十六进制哈希（用于微信开放平台扫码 ticket 签名）
pub fn sha1_hex<T: AsRef<[u8]>>(data: T) -> String {
    let mut hasher = Sha1::new();
    hasher.update(data);
    bytes_to_hex(&hasher.finalize())
}

/// 1. 酷狗概念版主力签名算法：`signatureAndroidParams`
///    参数键名升序拼接 `k=v`（无连接符）：`MD5(ANDROID_SALT + sorted + body_str + ANDROID_SALT)`
pub fn signature_android_params(params: &BTreeMap<String, String>, body_str: Option<&str>) -> String {
    let mut sorted_str = String::new();
    for (k, v) in params {
        sorted_str.push_str(k);
        sorted_str.push('=');
        sorted_str.push_str(v);
    }
    let body = body_str.unwrap_or("");
    let payload = format!("{ANDROID_SALT}{sorted_str}{body}{ANDROID_SALT}");
    md5_hex(payload.as_bytes())
}

/// 2. 酷狗 Web / H5 扫码签名算法：`signatureWebParams`
pub fn signature_web_params(params: &BTreeMap<String, String>, body_str: Option<&str>) -> String {
    let mut sorted_str = String::new();
    for (k, v) in params {
        sorted_str.push_str(k);
        sorted_str.push('=');
        sorted_str.push_str(v);
    }
    let body = body_str.unwrap_or("");
    let payload = format!("{WEB_SALT}{sorted_str}{body}{WEB_SALT}");
    md5_hex(payload.as_bytes())
}

/// 3. 注册类接口专用签名：`signatureRegisterParams`
pub fn signature_register_params(params: &BTreeMap<String, String>) -> String {
    let mut values: Vec<String> = params.values().cloned().collect();
    values.sort();
    let sorted_values = values.concat();
    let payload = format!("1014{sorted_values}1014");
    md5_hex(payload.as_bytes())
}

/// 4. 音源换链密钥签名：`signKey` = `MD5(hash + salt + appid + mid + userid)`
pub fn sign_key(hash: &str, mid: &str, userid: &str, appid: &str) -> String {
    let uid = if userid.is_empty() { "0" } else { userid };
    let salt = "185672dd44712f60bb1736df5a377e82";
    let payload = format!("{hash}{salt}{appid}{mid}{uid}");
    md5_hex(payload.as_bytes())
}

/// 参数密钥签名：`signParamsKey` = `MD5(appid + salt + clientver + data)`
pub fn sign_params_key(data: &str) -> String {
    let payload = format!("{LITE_APPID}{ANDROID_SALT}{LITE_CLIENTVER}{data}");
    md5_hex(payload.as_bytes())
}

/// 通用 AES-256-CBC 加密（酷狗 `cryptoAesEncrypt`：32 字节 ASCII key + 16 字节 ASCII iv -> Hex）
pub fn aes256_cbc_encrypt_hex(plain: &[u8], key32: &[u8], iv16: &[u8]) -> Result<String, String> {
    let enc = Aes256CbcEnc::new_from_slices(key32, iv16)
        .map_err(|e| format!("初始化 AES-256-CBC 加密器失败: {e:?}"))?;
    let mut buf = vec![0u8; plain.len() + 16];
    let ct = enc
        .encrypt_padded_b2b_mut::<Pkcs7>(plain, &mut buf)
        .map_err(|e| format!("AES-256-CBC 填充加密失败: {e:?}"))?;
    Ok(bytes_to_hex(ct))
}

/// 对齐 `cryptoAesEncrypt(data)`：自动生成 16 位随机 `temp_key`，返回 `(cipher_hex, temp_key)`
pub fn crypto_aes_encrypt_auto(plain: &str) -> Result<(String, String), String> {
    let temp_key = random_alnum_lower(16);
    let key_md5 = md5_hex(temp_key.as_bytes()); // 32 字节 ASCII
    let iv = &key_md5[16..32]; // 后 16 字节 ASCII
    let cipher_hex = aes256_cbc_encrypt_hex(plain.as_bytes(), key_md5.as_bytes(), iv.as_bytes())?;
    Ok((cipher_hex, temp_key))
}

/// 对齐 `cryptoAesDecrypt(hex_data, temp_key)`：用于解密登录返回的 `secu_params`
pub fn crypto_aes_decrypt(cipher_hex: &str, temp_key: &str) -> Result<String, String> {
    let key_md5 = md5_hex(temp_key.as_bytes());
    let iv = &key_md5[16..32];
    let mut ct_buf = hex_to_bytes(cipher_hex)?;
    let dec = Aes256CbcDec::new_from_slices(key_md5.as_bytes(), iv.as_bytes())
        .map_err(|e| format!("初始化 AES-256-CBC 解密器失败: {e:?}"))?;
    let pt = dec
        .decrypt_padded_mut::<Pkcs7>(&mut ct_buf)
        .map_err(|e| format!("AES-256-CBC 解密失败: {e:?}"))?;
    String::from_utf8(pt.to_vec()).map_err(|e| format!("UTF-8 解析失败: {e}"))
}

/// 对齐 `playlistAesEncrypt(data)`：6 位随机 key，AES-128-CBC 加密并输出 `(Base64, key)`（用于 `register_dev`）
pub fn playlist_aes_encrypt(plain: &str) -> Result<(String, String), String> {
    let key6 = random_alnum_lower(6);
    let md5_str = md5_hex(key6.as_bytes());
    let enc_key = &md5_str[0..16];
    let iv = &md5_str[16..32];

    let enc = Aes128CbcEnc::new_from_slices(enc_key.as_bytes(), iv.as_bytes())
        .map_err(|e| format!("初始化 AES-128-CBC 失败: {e:?}"))?;
    let mut buf = vec![0u8; plain.len() + 16];
    let ct = enc
        .encrypt_padded_b2b_mut::<Pkcs7>(plain.as_bytes(), &mut buf)
        .map_err(|e| format!("AES-128-CBC 加密失败: {e:?}"))?;
    Ok((BASE64_STANDARD.encode(ct), key6))
}

/// 对齐 `playlistAesDecrypt(raw_bytes, key6)`：用于解密 `register_dev` 返回的二进制密文
pub fn playlist_aes_decrypt_bytes(cipher_bytes: &[u8], key6: &str) -> Result<String, String> {
    let md5_str = md5_hex(key6.as_bytes());
    let dec_key = &md5_str[0..16];
    let iv = &md5_str[16..32];

    let mut buf = cipher_bytes.to_vec();
    let dec = Aes128CbcDec::new_from_slices(dec_key.as_bytes(), iv.as_bytes())
        .map_err(|e| format!("初始化 AES-128-CBC 解密器失败: {e:?}"))?;
    let pt = dec
        .decrypt_padded_mut::<Pkcs7>(&mut buf)
        .map_err(|e| format!("解密设备注册响应失败: {e:?}"))?;
    String::from_utf8(pt.to_vec()).map_err(|e| format!("UTF-8 转换失败: {e}"))
}

fn parse_lite_rsa_components() -> Result<(BigUint, BigUint, usize), String> {
    let pub_key = RsaPublicKey::from_public_key_pem(PUBLIC_LITE_RSA_KEY)
        .map_err(|e| format!("解析概念版 RSA 公钥失败: {e:?}"))?;
    let n_bytes = pub_key.n().to_bytes_be();
    let e_bytes = pub_key.e().to_bytes_be();
    let key_len = pub_key.size();
    Ok((
        BigUint::from_bytes_be(&n_bytes),
        BigUint::from_bytes_be(&e_bytes),
        key_len,
    ))
}

/// 对齐 `cryptoRSAEncrypt`：右侧零字节填充 (Zero-padding) Raw RSA 加密，输出 256 字符小写 Hex
pub fn crypto_rsa_encrypt_lite(plain: &str) -> Result<String, String> {
    let (n, e, key_len) = parse_lite_rsa_components()?;
    let raw = plain.as_bytes();
    if raw.len() > key_len {
        return Err("RSA 明文长度超出密钥块上限".to_string());
    }
    let mut padded = vec![0u8; key_len];
    padded[..raw.len()].copy_from_slice(raw);

    let m = BigUint::from_bytes_be(&padded);
    let c = m.modpow(&e, &n);
    let mut c_bytes = c.to_bytes_be();
    if c_bytes.len() < key_len {
        let mut left_padded = vec![0u8; key_len - c_bytes.len()];
        left_padded.append(&mut c_bytes);
        c_bytes = left_padded;
    }
    Ok(bytes_to_hex(&c_bytes))
}

/// 对齐 `rsaEncrypt2`：标准 RFC 8017 `RSAES-PKCS1-v1_5` 加密，输出小写 Hex
pub fn rsa_encrypt_pkcs1_v15_lite(plain: &str) -> Result<String, String> {
    let (n, e, key_len) = parse_lite_rsa_components()?;
    let raw = plain.as_bytes();
    if raw.len() + 11 > key_len {
        return Err("PKCS1v15 明文过长".to_string());
    }

    let ps_len = key_len - raw.len() - 3;
    let mut em = Vec::with_capacity(key_len);
    em.push(0x00);
    em.push(0x02);

    // 使用操作系统 CSPRNG (UUID v4 随机源) 生成非零填充字节 PS
    let mut ps = Vec::with_capacity(ps_len);
    while ps.len() < ps_len {
        for b in Uuid::new_v4().into_bytes() {
            if b != 0 && ps.len() < ps_len {
                ps.push(b);
            }
        }
    }
    em.extend_from_slice(&ps);
    em.push(0x00);
    em.extend_from_slice(raw);

    let m = BigUint::from_bytes_be(&em);
    let c = m.modpow(&e, &n);
    let mut c_bytes = c.to_bytes_be();
    if c_bytes.len() < key_len {
        let mut left_padded = vec![0u8; key_len - c_bytes.len()];
        left_padded.append(&mut c_bytes);
        c_bytes = left_padded;
    }
    Ok(bytes_to_hex(&c_bytes))
}

/// 解码酷狗 KRC 歌词：
/// 1. Base64 解码为字节
/// 2. 若前 4 字节为 `krc1`，跳过前 4 字节并与 16 字节固定密钥循环异或 (XOR)，随后 Zlib 解压
/// 3. 若本身已是普通 Base64 LRC 文本，则直接按 UTF-8 输出
/// 4. 清洗剥离逐字时间轴标签 `<...>`，生成轻量标准 `[mm:ss.xx]` 纯文本 LRC 行
pub fn decode_krc_lyrics(base64_str: &str) -> Result<String, String> {
    let raw_bytes = BASE64_STANDARD
        .decode(base64_str.trim())
        .map_err(|e| format!("歌词 Base64 解码失败: {e:?}"))?;

    if raw_bytes.len() < 4 {
        return Err("歌词字节长度过短".to_string());
    }

    // 若非 krc1 头部，尝试直接作为普通 UTF-8 LRC 解析
    if &raw_bytes[0..4] != b"krc1" {
        if let Ok(plain_lrc) = String::from_utf8(raw_bytes.clone()) {
            return Ok(plain_lrc);
        }
    }

    // 跳过前 4 个字节标识，从第 5 个字节开始循环异或解密
    let mut decrypted: Vec<u8> = raw_bytes[4..].to_vec();
    for (i, byte) in decrypted.iter_mut().enumerate() {
        *byte ^= KRC_XOR_KEY[i % 16];
    }

    // Zlib 解压
    let mut decoder = ZlibDecoder::new(&decrypted[..]);
    let mut uncompressed = String::new();
    decoder
        .read_to_string(&mut uncompressed)
        .map_err(|e| format!("Zlib 解压 KRC 失败: {e:?}"))?;

    Ok(clean_krc_to_lrc(&uncompressed))
}

/// 将解压后的 KRC 清洗转换为标准行级纯文本 LRC（架构红线五.3：不驻留逐字对象）
fn clean_krc_to_lrc(krc_content: &str) -> String {
    let mut lines = Vec::new();
    for line in krc_content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if line.starts_with("[ti:")
            || line.starts_with("[ar:")
            || line.starts_with("[al:")
            || line.starts_with("[offset:")
        {
            lines.push(line.to_string());
            continue;
        }

        // 时间轴行格式：[start_ms,duration_ms]<offset,dur,0>字...
        if let Some(bracket_end) = line.find(']') {
            if line.starts_with('[') && bracket_end > 1 {
                let time_part = &line[1..bracket_end];
                if let Some(comma_pos) = time_part.find(',') {
                    if let Ok(start_ms) = time_part[..comma_pos].parse::<u64>() {
                        let total_secs = start_ms / 1000;
                        let mins = total_secs / 60;
                        let secs = total_secs % 60;
                        let centis = (start_ms % 1000) / 10;
                        let standard_time = format!("[{mins:02}:{secs:02}.{centis:02}]");

                        let raw_text = &line[bracket_end + 1..];
                        let mut pure_text = String::with_capacity(raw_text.len());
                        let mut in_tag = false;
                        for ch in raw_text.chars() {
                            if ch == '<' {
                                in_tag = true;
                            } else if ch == '>' {
                                in_tag = false;
                            } else if !in_tag {
                                pure_text.push(ch);
                            }
                        }

                        if !pure_text.trim().is_empty() {
                            lines.push(format!("{standard_time}{pure_text}"));
                        }
                    }
                }
            }
        }
    }

    if lines.is_empty() {
        krc_content.to_string()
    } else {
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lite_rsa_pem_and_encrypt() {
        let raw_cipher = crypto_rsa_encrypt_lite(r#"{"clienttime":1700000000,"token":"abc"}"#)
            .expect("概念版 Raw RSA 加密不应失败");
        assert_eq!(raw_cipher.len(), 256);

        let pkcs1_cipher = rsa_encrypt_pkcs1_v15_lite(r#"{"aes":"123456","uid":1,"token":"t"}"#)
            .expect("概念版 PKCS1v15 RSA 加密不应失败");
        assert_eq!(pkcs1_cipher.len(), 256);
    }
}
