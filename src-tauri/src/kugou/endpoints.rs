use reqwest::Method;
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use crate::storage::{log_info, log_warn};

use super::client::{EncryptType, KugouClient, RequestBody, RequestOptions};
use super::config::{
    random_alnum_lower, LITE_APPID, LITE_CLIENTVER, SERVER_DEV, SRCAPPID, WX_LITE_APPID,
    WX_LITE_SECRET,
};
use super::crypto::{
    aes256_cbc_encrypt_hex, crypto_aes_decrypt, crypto_aes_encrypt_auto, crypto_rsa_encrypt_lite,
    decode_krc_lyrics, md5_hex, playlist_aes_decrypt_bytes, playlist_aes_encrypt,
    rsa_encrypt_pkcs1_v15_lite, sha1_hex, sign_params_key,
};

const LITE_T1_KEY: &[u8] = b"5e4ef500e9597fe004bd09a46d8add98";
const LITE_T1_IV: &[u8] = b"04bd09a46d8add98";
const LITE_T2_KEY: &[u8] = b"fd14b35e3f81af3817a20ae7adae7020";
const LITE_T2_IV: &[u8] = b"17a20ae7adae7020";
const LITE_TOKEN_KEY: &[u8] = b"c24f74ca2820225badc01946dba4fdf7";
const LITE_TOKEN_IV: &[u8] = b"adc01946dba4fdf7";

/// 计算当前东八区日期字符串 `YYYY-MM-DD`（对齐 `youth_day_vip.js` 的 `receive_day` 参数）
fn current_cn_date_string() -> String {
    let secs_cn = KugouClient::unix_secs() + 8 * 3600;
    let z = (secs_cn / 86_400) as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    format!("{year:04}-{m:02}-{d:02}")
}

impl KugouClient {
    /// 1. 注册虚拟设备获取持久化 24 位 `dfid`（升级至上游最新 `/risk/v3/r_register_dev`）
    pub async fn register_dev(&self) -> Result<String, String> {
        let (token, userid, _) = self.get_auth_pair().await;
        let uid_num = userid.parse::<u64>().unwrap_or(0);

        let device_map = json!({
            "availableRamSize": 4983533568u64,
            "availableRomSize": 48114719u64,
            "availableSDSize": 48114717u64,
            "basebandVer": "",
            "batteryLevel": 100,
            "batteryStatus": 3,
            "brand": "Redmi",
            "buildSerial": "unknown",
            "device": "marble",
            "imei": self.guid(),
            "imsi": "",
            "manufacturer": "Xiaomi",
            "uuid": self.guid(),
            "accelerometer": false,
            "accelerometerValue": "",
            "gravity": false,
            "gravityValue": "",
            "gyroscope": false,
            "gyroscopeValue": "",
            "light": false,
            "lightValue": "",
            "magnetic": false,
            "magneticValue": "",
            "orientation": false,
            "orientationValue": "",
            "pressure": false,
            "pressureValue": "",
            "step_counter": false,
            "step_counterValue": "",
            "temperature": false,
            "temperatureValue": ""
        });

        let (aes_b64, aes_key6) = playlist_aes_encrypt(&device_map.to_string())?;
        let p_plain = json!({
            "aes": aes_key6,
            "uid": uid_num,
            "token": token
        })
        .to_string();
        let p_hex = rsa_encrypt_pkcs1_v15_lite(&p_plain)?;

        let mut opts = RequestOptions::new(
            Method::POST,
            "https://userservice.kugou.com",
            "/risk/v3/r_register_dev",
        );
        opts.params.insert("part".to_string(), "1".to_string());
        opts.params.insert("platid".to_string(), "1".to_string());
        opts.params.insert("p".to_string(), p_hex);
        opts.body = RequestBody::RawText(aes_b64);

        let raw_resp = self.request_bytes(opts).await?;
        let decrypted_str = match playlist_aes_decrypt_bytes(&raw_resp, &aes_key6) {
            Ok(s) => s,
            Err(_) => String::from_utf8_lossy(&raw_resp).to_string(),
        };

        if let Ok(val) = serde_json::from_str::<Value>(&decrypted_str) {
            if let Some(dfid) = val
                .get("data")
                .and_then(|d| d.get("dfid"))
                .and_then(|v| v.as_str())
            {
                if !dfid.is_empty() {
                    self.update_dfid(dfid).await;
                    log_info(&format!("成功注册酷狗概念版设备指纹 dfid: {dfid}"));
                    return Ok(dfid.to_string());
                }
            }
        }

        Err("设备注册未返回有效 dfid".to_string())
    }

    /// 2. 扫码登录：获取二维码 key 与官方 `qrcode_img` Base64 图片 (`/v2/qrcode`)
    pub async fn get_qr_key(&self) -> Result<Value, String> {
        let mut opts = RequestOptions::new(Method::GET, "https://login-user.kugou.com", "/v2/qrcode");
        opts.encrypt_type = EncryptType::Web;
        opts.params.insert("appid".to_string(), "1001".to_string());
        opts.params.insert("type".to_string(), "1".to_string());
        opts.params.insert("plat".to_string(), "4".to_string());
        opts.params.insert(
            "qrcode_txt".to_string(),
            format!("https://h5.kugou.com/apps/loginQRCode/html/index.html?appid={LITE_APPID}&"),
        );
        opts.params
            .insert("srcappid".to_string(), SRCAPPID.to_string());

        let mut res = self.request_json(opts).await?;
        // 自动规范化 qrcode_img 为可直接绑定 <img> 的 data URI
        if let Some(data_obj) = res.get_mut("data").and_then(|d| d.as_object_mut()) {
            if let Some(img_str) = data_obj.get("qrcode_img").and_then(|v| v.as_str()) {
                if !img_str.is_empty() && !img_str.starts_with("data:image") {
                    let formatted = format!("data:image/png;base64,{img_str}");
                    data_obj.insert("qrcode_img".to_string(), Value::String(formatted));
                }
            }
        }
        Ok(res)
    }

    /// 3. 扫码登录：轮询扫码状态 (`/v2/get_userinfo_qrcode`)
    ///    status: 0=过期, 1=待扫码, 2=已扫码待确认, 4=授权成功（含 token 与 userid）
    pub async fn check_qr_status(&self, qrcode_key: &str) -> Result<Value, String> {
        let mut opts = RequestOptions::new(
            Method::GET,
            "https://login-user.kugou.com",
            "/v2/get_userinfo_qrcode",
        );
        opts.encrypt_type = EncryptType::Web;
        opts.params.insert("plat".to_string(), "4".to_string());
        opts.params
            .insert("appid".to_string(), LITE_APPID.to_string());
        opts.params
            .insert("srcappid".to_string(), SRCAPPID.to_string());
        opts.params
            .insert("qrcode".to_string(), qrcode_key.to_string());
        opts.params
            .insert("dev".to_string(), SERVER_DEV.to_string());

        self.request_json(opts).await
    }

    /// 3b. 微信开放平台扫码登录：创建微信二维码 (`login_wx_create`)
    pub async fn create_wx_qr(&self) -> Result<Value, String> {
        let token_resp: Value = self
            .http_client()
            .get("https://api.weixin.qq.com/cgi-bin/token")
            .query(&[
                ("appid", WX_LITE_APPID),
                ("secret", WX_LITE_SECRET),
                ("grant_type", "client_credential"),
            ])
            .send()
            .await
            .map_err(|e| format!("获取微信开放平台 token 失败: {e}"))?
            .json()
            .await
            .map_err(|e| format!("解析微信开放平台 token 失败: {e}"))?;

        let access_token = token_resp
            .get("access_token")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| format!("微信 token 响应异常: {token_resp}"))?;

        let ticket_resp: Value = self
            .http_client()
            .get("https://api.weixin.qq.com/cgi-bin/ticket/getticket")
            .query(&[("access_token", access_token), ("type", "2")])
            .send()
            .await
            .map_err(|e| format!("获取微信 sdk_ticket 失败: {e}"))?
            .json()
            .await
            .map_err(|e| format!("解析微信 sdk_ticket 失败: {e}"))?;

