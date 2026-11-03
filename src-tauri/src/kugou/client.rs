use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use lru::LruCache;
use reqwest::{Client, Method};
use serde_json::Value;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::storage::log_warn;

use super::config::{calculate_mid, LITE_APPID, LITE_CLIENTVER};
use super::crypto::{
    sign_key, signature_android_params, signature_register_params, signature_web_params,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncryptType {
    Android,
    Web,
    Register,
}

#[derive(Clone, Debug)]
pub enum RequestBody {
    None,
    Json(Value),
    RawText(String),
}

pub struct RequestOptions<'a> {
    pub method: Method,
    pub base_url: &'a str,
    pub path: &'a str,
    pub params: BTreeMap<String, String>,
    pub body: RequestBody,
    pub headers: Vec<(&'a str, String)>,
    pub encrypt_type: EncryptType,
    pub encrypt_key: bool,
    pub clear_default_params: bool,
    pub cancel_token: Option<CancellationToken>,
    pub use_cache: bool,
}

impl<'a> RequestOptions<'a> {
    pub fn new(method: Method, base_url: &'a str, path: &'a str) -> Self {
        Self {
            method,
            base_url,
            path,
            params: BTreeMap::new(),
            body: RequestBody::None,
            headers: Vec::new(),
            encrypt_type: EncryptType::Android,
            encrypt_key: false,
            clear_default_params: false,
            cancel_token: None,
            use_cache: false,
        }
    }
}

pub struct KugouClient {
    client: Client,
    cache: Arc<Mutex<LruCache<String, Value>>>,
    guid: String,
    mid: String,
    mac: String,
    dfid: Arc<Mutex<String>>,
    token: Arc<Mutex<String>>,
    userid: Arc<Mutex<String>>,
    t1: Arc<Mutex<String>>,
    verify_auth: Arc<Mutex<String>>,
    search_cancel: Arc<Mutex<Option<CancellationToken>>>,
    gateway_alert: Arc<Mutex<Option<String>>>,
}

impl KugouClient {
    pub fn new(guid: String, mac: String, dfid: String) -> Self {
        let mid = calculate_mid(&guid);

        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(5))
            .gzip(true)
            .user_agent("Android15-1070-11083-46-0-DiscoveryDRADProtocol-wifi")
            .build()
            .unwrap_or_else(|_| Client::new());

        // 架构红线（六.2）：接口响应内存缓存严格限定上限 50 条
        let cache_cap = NonZeroUsize::new(50).expect("LRU 容量非零");
        let cache = Arc::new(Mutex::new(LruCache::new(cache_cap)));

        Self {
            client,
            cache,
            guid,
            mid,
            mac,
            dfid: Arc::new(Mutex::new(dfid)),
            token: Arc::new(Mutex::new(String::new())),
            userid: Arc::new(Mutex::new("0".to_string())),
            t1: Arc::new(Mutex::new(String::new())),
            verify_auth: Arc::new(Mutex::new(String::new())),
            search_cancel: Arc::new(Mutex::new(None)),
            gateway_alert: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn take_gateway_alert(&self) -> Option<String> {
        self.gateway_alert.lock().await.take()
    }

    pub fn guid(&self) -> &str {
        &self.guid
    }

    pub fn mid(&self) -> &str {
        &self.mid
    }

    pub fn mac(&self) -> &str {
        &self.mac
    }

    pub fn http_client(&self) -> &Client {
        &self.client
    }

    pub async fn update_auth(&self, token: &str, userid: &str, t1: Option<&str>) {
        {
            let mut t = self.token.lock().await;
            *t = token.to_string();
        }
        {
            let mut u = self.userid.lock().await;
            *u = if userid.is_empty() {
                "0".to_string()
            } else {
                userid.to_string()
            };
        }
        if let Some(t1_val) = t1 {
            let mut guard = self.t1.lock().await;
            *guard = t1_val.to_string();
        }
        // 登录态变更后清空短期 verify_auth 与缓存
        {
            let mut va = self.verify_auth.lock().await;
            va.clear();
        }
        {
            let mut cache = self.cache.lock().await;
            cache.clear();
        }
    }

    pub async fn clear_cache(&self) {
        let mut cache = self.cache.lock().await;
        cache.clear();
    }

    /// 拉取歌曲 100px 封面并利用系统内置 WinRT BitmapDecoder 解码为指定尺寸 BGRA 像素数组（供托盘菜单 0ms 绘制）
    pub async fn fetch_cover_bgra_thumb(&self, raw_cover_url: &str, size_px: u32) -> Option<Vec<u8>> {
        let clean = raw_cover_url
            .trim()
            .replace("{size}", "100")
            .replace("http://", "https://");
        if !clean.starts_with("http") {
            return None;
        }
        let resp = self.client.get(&clean).send().await.ok()?;
        let bytes = resp.bytes().await.ok()?.to_vec();
        if bytes.is_empty() {
            return None;
        }

        #[cfg(windows)]
        {
            tokio::task::spawn_blocking(move || -> Option<Vec<u8>> {
                use windows::Graphics::Imaging::{
                    BitmapAlphaMode, BitmapDecoder, BitmapInterpolationMode, BitmapPixelFormat,
                    BitmapTransform, ColorManagementMode, ExifOrientationMode,
                };
                use windows::Storage::Streams::{DataWriter, InMemoryRandomAccessStream};

                let stream = InMemoryRandomAccessStream::new().ok()?;
                let writer = DataWriter::CreateDataWriter(&stream).ok()?;
                writer.WriteBytes(&bytes).ok()?;
                writer.StoreAsync().ok()?.get().ok()?;
                writer.FlushAsync().ok()?.get().ok()?;
                let _ = writer.DetachStream();
                stream.Seek(0).ok()?;

                let decoder = BitmapDecoder::CreateAsync(&stream).ok()?.get().ok()?;
                let transform = BitmapTransform::new().ok()?;
                transform.SetScaledWidth(size_px).ok()?;
                transform.SetScaledHeight(size_px).ok()?;
                transform
                    .SetInterpolationMode(BitmapInterpolationMode::Linear)
                    .ok()?;

                let pixel_provider = decoder
                    .GetPixelDataTransformedAsync(
                        BitmapPixelFormat::Bgra8,
                        BitmapAlphaMode::Ignore,
                        &transform,
                        ExifOrientationMode::IgnoreExifOrientation,
                        ColorManagementMode::DoNotColorManage,
                    )
                    .ok()?
                    .get()
                    .ok()?;

                let raw = pixel_provider.DetachPixelData().ok()?;
                let vec: Vec<u8> = raw.to_vec();
                if vec.len() == (size_px as usize) * (size_px as usize) * 4 {
                    Some(vec)
                } else {
                    None
                }
            })
            .await
            .ok()
            .flatten()
        }
        #[cfg(not(windows))]
        {
            let _ = size_px;
            None
        }
    }

    pub async fn get_auth_pair(&self) -> (String, String, String) {
        let token = self.token.lock().await.clone();
        let userid = self.userid.lock().await.clone();
        let t1 = self.t1.lock().await.clone();
        (token, userid, t1)
    }

    pub async fn get_dfid(&self) -> String {
        let d = self.dfid.lock().await.clone();
        if d.is_empty() {
            "-".to_string()
        } else {
            d
        }
    }

    pub async fn update_dfid(&self, dfid: &str) {
        let mut d = self.dfid.lock().await;
        *d = dfid.to_string();
    }

    pub async fn get_verify_auth(&self) -> String {
        self.verify_auth.lock().await.clone()
    }

    pub async fn set_verify_auth(&self, auth: &str) {
        let mut va = self.verify_auth.lock().await;
        *va = auth.to_string();
    }

    /// 每次发起新搜索时自动取消上一次未完成的搜索请求（架构红线六.2）
    pub async fn next_search_token(&self) -> CancellationToken {
        let mut guard = self.search_cancel.lock().await;
        if let Some(prev) = guard.take() {
            prev.cancel();
        }
        let token = CancellationToken::new();
        *guard = Some(token.clone());
        token
    }

    pub fn unix_secs() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    pub fn unix_millis() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    /// 发送请求并返回原始字节（含签名、并发取消与最多 2 次指数退避重试）
    pub async fn request_bytes(&self, opts: RequestOptions<'_>) -> Result<Vec<u8>, String> {
        let dfid = self.get_dfid().await;
        let (token, userid, _) = self.get_auth_pair().await;
        let clienttime = Self::unix_secs().to_string();

        let mut params = opts.params.clone();
        if !opts.clear_default_params {
            params
                .entry("dfid".to_string())
                .or_insert_with(|| dfid.clone());
            params
                .entry("mid".to_string())
                .or_insert_with(|| self.mid.clone());
            params
                .entry("uuid".to_string())
                .or_insert_with(|| "-".to_string());
            params
                .entry("appid".to_string())
                .or_insert_with(|| LITE_APPID.to_string());
            params
                .entry("clientver".to_string())
                .or_insert_with(|| LITE_CLIENTVER.to_string());
            params
                .entry("clienttime".to_string())
                .or_insert_with(|| clienttime.clone());

            if !token.is_empty() {
                params
                    .entry("token".to_string())
                    .or_insert_with(|| token.clone());
            }
            if !userid.is_empty() && userid != "0" {
                params
                    .entry("userid".to_string())
                    .or_insert_with(|| userid.clone());
            }
        }

        if opts.encrypt_key {
            let hash = params.get("hash").cloned().unwrap_or_default();
            let appid = params
                .get("appid")
                .cloned()
                .unwrap_or_else(|| LITE_APPID.to_string());
            let uid = params
                .get("userid")
                .cloned()
                .unwrap_or_else(|| "0".to_string());
            let key_val = sign_key(&hash, &self.mid, &uid, &appid);
            params.insert("key".to_string(), key_val);
        }

        let body_str = match &opts.body {
            RequestBody::None => String::new(),
            RequestBody::Json(v) => v.to_string(),
            RequestBody::RawText(s) => s.clone(),
        };

        if !params.contains_key("signature") {
            let sig = match opts.encrypt_type {
                EncryptType::Android => signature_android_params(&params, Some(&body_str)),
                EncryptType::Web => signature_web_params(&params, Some(&body_str)),
                EncryptType::Register => signature_register_params(&params),
            };
            if !sig.is_empty() {
                params.insert("signature".to_string(), sig);
            }
        }

        let url = format!("{}{}", opts.base_url.trim_end_matches('/'), opts.path);
        let header_clienttime = params
            .get("clienttime")
            .cloned()
            .unwrap_or(clienttime);
        let cookie_header = format!(
            "dfid={dfid}; KUGOU_API_MID={}; kg_mid={}; kg_dfid={dfid}; token={token}; userid={userid}",
            self.mid, self.mid
        );

        let max_attempts = 3usize; // 初次请求 + 最多 2 次指数退避重试
        let mut last_err = String::new();

        for attempt in 0..max_attempts {
            if let Some(ref ct) = opts.cancel_token {
                if ct.is_cancelled() {
                    return Err("请求已被取消".to_string());
                }
            }

            if attempt > 0 {
                let backoff_ms = 250u64 * (1u64 << (attempt - 1));
                if let Some(ref ct) = opts.cancel_token {
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_millis(backoff_ms)) => {}
                        _ = ct.cancelled() => return Err("请求在重试等待期间被取消".to_string()),
                    }
                } else {
                    tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                }
            }

            let mut req = self
                .client
                .request(opts.method.clone(), &url)
                .query(&params)
                .header("dfid", &dfid)
                .header("mid", &self.mid)
                .header("clienttime", &header_clienttime)
                .header("kg-rc", "1")
                .header("kg-thash", "5d816a0")
                .header("kg-rec", "1")
                .header("kg-rf", "B9EDA08A64250DEFFBCADDEE00F8F25F")
                .header("Cookie", &cookie_header);

            for (hk, hv) in &opts.headers {
                req = req.header(*hk, hv);
            }

            req = match &opts.body {
                RequestBody::None => req,
                RequestBody::Json(_) => req
                    .header("Content-Type", "application/json;charset=utf-8")
                    .body(body_str.clone()),
                RequestBody::RawText(_) => req
                    .header("Content-Type", "text/plain;charset=utf-8")
                    .body(body_str.clone()),
            };

            let send_fut = req.send();
            let resp_res = if let Some(ref ct) = opts.cancel_token {
                tokio::select! {
                    r = send_fut => r,
                    _ = ct.cancelled() => return Err("请求已被新操作取消".to_string()),
                }
            } else {
                send_fut.await
            };

            match resp_res {
                Ok(resp) => {
                    let bytes_fut = resp.bytes();
                    let bytes_res = if let Some(ref ct) = opts.cancel_token {
                        tokio::select! {
                            b = bytes_fut => b,
                            _ = ct.cancelled() => return Err("响应读取已被新操作取消".to_string()),
                        }
                    } else {
                        bytes_fut.await
                    };

                    match bytes_res {
                        Ok(b) => return Ok(b.to_vec()),
                        Err(e) => {
                            last_err = format!("读取响应字节失败: {e:?}");
                            log_warn(&format!(
                                "酷狗接口读取重试 ({}/{max_attempts}) {}: {last_err}",
                                attempt + 1,
                                opts.path
                            ));
                        }
                    }
                }
                Err(e) => {
                    last_err = format!("网络请求异常: {e:?}");
                    log_warn(&format!(
                        "酷狗接口网络重试 ({}/{max_attempts}) {}: {last_err}",
                        attempt + 1,
                        opts.path
                    ));
                }
            }
        }

        Err(last_err)
    }

    /// 发送请求并解析为 JSON（可选 50 条有界 LRU 缓存）
    pub async fn request_json(&self, opts: RequestOptions<'_>) -> Result<Value, String> {
        let use_cache = opts.use_cache;
        let cache_key = if use_cache {
            let body_dbg = match &opts.body {
                RequestBody::None => String::new(),
                RequestBody::Json(v) => v.to_string(),
                RequestBody::RawText(s) => s.clone(),
            };
            format!("{}{}:{:?}:{}", opts.base_url, opts.path, opts.params, body_dbg)
        } else {
            String::new()
        };

        if use_cache {
            let mut cache = self.cache.lock().await;
            if let Some(hit) = cache.get(&cache_key) {
                return Ok(hit.clone());
            }
        }

        let raw_bytes = self.request_bytes(opts).await?;
        let text = String::from_utf8_lossy(&raw_bytes);
        // 兼容部分旧接口可能带有的前后空白或 BOM
        let trimmed = text.trim().trim_start_matches('\u{feff}');
        let json_val: Value = serde_json::from_str(trimmed)
            .map_err(|e| format!("响应 JSON 解析失败: {e:?}"))?;

        // 统一拦截登录过期错误码与风控拦截码
        let err_code = json_val
            .get("error_code")
            .or_else(|| json_val.get("errcode"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        if matches!(err_code, 20018 | 30001 | 10018) {
            let has_token = !self.token.lock().await.is_empty();
            if has_token {
                log_warn(&format!("检测到酷狗登录令牌已过期 (err_code={err_code})，已清理短期凭证"));
                self.update_auth("", "0", Some("")).await;
                *self.gateway_alert.lock().await =
                    Some("酷狗账号登录状态已过期，请重新登录".to_string());
            }
        } else if matches!(err_code, 20022 | 30002 | 10032) {
            log_warn(&format!("触发酷狗风控验证拦截 (err_code={err_code})"));
            *self.gateway_alert.lock().await =
                Some("触发酷狗安全风控校验，请稍后重试或使用扫码重新登录".to_string());
        }

        if use_cache && (err_code == 0 || err_code == 200) {
            let mut cache = self.cache.lock().await;
            cache.put(cache_key, json_val.clone());
        }

        Ok(json_val)
    }
}
