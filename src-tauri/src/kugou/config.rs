use num_bigint::BigUint;
use uuid::Uuid;

/// 酷狗概念版 (Lite / Young) 核心协议常量
pub const LITE_APPID: &str = "3116";
pub const LITE_CLIENTVER: &str = "11440";
pub const SRCAPPID: &str = "2919";
pub const SERVER_DEV: &str = "KuYin";
pub const WX_LITE_APPID: &str = "wx72b795aca60ad321";
pub const WX_LITE_SECRET: &str = "33e486041e5e25729a4e3d2da7502f9a";

/// 生成全新 UUID v4 标准格式字符串作为设备持久化 guid
pub fn generate_guid() -> String {
    Uuid::new_v4().to_string()
}

/// 生成模拟设备 MAC 地址（基于 UUID 随机字节格式化，仅在首次初始化生成并持久化）
pub fn generate_mac() -> String {
    let b = Uuid::new_v4().into_bytes();
    format!(
        "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        b[0] & 0xfe,
        b[1],
        b[2],
        b[3],
        b[4],
        b[5]
    )
}

/// 生成指定长度的随机小写字母数字串（基于操作系统 CSPRNG UUID v4，零额外 rand 依赖）
pub fn random_alnum_lower(len: usize) -> String {
    const CHARSET: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = String::with_capacity(len);
    while out.len() < len {
        let bytes = Uuid::new_v4().into_bytes();
        for b in bytes {
            if out.len() >= len {
                break;
            }
            let idx = (b as usize) % CHARSET.len();
            out.push(CHARSET[idx] as char);
        }
    }
    out
}

/// 对齐 KuGouMusicApi `calculateMid`：
/// 计算 guid 的 32 位小写 MD5，按十六进制大整数解析后转为十进制字符串
pub fn calculate_mid(guid: &str) -> String {
    let hex_str = super::crypto::md5_hex(guid.as_bytes());
    if let Some(biguint) = BigUint::parse_bytes(hex_str.as_bytes(), 16) {
        biguint.to_str_radix(10)
    } else {
        "0".to_string()
    }
}