        let ticket = ticket_resp
            .get("ticket")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| format!("微信 sdk_ticket 响应异常: {ticket_resp}"))?;

        let timestamp = Self::unix_millis().to_string();
        let noncestr = md5_hex(random_alnum_lower(16).as_bytes());
        let sig_plain = format!(
            "appid={WX_LITE_APPID}&noncestr={noncestr}&sdk_ticket={ticket}&timestamp={timestamp}"
        );
        let signature = sha1_hex(sig_plain.as_bytes());

        let mut connect_resp: Value = self
            .http_client()
            .get("https://open.weixin.qq.com/connect/sdk/qrconnect")
            .query(&[
                ("appid", WX_LITE_APPID),
                ("noncestr", noncestr.as_str()),
                ("timestamp", timestamp.as_str()),
                ("scope", "snsapi_userinfo"),
                ("signature", signature.as_str()),
            ])
            .send()
            .await
            .map_err(|e| format!("生成微信登录二维码失败: {e}"))?
            .json()
            .await
            .map_err(|e| format!("解析微信二维码响应失败: {e}"))?;

        if connect_resp.get("errcode").and_then(|v| v.as_i64()) != Some(0) {
            return Err(format!("微信二维码返回错误: {connect_resp}"));
        }

        let b64 = connect_resp
            .get("qrcode")
            .and_then(|q| q.get("qrcodebase64"))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        if !b64.is_empty() {
            let formatted = if b64.starts_with("data:image") {
                b64
            } else {
                format!("data:image/jpeg;base64,{b64}")
            };
            if let Some(obj) = connect_resp.as_object_mut() {
                obj.insert("qrcode_img".to_string(), Value::String(formatted));
            }
        }

        Ok(connect_resp)
    }

    /// 3c. 微信开放平台扫码登录：长轮询扫码状态 (`login_wx_check`)
    ///     wx_errcode: 408=等待扫码, 404=已扫码待确认, 405=已确认(含 wx_code), 402/403=过期或取消
    pub async fn check_wx_qr_status(&self, uuid: &str) -> Result<Value, String> {
        let now_ms = Self::unix_millis().to_string();
        let req = self
            .http_client()
            .get("https://long.open.weixin.qq.com/connect/l/qrconnect")
            .query(&[("f", "json"), ("uuid", uuid.trim()), ("_", now_ms.as_str())])
            .timeout(std::time::Duration::from_secs(28));

        match req.send().await {
            Ok(resp) => resp
                .json::<Value>()
                .await
                .map_err(|e| format!("解析微信扫码轮询响应失败: {e}")),
            Err(e) if e.is_timeout() => Ok(json!({ "wx_errcode": 408 })),
            Err(e) => Err(format!("轮询微信扫码状态失败: {e}")),
        }
    }

    /// 3d. 微信开放平台扫码登录：用 `wx_code` 换取酷狗概念版登录态 (`/v6/login_by_openplat`)
    pub async fn login_by_wx_code(&self, code: &str) -> Result<Value, String> {
        let wx_oauth: Value = self
            .http_client()
            .post("https://api.weixin.qq.com/sns/oauth2/access_token")
            .query(&[
                ("secret", WX_LITE_SECRET),
                ("appid", WX_LITE_APPID),
                ("code", code.trim()),
                ("grant_type", "authorization_code"),
            ])
            .send()
            .await
            .map_err(|e| format!("换取微信 OAuth token 失败: {e}"))?
            .json()
            .await
            .map_err(|e| format!("解析微信 OAuth token 失败: {e}"))?;

        let wx_access_token = wx_oauth
            .get("access_token")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| format!("微信 OAuth 未返回有效 access_token: {wx_oauth}"))?;
        let wx_openid = wx_oauth
            .get("openid")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| format!("微信 OAuth 未返回有效 openid: {wx_oauth}"))?;

        let now_ms = Self::unix_millis();
        let plain_payload = json!({
            "access_token": wx_access_token
        })
        .to_string();
        let (enc_params, temp_key) = crypto_aes_encrypt_auto(&plain_payload)?;
        let pk_plain = json!({
            "clienttime_ms": now_ms,
            "key": temp_key
        })
        .to_string();
        let pk = crypto_rsa_encrypt_lite(&pk_plain)?.to_uppercase();

        let t1_plain = format!("|{now_ms}");
        let t1 = aes256_cbc_encrypt_hex(t1_plain.as_bytes(), LITE_T1_KEY, LITE_T1_IV)?;
        let t2_plain = format!(
            "{}|0f607264fc6318a92b9e13c65db7cd3c|{}|{SERVER_DEV}|{now_ms}",
            self.guid(),
            self.mac()
        );
        let t2 = aes256_cbc_encrypt_hex(t2_plain.as_bytes(), LITE_T2_KEY, LITE_T2_IV)?;

        let mut opts = RequestOptions::new(
            Method::POST,
            "https://gateway.kugou.com",
            "/v6/login_by_openplat",
        );
        opts.headers
            .push(("x-router", "login.user.kugou.com".to_string()));
        opts.body = RequestBody::Json(json!({
            "dev": SERVER_DEV,
            "force_login": 1,
            "partnerid": 36,
            "clienttime_ms": now_ms,
            "t1": t1,
            "t2": t2,
            "t3": "MCwwLDAsMCwwLDAsMCwwLDA=",
            "openid": wx_openid,
            "params": enc_params,
            "pk": pk
        }));

        let mut res = self.request_json(opts).await?;
        Self::merge_decrypted_secu_params(&mut res, &temp_key);
        Ok(res)
    }

    /// 4. 发送手机短信验证码 (`/v7/send_mobile_code`)
    pub async fn send_mobile_code(&self, mobile: &str) -> Result<Value, String> {
        let mut opts = RequestOptions::new(
            Method::POST,
            "http://login.user.kugou.com",
            "/v7/send_mobile_code",
        );
        opts.body = RequestBody::Json(json!({
            "businessid": 5,
            "mobile": mobile.trim(),
            "plat": 3
        }));
        self.request_json(opts).await
    }

    /// 5. 手机短信验证码登录 (`/v7/login_by_verifycode` + `secu_params` 解密)
    pub async fn login_by_mobile(&self, mobile: &str, code: &str) -> Result<Value, String> {
        let clean_mobile = mobile.trim();
        let now_ms = Self::unix_millis();
        let dfid = self.get_dfid().await;

        let plain_payload = json!({
            "mobile": clean_mobile,
            "code": code.trim()
        })
        .to_string();
        let (enc_params, temp_key) = crypto_aes_encrypt_auto(&plain_payload)?;

        let masked_mobile = if clean_mobile.len() >= 11 {
            format!("{}*****{}", &clean_mobile[0..2], &clean_mobile[10..11])
        } else {
            clean_mobile.to_string()
        };

        let t1_plain = format!("|{now_ms}");
        let t1 = aes256_cbc_encrypt_hex(t1_plain.as_bytes(), LITE_T1_KEY, LITE_T1_IV)?;
        let t2_plain = format!(
            "{}|0f607264fc6318a92b9e13c65db7cd3c|{}|{SERVER_DEV}|{now_ms}",
            self.guid(),
            self.mac()
        );
        let t2 = aes256_cbc_encrypt_hex(t2_plain.as_bytes(), LITE_T2_KEY, LITE_T2_IV)?;

        let pk_plain = json!({
            "clienttime_ms": now_ms,
            "key": temp_key
        })
        .to_string();
        let pk = crypto_rsa_encrypt_lite(&pk_plain)?.to_uppercase();

        let body = json!({
            "plat": 1,
            "support_multi": 1,
            "t1": t1,
            "t2": t2,
            "clienttime_ms": now_ms,
            "mobile": masked_mobile,
            "key": sign_params_key(&now_ms.to_string()),
            "pk": pk,
            "params": enc_params,
            "dfid": dfid,
            "dev": SERVER_DEV,
            "gitversion": "5f0b7c4"
        });

        let mut opts = RequestOptions::new(
            Method::POST,
            "https://loginserviceretry.kugou.com",
            "/v7/login_by_verifycode",
        );
        opts.body = RequestBody::Json(body);
        opts.headers
            .push(("support-calm", "1".to_string()));
        opts.headers.push((
            "User-Agent",
            "Android16-1070-11440-130-0-LOGIN-wifi".to_string(),
        ));

        let mut res = self.request_json(opts).await?;
        Self::merge_decrypted_secu_params(&mut res, &temp_key);
        Ok(res)
    }

    /// 6. 账号密码登录 (`/v9/login_by_pwd` + `secu_params` 解密)
    pub async fn login_by_password(&self, username: &str, password: &str) -> Result<Value, String> {
        let now_ms = Self::unix_millis();
        // 酷狗密码若非 32 位 MD5 则先计算一次 MD5
        let pwd_md5 = if password.trim().len() == 32 && password.chars().all(|c| c.is_ascii_hexdigit()) {
            password.trim().to_lowercase()
        } else {
            super::crypto::md5_hex(password.trim().as_bytes())
        };

        let plain_payload = json!({
            "pwd": pwd_md5,
            "code": "",
            "clienttime_ms": now_ms
        })
        .to_string();
        let (enc_params, temp_key) = crypto_aes_encrypt_auto(&plain_payload)?;
        let pk_plain = json!({
            "clienttime_ms": now_ms,
            "key": temp_key
        })
        .to_string();
        let pk = crypto_rsa_encrypt_lite(&pk_plain)?.to_uppercase();

        let t1_plain = format!("|{now_ms}");
        let t1 = aes256_cbc_encrypt_hex(t1_plain.as_bytes(), LITE_T1_KEY, LITE_T1_IV)?;
        let t2_plain = format!(
            "{}|0f607264fc6318a92b9e13c65db7cd3c|{}|{SERVER_DEV}|{now_ms}",
            self.guid(),
            self.mac()
        );
        let t2 = aes256_cbc_encrypt_hex(t2_plain.as_bytes(), LITE_T2_KEY, LITE_T2_IV)?;

        let mut opts = RequestOptions::new(
            Method::POST,
            "https://gateway.kugou.com",
            "/v9/login_by_pwd",
        );
        opts.headers
            .push(("x-router", "login.user.kugou.com".to_string()));
        opts.body = RequestBody::Json(json!({
            "plat": 1,
            "support_multi": 1,
            "clienttime_ms": now_ms,
            "t1": t1,
            "t2": t2,
            "t3": "MCwwLDAsMCwwLDAsMCwwLDA=",
            "dev": SERVER_DEV,
            "username": username.trim(),
            "params": enc_params,
            "pk": pk
        }));

        let mut res = self.request_json(opts).await?;
        Self::merge_decrypted_secu_params(&mut res, &temp_key);
        Ok(res)
    }

    /// 7. 刷新登录令牌 (`/v5/login_by_token`)
    pub async fn refresh_login_token(&self) -> Result<Value, String> {
        let (token, userid, saved_t1) = self.get_auth_pair().await;
        if token.is_empty() || userid == "0" {
            return Err("当前未登录，无需刷新 Token".to_string());
        }

        let now_ms = Self::unix_millis();
        let dfid = self.get_dfid().await;

        let p3_plain = json!({
            "clienttime": now_ms / 1000,
            "token": token
        })
        .to_string();
        let p3 = aes256_cbc_encrypt_hex(p3_plain.as_bytes(), LITE_TOKEN_KEY, LITE_TOKEN_IV)?;
        let (enc_params, temp_key) = crypto_aes_encrypt_auto("{}")?;
        let pk_plain = json!({
            "clienttime_ms": now_ms,
            "key": temp_key
        })
        .to_string();
        let pk = crypto_rsa_encrypt_lite(&pk_plain)?;

        let t1_plain = if saved_t1.is_empty() {
            format!("|{now_ms}")
        } else {
            format!("{saved_t1}|{now_ms}")
        };
        let t1 = aes256_cbc_encrypt_hex(t1_plain.as_bytes(), LITE_T1_KEY, LITE_T1_IV)?;
        let t2_plain = format!(
            "{}|0f607264fc6318a92b9e13c65db7cd3c|{}|{SERVER_DEV}|{now_ms}",
            self.guid(),
            self.mac()
        );
        let t2 = aes256_cbc_encrypt_hex(t2_plain.as_bytes(), LITE_T2_KEY, LITE_T2_IV)?;

        let mut opts = RequestOptions::new(
            Method::POST,
            "http://login.user.kugou.com",
            "/v5/login_by_token",
        );
        opts.body = RequestBody::Json(json!({
            "dfid": dfid,
            "p3": p3,
            "plat": 1,
            "t1": t1,
            "t2": t2,
            "t3": "MCwwLDAsMCwwLDAsMCwwLDA=",
            "pk": pk,
            "params": enc_params,
            "userid": userid,
            "clienttime_ms": now_ms,
            "dev": SERVER_DEV
        }));

        let mut res = self.request_json(opts).await?;
        Self::merge_decrypted_secu_params(&mut res, &temp_key);
        Ok(res)
    }

    fn merge_decrypted_secu_params(res: &mut Value, temp_key: &str) {
        let Some(data_obj) = res.get_mut("data").and_then(|d| d.as_object_mut()) else {
            return;
        };
        let Some(secu_hex) = data_obj
            .get("secu_params")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
        else {
            return;
        };

        if let Ok(decrypted) = crypto_aes_decrypt(&secu_hex, temp_key) {
            if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(&decrypted) {
                for (k, v) in map {
                    data_obj.insert(k, v);
                }
            } else if !decrypted.is_empty() {
                data_obj.insert("token".to_string(), Value::String(decrypted));
            }
        }
    }

    /// 8. 获取用户资料详情 (`/v3/get_my_info`)
    pub async fn get_user_detail(&self) -> Result<Value, String> {
        let (token, userid, _) = self.get_auth_pair().await;
        let uid_num = userid.parse::<u64>().unwrap_or(0);
        let now_sec = Self::unix_secs();

        let pk_plain = json!({
            "token": token,
            "clienttime": now_sec
        })
        .to_string();
        let pk = crypto_rsa_encrypt_lite(&pk_plain)?.to_uppercase();

        let mut opts = RequestOptions::new(
            Method::POST,
            "https://gateway.kugou.com",
            "/v3/get_my_info",
        );
        opts.params.insert("plat".to_string(), "1".to_string());
        opts.headers
            .push(("x-router", "usercenter.kugou.com".to_string()));
        opts.body = RequestBody::Json(json!({
            "visit_time": now_sec,
            "usertype": 1,
            "p": pk,
            "userid": uid_num
        }));

        self.request_json(opts).await
    }

    /// 9. 每日自动签到领取概念版 VIP + 自动升级 VIP 闭环
    pub async fn claim_youth_vip(&self) -> Result<Value, String> {
        let (_, userid, _) = self.get_auth_pair().await;
        let uid_num = userid.parse::<u64>().unwrap_or(0);
        let receive_day = current_cn_date_string();

        // 步骤 1：领取概念版每日畅听 VIP（补齐上游必填的 receive_day）
        let mut opts1 = RequestOptions::new(
            Method::POST,
            "https://gateway.kugou.com",
            "/youth/v1/recharge/receive_vip_listen_song",
        );
        opts1
            .params
            .insert("source_id".to_string(), "90139".to_string());
        opts1
            .params
            .insert("receive_day".to_string(), receive_day);
        opts1.headers.push((
            "content-type",
            "application/x-www-form-urlencoded".to_string(),
        ));
        let res1 = self.request_json(opts1).await?;

        // 步骤 2：升级为概念版全曲库 VIP 奖励
        let mut opts2 = RequestOptions::new(
            Method::POST,
            "https://gateway.kugou.com",
            "/youth/v1/listen_song/upgrade_vip_reward",
        );
        opts2
            .params
            .insert("kugouid".to_string(), uid_num.to_string());
        opts2.params.insert("ad_type".to_string(), "1".to_string());
        let upgrade_res = self.request_json(opts2).await.ok();

        Ok(json!({
            "receive": res1,
            "upgrade": upgrade_res
        }))
    }

    /// 10. 播放前用户校验 (`/v1/user_verify`)
    pub async fn ensure_user_verify_auth(&self, cancel_token: Option<CancellationToken>) -> String {
        let existing = self.get_verify_auth().await;
        if !existing.is_empty() {
            return existing;
        }

        let mut opts = RequestOptions::new(
            Method::GET,
            "http://gateway.kugou.com",
            "/v1/user_verify",
        );
        opts.params
            .insert("module_id".to_string(), "51".to_string());
        opts.headers
            .push(("x-router", "trackercdngz.kugou.com".to_string()));
        opts.cancel_token = cancel_token;

        if let Ok(res) = self.request_json(opts).await {
            if let Some(auth) = res
                .get("data")
                .and_then(|d| d.get("auth"))
                .and_then(|v| v.as_str())
            {
                if !auth.is_empty() {
                    self.set_verify_auth(auth).await;
                    return auth.to_string();
                }
            }
        }
        String::new()
    }

    /// 11. 查询单曲各音质独立音源 `hash`（对齐 `/v2/get_res_privilege/lite`）
    ///     提取标准容器：level=2 -> 128K MP3、level=4 -> 320K MP3、level=5 -> FLAC（排除 OGG 容器）
    pub async fn get_privilege_hash_map(
        &self,
        hash: &str,
        album_id: Option<&str>,
        cancel_token: Option<CancellationToken>,
    ) -> std::collections::HashMap<String, String> {
        let mut map = std::collections::HashMap::new();
        let clean_hash = hash.trim().to_lowercase();
        if clean_hash.is_empty() {
            return map;
        }
        let alb_id_num = album_id
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0);

        let mut opts = RequestOptions::new(
            Method::POST,
            "https://gateway.kugou.com",
            "/v2/get_res_privilege/lite",
        );
        opts.headers
            .push(("x-router", "media.store.kugou.com".to_string()));
        opts.cancel_token = cancel_token;
        opts.body = RequestBody::Json(json!({
            "appid": LITE_APPID,
            "area_code": 1,
            "behavior": "play",
            "clientver": LITE_CLIENTVER,
            "need_hash_offset": 1,
            "relate": 1,
            "support_verify": 1,
            "resource": [{
                "type": "audio",
                "page_id": 0,
                "hash": clean_hash,
                "album_id": alb_id_num
            }],
            "qualities": ["128", "320", "flac", "high"]
        }));

        if let Ok(res) = self.request_json(opts).await {
            if let Some(first) = res
                .get("data")
                .and_then(|d| d.as_array())
                .and_then(|arr| arr.first())
            {
                if let Some(goods) = first.get("relate_goods").and_then(|g| g.as_array()) {
                    for (q_key, target_level) in [("128", 2i64), ("320", 4i64), ("flac", 5i64)] {
                        let matched = goods
                            .iter()
                            .find(|item| {
                                item.get("level").and_then(|v| v.as_i64()) == Some(target_level)
                            })
                            .or_else(|| {
                                goods.iter().find(|item| {
                                    let q_match = item.get("quality").and_then(|v| v.as_str())
                                        == Some(q_key);
                                    let lvl = item.get("level").and_then(|v| v.as_i64()).unwrap_or(0);
                                    let ext = item
                                        .get("info")
                                        .and_then(|i| i.get("extname"))
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("");
                                    q_match && lvl != 1 && !ext.eq_ignore_ascii_case("ogg")
                                })
                            });

                        if let Some(item) = matched {
                            if let Some(h) = item.get("hash").and_then(|v| v.as_str()) {
                                let h_clean = h.trim().to_lowercase();
                                if !h_clean.is_empty() {
                                    map.insert(q_key.to_string(), h_clean);
                                }
                            }
                        }
                    }
                }
            }
        }
        map
    }

    /// 获取指定单一品质（`"128"` / `"320"` / `"flac"`）的原生硬解兼容播放直链
    /// （彻底剔除 `need_ogg=1`，双通道互补：概念版授权 `/tracker/v5/url` + 标准 `/v5/url`）
    pub async fn get_song_urls_for_quality(
        &self,
        target_hash: &str,
        album_id: Option<&str>,
        album_audio_id: Option<i64>,
        quality: &str,
        cancel_token: Option<CancellationToken>,
    ) -> Result<Vec<String>, String> {
        let clean_hash = target_hash.trim().to_lowercase();
        if clean_hash.is_empty() {
            return Err("歌曲 hash 为空".to_string());
        }

        if let Some(ref ct) = cancel_token {
            if ct.is_cancelled() {
                return Err("切歌换链已被取消".to_string());
            }
        }

        let verify_auth = self.ensure_user_verify_auth(cancel_token.clone()).await;
        let audio_id = album_audio_id.unwrap_or(0);
        let alb_id_num = album_id
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0);

        // 通道 1：概念版单曲授权 + `/tracker/v5/url`（不传 need_ogg，确保返回标准 MP3/FLAC 且兼容概念版 VIP）
        let mut auth_opts = RequestOptions::new(
            Method::GET,
            "http://gateway.kugou.com",
            "/v1/authorization",
        );
        auth_opts
            .params
            .insert("authorization".to_string(), verify_auth);
        auth_opts
            .params
            .insert("module_id".to_string(), "51".to_string());
        auth_opts
            .params
            .insert("album_audio_id".to_string(), audio_id.to_string());
        auth_opts
            .params
            .insert("clientver".to_string(), "11561".to_string());
        auth_opts
            .params
            .insert("hash".to_string(), clean_hash.clone());
        auth_opts
            .headers
            .push(("x-router", "trackercdngz.kugou.com".to_string()));
        auth_opts.cancel_token = cancel_token.clone();

        let (song_auth_str, open_time) = match self.request_json(auth_opts).await {
            Ok(val) => {
                let a = val
                    .get("data")
                    .and_then(|d| d.get("auth"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let ot = val
                    .get("data")
                    .and_then(|d| d.get("open_time"))
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                (a, ot)
            }
            Err(_) => (String::new(), 0),
        };

        let mut tracker = RequestOptions::new(
            Method::GET,
            "https://gateway.kugou.com",
            "/tracker/v5/url",
        );
        tracker.encrypt_key = true;
        tracker.cancel_token = cancel_token.clone();
        tracker
            .params
            .insert("album_id".to_string(), alb_id_num.to_string());
        tracker
            .params
            .insert("area_code".to_string(), "1".to_string());
        tracker.params.insert("module".to_string(), String::new());
        tracker
            .params
            .insert("hash".to_string(), clean_hash.clone());
        tracker
            .params
            .insert("need_m".to_string(), "0".to_string());
        tracker
            .params
            .insert("ssa_flag".to_string(), "is_fromtrack".to_string());
        tracker
            .params
            .insert("version".to_string(), "11430".to_string());
        tracker
            .params
            .insert("open_time".to_string(), open_time.to_string());
        tracker.params.insert("ptype".to_string(), "0".to_string());
        tracker
            .params
            .insert("page_id".to_string(), "967177915".to_string());
        tracker
            .params
            .insert("auth".to_string(), song_auth_str);
        tracker.params.insert("mtype".to_string(), "0".to_string());
        tracker
            .params
            .insert("quality".to_string(), quality.to_string());
        tracker
            .params
            .insert("album_audio_id".to_string(), audio_id.to_string());
        tracker
            .params
            .insert("behavior".to_string(), "play".to_string());
        tracker.params.insert("pid".to_string(), "411".to_string());
        tracker
            .params
            .insert("module_id".to_string(), "51".to_string());
        tracker.params.insert("cmd".to_string(), "26".to_string());
        tracker.params.insert(
            "ppage_id".to_string(),
            "356753938,823673182,967485191".to_string(),
        );
        tracker
            .params
            .insert("clientver".to_string(), "11561".to_string());
        tracker
            .params
            .insert("pidversion".to_string(), "3001".to_string());
        tracker
            .params
            .insert("cdnBackup".to_string(), "1".to_string());

        if let Ok(res) = self.request_json(tracker).await {
            let urls = Self::extract_tracker_urls(&res);
            if !urls.is_empty() {
                return Ok(urls);
            }
        }

        if let Some(ref ct) = cancel_token {
            if ct.is_cancelled() {
                return Err("切歌换链已被取消".to_string());
            }
        }

        // 通道 2：标准 `/v5/url`（x-router: trackercdn.kugou.com，对齐上游 song_url.js）
        let mut cdn_opts =
            RequestOptions::new(Method::GET, "https://gateway.kugou.com", "/v5/url");
        cdn_opts.encrypt_key = true;
        cdn_opts.cancel_token = cancel_token;
        cdn_opts
            .headers
            .push(("x-router", "trackercdn.kugou.com".to_string()));
        cdn_opts
            .params
            .insert("album_id".to_string(), alb_id_num.to_string());
        cdn_opts
            .params
            .insert("area_code".to_string(), "1".to_string());
        cdn_opts
            .params
            .insert("hash".to_string(), clean_hash);
        cdn_opts
            .params
            .insert("ssa_flag".to_string(), "is_fromtrack".to_string());
        cdn_opts
            .params
            .insert("version".to_string(), "11430".to_string());
        cdn_opts
            .params
            .insert("page_id".to_string(), "967177915".to_string());
        cdn_opts
            .params
            .insert("quality".to_string(), quality.to_string());
        cdn_opts
            .params
            .insert("album_audio_id".to_string(), audio_id.to_string());
        cdn_opts
            .params
            .insert("behavior".to_string(), "play".to_string());
        cdn_opts.params.insert("pid".to_string(), "411".to_string());
        cdn_opts.params.insert("cmd".to_string(), "26".to_string());
        cdn_opts
            .params
            .insert("pidversion".to_string(), "3001".to_string());
        cdn_opts
            .params
            .insert("IsFreePart".to_string(), "0".to_string());
        cdn_opts.params.insert(
            "ppage_id".to_string(),
            "356753938,823673182,967485191".to_string(),
        );
        cdn_opts
            .params
            .insert("cdnBackup".to_string(), "1".to_string());
        cdn_opts.params.insert("module".to_string(), String::new());
        cdn_opts
            .params
            .insert("clientver".to_string(), "11430".to_string());

        if let Ok(res) = self.request_json(cdn_opts).await {
            let urls = Self::extract_tracker_urls(&res);
            if !urls.is_empty() {
                return Ok(urls);
            }
        }

        Err(format!("音质 {quality} 暂无可用音源直链"))
    }

    fn is_supported_stream_url(url: &str) -> bool {
        if !url.starts_with("http") {
            return false;
        }
        let path_part = url.split('?').next().unwrap_or(url).to_ascii_lowercase();
        // 排除 Windows 原生 MediaPlayer 默认不支持的 .ogg 容器以及酷狗加密试听壳 .mp4
        !path_part.ends_with(".ogg") && !path_part.ends_with(".mp4")
    }

    fn extract_tracker_urls(res: &Value) -> Vec<String> {
        let mut urls = Vec::new();
        let target = res.get("data").unwrap_or(res);

        let ext_name = target
            .get("extName")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if ext_name == "ogg" || ext_name == "mp4" {
            return urls;
        }

        if let Some(arr) = target.get("url").and_then(|u| u.as_array()) {
            for item in arr {
                if let Some(s) = item.as_str() {
                    if Self::is_supported_stream_url(s) {
                        urls.push(s.to_string());
                    }
                }
            }
        } else if let Some(s) = target.get("url").and_then(|u| u.as_str()) {
            if Self::is_supported_stream_url(s) {
                urls.push(s.to_string());
            }
        }

        if let Some(arr) = target
            .get("backupUrl")
            .or_else(|| target.get("backup_url"))
            .and_then(|u| u.as_array())
        {
            for item in arr {
                if let Some(s) = item.as_str() {
                    if Self::is_supported_stream_url(s) && !urls.contains(&s.to_string()) {
                        urls.push(s.to_string());
                    }
                }
            }
        }

        urls
    }

    async fn search_lyric_candidates(
        &self,
        hash: &str,
        duration_ms: u64,
        keyword: &str,
        album_audio_id: i64,
        man: &str,
    ) -> Vec<Value> {
        let mut search_opts =
            RequestOptions::new(Method::GET, "https://lyrics.kugou.com", "/v1/search");
        search_opts.clear_default_params = true;
        search_opts.encrypt_type = EncryptType::Android;
        search_opts.use_cache = true;
        search_opts
            .params
            .insert("album_audio_id".to_string(), album_audio_id.to_string());
        search_opts
            .params
            .insert("appid".to_string(), LITE_APPID.to_string());
        search_opts
            .params
            .insert("clientver".to_string(), LITE_CLIENTVER.to_string());
        search_opts
            .params
            .insert("duration".to_string(), duration_ms.to_string());
        search_opts
            .params
            .insert("hash".to_string(), hash.to_string());
        search_opts
            .params
            .insert("keyword".to_string(), keyword.to_string());
        search_opts
            .params
            .insert("lrctxt".to_string(), "1".to_string());
        search_opts
            .params
            .insert("man".to_string(), man.to_string());

        let Ok(search_res) = self.request_json(search_opts).await else {
            return Vec::new();
        };

        search_res
            .get("candidates")
            .or_else(|| search_res.get("info"))
            .and_then(|c| c.as_array())
            .cloned()
            .unwrap_or_default()
    }

    /// 12. 歌词检索与 KRC/LRC 解密转换（双阶段检索：先 hash 精准匹配，无候选时自动以 keyword + man=yes 模糊回退）
    pub async fn get_song_lyric(
        &self,
        hash: &str,
        duration_ms: u64,
        keyword: Option<&str>,
        album_audio_id: Option<i64>,
    ) -> Result<String, String> {
        use base64::prelude::*;

        let clean_hash = hash.trim().to_lowercase();
        let kw = keyword.unwrap_or("").trim().to_string();
        let audio_id = album_audio_id.unwrap_or(0);

        // 第一阶段：使用 hash + album_audio_id 精准检索（keyword 置空避免干扰 hash 索引）
        let mut candidates = if !clean_hash.is_empty() {
            self.search_lyric_candidates(&clean_hash, duration_ms, "", audio_id, "no")
                .await
        } else {
            Vec::new()
        };

        // 第二阶段：若精准 hash 未命中且有歌名关键词，使用 keyword + man=yes 二次回退检索
        if candidates.is_empty() && !kw.is_empty() {
            candidates = self
                .search_lyric_candidates("", duration_ms, &kw, 0, "yes")
                .await;
        }

        if candidates.is_empty() {
            return Err("未找到匹配的歌词候选".to_string());
        }

        for cand in candidates.iter().take(3) {
            let id = cand
                .get("id")
                .map(|v| match v {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .unwrap_or_default();
            let accesskey = cand
                .get("accesskey")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if id.is_empty() || accesskey.is_empty() {
                continue;
            }

            for fmt in ["krc", "lrc"] {
                let mut dl_opts =
                    RequestOptions::new(Method::GET, "https://lyrics.kugou.com", "/download");
                dl_opts.clear_default_params = true;
                dl_opts.use_cache = true;
                dl_opts.params.insert("ver".to_string(), "1".to_string());
                dl_opts
                    .params
                    .insert("client".to_string(), "android".to_string());
                dl_opts.params.insert("id".to_string(), id.clone());
                dl_opts
                    .params
                    .insert("accesskey".to_string(), accesskey.to_string());
                dl_opts.params.insert("fmt".to_string(), fmt.to_string());
                dl_opts
                    .params
                    .insert("charset".to_string(), "utf8".to_string());

                if let Ok(dl_res) = self.request_json(dl_opts).await {
                    let content_type = dl_res
                        .get("contenttype")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                    if let Some(content) = dl_res.get("content").and_then(|v| v.as_str()) {
                        let trimmed_content = content.trim();
                        if trimmed_content.is_empty() {
                            continue;
                        }
                        if fmt == "lrc" || content_type != 0 {
                            if let Ok(bytes) = BASE64_STANDARD.decode(trimmed_content) {
                                let text = String::from_utf8_lossy(&bytes).to_string();
                                if text.contains('[') {
                                    return Ok(text);
                                }
                            }
                        } else if let Ok(decoded) = decode_krc_lyrics(trimmed_content) {
                            if decoded.contains('[') {
                                return Ok(decoded);
                            }
                        }
                    }
                }
            }
        }

        Err("歌词内容为空".to_string())
    }

    /// 13. 全曲库防抖取消搜索 (`/v2/search/song` / `/v1/search/album` / `/v1/search/{type}`)
    pub async fn search_music(
        &self,
        keyword: &str,
        page: u32,
        pagesize: u32,
        search_type: &str,
    ) -> Result<Value, String> {
        let cancel_token = self.next_search_token().await;
        let kind = match search_type {
            "song" | "album" | "author" | "special" | "lyric" => search_type,
            _ => "song",
        };

        let path = match kind {
            "song" => "/v2/search/song".to_string(),
            "album" => "/v1/search/album".to_string(),
            other => format!("/v1/search/{other}"),
        };

        let mut opts = RequestOptions::new(Method::GET, "https://gateway.kugou.com", &path);
        opts.cancel_token = Some(cancel_token);
        opts.use_cache = true;
        opts.headers
            .push(("x-router", "complexsearch.kugou.com".to_string()));
        opts.params
            .insert("keyword".to_string(), keyword.trim().to_string());
        opts.params.insert("page".to_string(), page.max(1).to_string());
        opts.params
            .insert("pagesize".to_string(), pagesize.clamp(1, 60).to_string());
        opts.params
            .insert("platform".to_string(), "AndroidFilter".to_string());
        opts.params
            .insert("iscorrection".to_string(), "1".to_string());

        if kind == "song" {
            opts.params
                .insert("privilegefilter".to_string(), "0".to_string());
            opts.params.insert("area_code".to_string(), "1".to_string());
            opts.params.insert("dopicfull".to_string(), "1".to_string());
            opts.params
                .insert("clientver".to_string(), "201".to_string());
        } else if kind == "album" {
            opts.params.insert("category".to_string(), "1".to_string());
            opts.params.insert("sorttype".to_string(), "0".to_string());
            opts.params
                .insert("searchsong".to_string(), "0".to_string());
            opts.params
                .insert("clientver".to_string(), "201".to_string());
        } else {
            opts.params.insert("albumhide".to_string(), "0".to_string());
            opts.params.insert("nocollect".to_string(), "0".to_string());
        }

        self.request_json(opts).await
    }

    /// 14. 每日推荐歌曲 (`/everyday_song_recommend`)；未登录或为空时回退至概念版私人专属好歌卡片 (`/youth/v1/song/single_card_recommend`)
    pub async fn get_daily_recommend(&self) -> Result<Value, String> {
        let (_, userid, _) = self.get_auth_pair().await;
        let uid_num = userid.parse::<u64>().unwrap_or(0);

        let mut opts = RequestOptions::new(
            Method::POST,
            "https://gateway.kugou.com",
            "/everyday_song_recommend",
        );
        opts.use_cache = true;
        opts.params
            .insert("platform".to_string(), "android".to_string());
        opts.headers
            .push(("x-router", "everydayrec.service.kugou.com".to_string()));
        opts.body = RequestBody::Json(json!({
            "platform": "android",
            "userid": uid_num
        }));

        if let Ok(res) = self.request_json(opts).await {
            let has_songs = res
                .get("data")
                .and_then(|d| d.get("song_list").or_else(|| d.get("list")))
                .and_then(|l| l.as_array())
                .map(|arr| !arr.is_empty())
                .unwrap_or(false);
            if has_songs {
                return Ok(res);
            }
        }

        log_warn("个性化每日推荐为空（可能未登录），自动回退至概念版私人专属好歌推荐卡片");
        let mut fallback = RequestOptions::new(
            Method::POST,
            "https://gateway.kugou.com",
            "/youth/v1/song/single_card_recommend",
        );
        fallback.use_cache = true;
        fallback
            .params
            .insert("card_id".to_string(), "3001".to_string());
        fallback
            .params
            .insert("area_code".to_string(), "1".to_string());
        fallback
            .params
            .insert("platform".to_string(), "ios".to_string());
        fallback
            .params
            .insert("module_id".to_string(), "1".to_string());
        fallback
            .params
            .insert("ver".to_string(), "v2".to_string());
        fallback
            .params
            .insert("pagesize".to_string(), "30".to_string());
        fallback
            .params
            .insert("clientver".to_string(), "12500".to_string());
        fallback.body = RequestBody::Json(json!({
            "tagid": "",
            "u_info": "",
            "source_mixsong": ""
        }));

        self.request_json(fallback).await
    }

    /// 16. 精选歌单广场推荐 (`/v2/special_recommend`)
    pub async fn get_top_playlists(
        &self,
        page: u32,
        pagesize: u32,
        refresh: bool,
    ) -> Result<Value, String> {
        if refresh {
            self.clear_cache().await;
        }
        let (_, userid, _) = self.get_auth_pair().await;
        let uid_num = userid.parse::<u64>().unwrap_or(0);
        let now_sec = Self::unix_secs().to_string();

        let mut opts = RequestOptions::new(
            Method::POST,
            "https://gateway.kugou.com",
            "/v2/special_recommend",
        );
        opts.use_cache = true;
        opts.headers
            .push(("x-router", "specialrec.service.kugou.com".to_string()));
        opts.body = RequestBody::Json(json!({
            "appid": LITE_APPID,
            "mid": self.mid(),
            "clientver": LITE_CLIENTVER,
            "platform": "android",
            "clienttime": now_sec,
            "userid": uid_num,
            "module_id": 1,
            "page": page.max(1),
            "pagesize": pagesize.clamp(1, 30),
            "key": sign_params_key(&now_sec),
            "special_recommend": {
                "withtag": 1,
                "withsong": 1,
                "sort": 1,
                "ugc": 1,
                "is_selected": 0,
                "withrecommend": 1,
                "area_code": 1,
                "categoryid": 0
            },
            "req_multi": 1,
            "retrun_min": 5,
            "return_special_falg": 1
        }));

        self.request_json(opts).await
    }

    /// 17. 私人 FM 电台推荐 (`/v2/personal_recommend`)
    pub async fn get_personal_fm(&self) -> Result<Value, String> {
        let (token, userid, _) = self.get_auth_pair().await;
        let uid_num = userid.parse::<u64>().unwrap_or(0);
        let now_ms = Self::unix_millis();

        let mut body = json!({
            "appid": LITE_APPID,
            "clienttime": now_ms,
            "mid": self.mid(),
            "action": "play",
            "recommend_source_locked": 0,
            "song_pool_id": 0,
            "callerid": 0,
            "m_type": 1,
            "platform": "ios",
            "area_code": 1,
            "remain_songcnt": 0,
            "clientver": LITE_CLIENTVER,
            "is_overplay": 0,
            "mode": "normal",
            "fakem": "ca981cfc583a4c37f28d2d49000013c16a0a",
            "key": sign_params_key(&now_ms.to_string())
        });

        if let Some(map) = body.as_object_mut() {
            if uid_num > 0 {
                map.insert("userid".to_string(), json!(uid_num));
                map.insert("kguid".to_string(), json!(uid_num));
            }
            if !token.is_empty() {
                map.insert("token".to_string(), json!(token));
            }
        }

        let mut opts = RequestOptions::new(
            Method::POST,
            "https://gateway.kugou.com",
            "/v2/personal_recommend",
        );
        opts.headers
            .push(("x-router", "persnfm.service.kugou.com".to_string()));
        opts.body = RequestBody::Json(body);

        self.request_json(opts).await
    }

    /// 18. 排行榜列表 (`/ocean/v6/rank/list`) 与榜单曲目 (`/openapi/kmr/v2/rank/audio`)
    pub async fn get_rankings(&self, refresh: bool) -> Result<Value, String> {
        if refresh {
            self.clear_cache().await;
        }
        let mut opts = RequestOptions::new(
            Method::GET,
            "https://gateway.kugou.com",
            "/ocean/v6/rank/list",
        );
        opts.use_cache = true;
        opts.params.insert("plat".to_string(), "2".to_string());
        opts.params.insert("withsong".to_string(), "1".to_string());
        opts.params.insert("parentid".to_string(), "0".to_string());
        self.request_json(opts).await
    }

    pub async fn get_ranking_songs(
        &self,
        rank_id: &str,
        page: u32,
        pagesize: u32,
    ) -> Result<Value, String> {
        let rank_num = rank_id.parse::<u64>().unwrap_or(8888);
        let mut opts = RequestOptions::new(
            Method::POST,
            "https://gateway.kugou.com",
            "/openapi/kmr/v2/rank/audio",
        );
        opts.use_cache = true;
        opts.headers.push(("kg-tid", "369".to_string()));
        opts.body = RequestBody::Json(json!({
            "show_portrait_mv": 1,
            "show_type_total": 1,
            "filter_original_remarks": 1,
            "area_code": 1,
            "pagesize": pagesize.clamp(1, 100),
            "rank_cid": 0,
            "type": 1,
            "page": page.max(1),
            "rank_id": rank_num
        }));

        self.request_json(opts).await
    }

    /// 19. 公共歌单详情与曲目分页 (`/v3/get_list_info` + `/pubsongs/v2/get_other_list_file_nofilt`)
    pub async fn get_playlist_detail(
        &self,
        global_collection_id: &str,
        page: u32,
        pagesize: u32,
    ) -> Result<Value, String> {
        let (token, userid, _) = self.get_auth_pair().await;
        let uid_num = userid.parse::<u64>().unwrap_or(0);

        let mut info_opts = RequestOptions::new(
            Method::POST,
            "https://gateway.kugou.com",
            "/v3/get_list_info",
        );
        info_opts.use_cache = true;
        info_opts
            .headers
            .push(("x-router", "pubsongs.kugou.com".to_string()));
        info_opts.body = RequestBody::Json(json!({
            "data": [{ "global_collection_id": global_collection_id }],
            "userid": uid_num,
            "token": token
        }));
        let info = self.request_json(info_opts).await.ok();

        let psize = pagesize.clamp(1, 100);
        let begin_idx = (page.max(1) - 1) * psize;
        let mut song_opts = RequestOptions::new(
            Method::GET,
            "https://gateway.kugou.com",
            "/pubsongs/v2/get_other_list_file_nofilt",
        );
        song_opts.use_cache = true;
        song_opts
            .params
            .insert("area_code".to_string(), "1".to_string());
        song_opts
            .params
            .insert("begin_idx".to_string(), begin_idx.to_string());
        song_opts.params.insert("plat".to_string(), "1".to_string());
        song_opts.params.insert("type".to_string(), "1".to_string());
        song_opts.params.insert("mode".to_string(), "1".to_string());
        song_opts
            .params
            .insert("personal_switch".to_string(), "1".to_string());
        song_opts.params.insert(
            "extend_fields".to_string(),
            "abtags,hot_cmt,popularization".to_string(),
        );
        song_opts
            .params
            .insert("pagesize".to_string(), psize.to_string());
        song_opts.params.insert(
            "global_collection_id".to_string(),
            global_collection_id.to_string(),
        );

        let songs = self.request_json(song_opts).await?;
        Ok(json!({
            "info": info,
            "songs": songs
        }))
    }

    /// 20. 用户自建/收藏云歌单与红心增删、歌手/专辑歌曲
    pub async fn get_user_playlists(&self, page: u32, pagesize: u32) -> Result<Value, String> {
        let (token, userid, _) = self.get_auth_pair().await;
        let uid_num = userid.parse::<u64>().unwrap_or(0);
        if token.is_empty() || uid_num == 0 {
            return Err("请先登录账号".to_string());
        }

        let mut opts = RequestOptions::new(
            Method::POST,
            "https://gateway.kugou.com",
            "/v7/get_all_list",
        );
        opts.headers
            .push(("x-router", "cloudlist.service.kugou.com".to_string()));
        opts.params.insert("plat".to_string(), "1".to_string());
        opts.params
            .insert("userid".to_string(), uid_num.to_string());
        opts.params.insert("token".to_string(), token.clone());
        opts.body = RequestBody::Json(json!({
            "userid": uid_num,
            "token": token,
            "total_ver": 979,
            "type": 2,
            "page": page.max(1),
            "pagesize": pagesize.clamp(1, 50)
        }));

        self.request_json(opts).await
    }

    pub async fn get_user_playlist_tracks(
        &self,
        listid: i64,
        page: u32,
        pagesize: u32,
    ) -> Result<Value, String> {
        let (token, userid, _) = self.get_auth_pair().await;
        let mut opts = RequestOptions::new(
            Method::POST,
            "https://gateway.kugou.com",
            "/v4/get_list_all_file_v3",
        );
        opts.headers
            .push(("x-router", "cloudlist.service.kugou.com".to_string()));
        opts.body = RequestBody::Json(json!({
            "listid": listid,
            "userid": userid,
            "type": 0,
            "page": page.max(1),
            "pagesize": pagesize.clamp(1, 300),
            "area_code": 1,
            "allplatform": 1,
            "show_cover": 1,
            "token": token
        }));
        self.request_json(opts).await
    }

    pub async fn playlist_add_song(
        &self,
        listid: i64,
        song_name: &str,
        hash: &str,
        album_id: Option<&str>,
        album_audio_id: Option<i64>,
    ) -> Result<Value, String> {
        let (token, userid, _) = self.get_auth_pair().await;
        let uid_num = userid.parse::<u64>().unwrap_or(0);
        if token.is_empty() || uid_num == 0 {
            return Err("请先登录后再收藏歌曲".to_string());
        }

        let now_sec = Self::unix_secs();
        let alb_num = album_id.and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
        let mix_id = album_audio_id.unwrap_or(0);

        let mut opts = RequestOptions::new(
            Method::POST,
            "https://gateway.kugou.com",
            "/cloudlist.service/v6/add_song",
        );
        opts.params
            .insert("last_time".to_string(), now_sec.to_string());
        opts.params
            .insert("last_area".to_string(), "gztx".to_string());
        opts.params
            .insert("userid".to_string(), uid_num.to_string());
        opts.params.insert("token".to_string(), token.clone());
        opts.body = RequestBody::Json(json!({
            "userid": uid_num,
            "token": token,
            "listid": listid,
            "list_ver": 0,
            "type": 0,
            "slow_upload": 1,
            "scene": "false;null",
            "data": [{
                "number": 1,
                "name": song_name,
                "hash": hash,
                "size": 0,
                "sort": 0,
                "timelen": 0,
                "bitrate": 0,
                "album_id": alb_num,
                "mixsongid": mix_id
            }]
        }));
        self.request_json(opts).await
    }

    pub async fn playlist_del_song(&self, listid: i64, fileid: i64) -> Result<Value, String> {
        let (token, userid, _) = self.get_auth_pair().await;
        let uid_num = userid.parse::<u64>().unwrap_or(0);

        let mut opts = RequestOptions::new(
            Method::POST,
            "https://gateway.kugou.com",
            "/v4/delete_songs",
        );
        opts.headers
            .push(("x-router", "cloudlist.service.kugou.com".to_string()));
        opts.body = RequestBody::Json(json!({
            "listid": listid,
            "userid": uid_num,
            "data": [{ "fileid": fileid }],
            "type": 0,
            "token": token,
            "list_ver": 0
        }));
        self.request_json(opts).await
    }

    pub async fn get_artist_songs(
        &self,
        author_id: &str,
        page: u32,
        pagesize: u32,
    ) -> Result<Value, String> {
        let mut opts = RequestOptions::new(
            Method::GET,
            "https://gateway.kugou.com",
            "/openapi/kmr/v2/audio_group/author",
        );
        opts.use_cache = true;
        opts.headers.push(("kg-tid", "36".to_string()));
        opts.params
            .insert("author_id".to_string(), author_id.to_string());
        opts.params
            .insert("area_code".to_string(), "all".to_string());
        opts.params.insert("sort".to_string(), "1".to_string());
        opts.params.insert("page".to_string(), page.max(1).to_string());
        opts.params
            .insert("pagesize".to_string(), pagesize.clamp(1, 100).to_string());
        opts.params
            .insert("replace_api_version".to_string(), "1".to_string());
        opts.params
            .insert("mvdata_need".to_string(), "1".to_string());
        opts.params
            .insert("show_audio_honor".to_string(), "1".to_string());
        opts.params
            .insert("show_audio_tag".to_string(), "1".to_string());
        opts.params
            .insert("replace_need".to_string(), "1".to_string());
        self.request_json(opts).await
    }

    pub async fn get_album_songs(
        &self,
        album_id: &str,
        page: u32,
        pagesize: u32,
    ) -> Result<Value, String> {
        let mut opts = RequestOptions::new(
            Method::POST,
            "https://gateway.kugou.com",
            "/v1/album_audio/lite",
        );
        opts.use_cache = true;
        opts.headers
            .push(("x-router", "openapi.kugou.com".to_string()));
        opts.headers.push(("kg-tid", "255".to_string()));
        opts.body = RequestBody::Json(json!({
            "album_id": album_id,
            "is_buy": "",
            "page": page.max(1),
            "pagesize": pagesize.clamp(1, 50)
        }));
        self.request_json(opts).await
    }

    /// 搜索实时联想建议 (`/v2/getSearchTip`)
    pub async fn get_search_suggest(&self, keyword: &str) -> Result<Value, String> {
        let kw = keyword.trim();
        if kw.is_empty() {
            return Ok(json!({ "data": [] }));
        }
        let mut opts = RequestOptions::new(Method::GET, "https://gateway.kugou.com", "/v2/getSearchTip");
        opts.use_cache = true;
        opts.headers
            .push(("x-router", "searchtip.kugou.com".to_string()));
        opts.params.insert("keyword".to_string(), kw.to_string());
        opts.params.insert("AlbumTipCount".to_string(), "4".to_string());
        opts.params.insert("CorrectTipCount".to_string(), "2".to_string());
        opts.params.insert("MVTipCount".to_string(), "0".to_string());
        opts.params.insert("MusicTipCount".to_string(), "8".to_string());
        opts.params.insert("radiotip".to_string(), "1".to_string());
        self.request_json(opts).await
    }

    /// 新建个人云歌单 (`list_type=0`) 或收藏外部歌单 (`list_type=1`) (`/cloudlist.service/v5/add_list`)
    pub async fn create_or_collect_playlist(
        &self,
        name: &str,
        list_type: u32,
        list_create_userid: Option<u64>,
        list_create_listid: Option<i64>,
        list_create_gid: Option<&str>,
    ) -> Result<Value, String> {
        let (token, userid, _) = self.get_auth_pair().await;
        let uid_num = userid.parse::<u64>().unwrap_or(0);
        if token.is_empty() || uid_num == 0 {
            return Err("请先登录账号".to_string());
        }
        let now_sec = Self::unix_secs();

        let mut opts = RequestOptions::new(
            Method::POST,
            "https://gateway.kugou.com",
            "/cloudlist.service/v5/add_list",
        );
        if list_type == 0 {
            opts.params
                .insert("last_time".to_string(), now_sec.to_string());
            opts.params
                .insert("last_area".to_string(), "gztx".to_string());
            opts.params
                .insert("userid".to_string(), uid_num.to_string());
            opts.params.insert("token".to_string(), token.clone());
        }
        opts.body = RequestBody::Json(json!({
            "userid": uid_num,
            "token": token,
            "total_ver": 0,
            "name": name.trim(),
            "type": list_type,
            "source": 1,
            "is_pri": 0,
            "list_create_userid": list_create_userid.unwrap_or(uid_num),
            "list_create_listid": list_create_listid.unwrap_or(0),
            "list_create_gid": list_create_gid.unwrap_or(""),
            "from_shupinmv": 0
        }));
        self.request_json(opts).await
    }

    /// 删除自建歌单或取消收藏歌单 (`/v2/delete_list`)
    pub async fn delete_playlist(&self, listid: i64) -> Result<Value, String> {
        let (token, userid, _) = self.get_auth_pair().await;
        let uid_num = userid.parse::<u64>().unwrap_or(0);
        if token.is_empty() || uid_num == 0 {
            return Err("请先登录账号".to_string());
        }
        let now_sec = Self::unix_secs();

        let payload = json!({
            "listid": listid,
            "total_ver": 0,
            "type": 1
        })
        .to_string();
        let (aes_b64, aes_key6) = playlist_aes_encrypt(&payload)?;
        let p_plain = json!({
            "aes": aes_key6,
            "uid": uid_num,
            "token": token
        })
        .to_string();
        let p_hex = rsa_encrypt_pkcs1_v15_lite(&p_plain)?.to_uppercase();

        let mut opts = RequestOptions::new(Method::POST, "https://gateway.kugou.com", "/v2/delete_list");
        opts.headers
            .push(("x-router", "cloudlist.service.kugou.com".to_string()));
        opts.params
            .insert("clienttime".to_string(), now_sec.to_string());
        opts.params
            .insert("key".to_string(), sign_params_key(&now_sec.to_string()));
        opts.params
            .insert("last_area".to_string(), "gztx".to_string());
        opts.params
            .insert("last_time".to_string(), now_sec.to_string());
        opts.params.insert("p".to_string(), p_hex);
        opts.body = RequestBody::RawText(aes_b64);

        let raw_bytes = self.request_bytes(opts).await?;
        let text = playlist_aes_decrypt_bytes(&raw_bytes, &aes_key6)
            .unwrap_or_else(|_| String::from_utf8_lossy(&raw_bytes).to_string());
        Ok(serde_json::from_str(&text).unwrap_or_else(|_| json!({ "status": 1, "raw": text })))
    }

    /// 查询概念版 VIP 详情状态 (`/v1/get_union_vip`)
    pub async fn get_vip_detail(&self) -> Result<Value, String> {
        let mut opts = RequestOptions::new(
            Method::GET,
            "https://kugouvip.kugou.com",
            "/v1/get_union_vip",
        );
        opts.params
            .insert("busi_type".to_string(), "concept".to_string());
        self.request_json(opts).await
    }

    /// 听歌历史云端上报 (`/playhistory/v1/upload_songs`)
    pub async fn upload_play_history(&self, mxid: i64) -> Result<Value, String> {
        let (token, userid, _) = self.get_auth_pair().await;
        let uid_num = userid.parse::<u64>().unwrap_or(0);
        if token.is_empty() || uid_num == 0 || mxid <= 0 {
            return Ok(json!({ "skipped": true }));
        }
        let now_sec = Self::unix_secs();
        let mut opts = RequestOptions::new(
            Method::POST,
            "https://gateway.kugou.com",
            "/playhistory/v1/upload_songs",
        );
        opts.params.insert("plat".to_string(), "3".to_string());
        opts.body = RequestBody::Json(json!({
            "songs": [{ "mxid": mxid, "op": 1, "ot": now_sec, "pc": 1 }],
            "token": token,
            "userid": uid_num
        }));
        self.request_json(opts).await
    }

    /// 歌曲元数据与 `{size}` 缩放封面兜底查询 (`/v1/audio/audio`)
    pub async fn get_audio_metadata(&self, hash: &str) -> Result<Option<String>, String> {
        let clean_hash = hash.trim().to_lowercase();
        if clean_hash.is_empty() {
            return Ok(None);
        }
        let (token, userid, _) = self.get_auth_pair().await;
        let uid_num = userid.parse::<u64>().unwrap_or(0);
        let dfid = self.get_dfid().await;
        let now_ms = Self::unix_millis();

        let mut body = json!({
            "appid": LITE_APPID,
            "clienttime": now_ms,
            "clientver": LITE_CLIENTVER,
            "data": [{ "hash": clean_hash, "audio_id": 0 }],
            "dfid": dfid,
            "key": sign_params_key(&now_ms.to_string()),
            "mid": self.mid()
        });
        if let Some(map) = body.as_object_mut() {
            if !token.is_empty() {
                map.insert("token".to_string(), json!(token));
            }
            if uid_num > 0 {
                map.insert("userid".to_string(), json!(uid_num));
            }
        }

        let mut opts = RequestOptions::new(
            Method::POST,
            "http://kmr.service.kugou.com",
            "/v1/audio/audio",
        );
        opts.use_cache = true;
        opts.headers
            .push(("x-router", "kmr.service.kugou.com".to_string()));
        opts.body = RequestBody::Json(body);

        let res = self.request_json(opts).await?;
        let cover = res
            .get("data")
            .and_then(|d| d.as_array())
            .and_then(|arr| arr.first())
            .and_then(|first| {
                if let Some(sub) = first.as_array() {
                    sub.first()
                } else {
                    Some(first)
                }
            })
            .and_then(|item| {
                item.get("trans_param")
                    .and_then(|tp| tp.get("union_cover"))
                    .or_else(|| item.get("album_img"))
                    .or_else(|| item.get("sizable_cover"))
                    .and_then(|v| v.as_str())
            })
            .filter(|s| !s.is_empty())
            .map(|s| s.replace("{size}", "100").replace("http://", "https://"));

        Ok(cover)
    }
}
