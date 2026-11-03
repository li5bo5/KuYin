use std::sync::OnceLock;
use tokio::sync::mpsc::UnboundedSender;

use crate::storage::{log_info, log_warn};

#[cfg(windows)]
use windows::core::IInspectable;
#[cfg(windows)]
use windows::Devices::Enumeration::{DeviceInformation, DeviceInformationUpdate, DeviceWatcher};
#[cfg(windows)]
use windows::Foundation::TypedEventHandler;
#[cfg(windows)]
use windows::Media::Devices::{
    AudioDeviceRole, DefaultAudioRenderDeviceChangedEventArgs, MediaDevice,
};
#[cfg(windows)]
use windows::Media::Playback::MediaPlayer;

#[cfg(windows)]
static DEVICE_WATCHER: OnceLock<DeviceWatcher> = OnceLock::new();

#[derive(Clone, Debug)]
pub enum DeviceWatchEvent {
    Added,
    Removed(String),
    Updated,
    DefaultChanged,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AudioDeviceInfo {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

pub struct DeviceManager;

impl DeviceManager {
    /// 启动系统原生音频渲染设备插拔与状态监听（WinRT DeviceWatcher + DefaultAudioRenderDeviceChanged）
    pub fn start_device_watcher(tx: UnboundedSender<DeviceWatchEvent>) {
        #[cfg(windows)]
        {
            let tx_default = tx.clone();
            let _ = MediaDevice::DefaultAudioRenderDeviceChanged(&TypedEventHandler::<
                IInspectable,
                DefaultAudioRenderDeviceChangedEventArgs,
            >::new(
                move |_sender, _args| {
                    let _ = tx_default.send(DeviceWatchEvent::DefaultChanged);
                    Ok(())
                },
            ));

            if let Ok(selector) = MediaDevice::GetAudioRenderSelector() {
                if let Ok(watcher) = DeviceInformation::CreateWatcherAqsFilter(&selector) {
                    let tx_added = tx.clone();
                    let _ = watcher.Added(&TypedEventHandler::<
                        DeviceWatcher,
                        DeviceInformation,
                    >::new(move |_w, _info| {
                        let _ = tx_added.send(DeviceWatchEvent::Added);
                        Ok(())
                    }));

                    let tx_removed = tx.clone();
                    let _ = watcher.Removed(&TypedEventHandler::<
                        DeviceWatcher,
                        DeviceInformationUpdate,
                    >::new(move |_w, upd| {
                        let id = upd
                            .as_ref()
                            .and_then(|u| u.Id().ok())
                            .map(|h| h.to_string())
                            .unwrap_or_default();
                        let _ = tx_removed.send(DeviceWatchEvent::Removed(id));
                        Ok(())
                    }));

                    let tx_updated = tx;
                    let _ = watcher.Updated(&TypedEventHandler::<
                        DeviceWatcher,
                        DeviceInformationUpdate,
                    >::new(move |_w, _upd| {
                        let _ = tx_updated.send(DeviceWatchEvent::Updated);
                        Ok(())
                    }));

                    if watcher.Start().is_ok() {
                        let _ = DEVICE_WATCHER.set(watcher);
                    }
                }
            }
        }
        #[cfg(not(windows))]
        {
            let _ = tx;
        }
    }

    /// 枚举当前系统所有已启用且在线的音频输出渲染设备
    pub async fn list_output_devices() -> Result<Vec<AudioDeviceInfo>, String> {
        #[cfg(windows)]
        {
            let selector = MediaDevice::GetAudioRenderSelector()
                .map_err(|e| format!("获取音频选择器失败: {e:?}"))?;

            let async_op = DeviceInformation::FindAllAsyncAqsFilter(&selector)
                .map_err(|e| format!("查询设备列表失败: {e:?}"))?;
            let devices = async_op
                .get()
                .map_err(|e| format!("等待设备列表完成失败: {e:?}"))?;

            let default_id = MediaDevice::GetDefaultAudioRenderId(AudioDeviceRole::Default)
                .map(|h| h.to_string())
                .unwrap_or_default();

            let mut list = Vec::new();
            for i in 0..devices.Size().unwrap_or(0) {
                if let Ok(dev) = devices.GetAt(i) {
                    if !dev.IsEnabled().unwrap_or(true) {
                        continue;
                    }
                    let id = dev.Id().map(|h| h.to_string()).unwrap_or_default();
                    if id.is_empty() {
                        continue;
                    }
                    let name = dev
                        .Name()
                        .map(|h| h.to_string())
                        .unwrap_or_else(|_| "未知音频设备".to_string());
                    let is_default = id == default_id;

                    list.push(AudioDeviceInfo {
                        id,
                        name,
                        is_default,
                    });
                }
            }
            Ok(list)
        }
        #[cfg(not(windows))]
        {
            Ok(vec![])
        }
    }

    /// 精确检测指定音频输出设备当前是否在线并处于启用状态
    pub async fn is_device_online(device_id: &str) -> bool {
        #[cfg(windows)]
        {
            let trimmed = device_id.trim();
            if trimmed.is_empty() {
                return false;
            }
            let h_id = windows::core::HSTRING::from(trimmed);
            if let Ok(op) = DeviceInformation::CreateFromIdAsync(&h_id) {
                if let Ok(info) = op.get() {
                    if !info.IsEnabled().unwrap_or(false) {
                        return false;
                    }
                } else {
                    return false;
                }
            }
            if let Ok(list) = Self::list_output_devices().await {
                return list.iter().any(|d| d.id.eq_ignore_ascii_case(trimmed));
            }
            false
        }
        #[cfg(not(windows))]
        {
            let _ = device_id;
            false
        }
    }

    /// 热切换播放器输出设备；若设备失效或传入 None 自动回退系统默认设备
    pub async fn set_audio_device(
        #[cfg(windows)] player: &MediaPlayer,
        device_id: Option<String>,
    ) -> Result<(), String> {
        #[cfg(windows)]
        {
            match device_id {
                Some(id) if !id.is_empty() => {
                    let h_id = windows::core::HSTRING::from(id.as_str());
                    let async_op = DeviceInformation::CreateFromIdAsync(&h_id)
                        .map_err(|e| format!("获取设备句柄失败: {e:?}"))?;

                    match async_op.get() {
                        Ok(dev_info) => {
                            player
                                .SetAudioDevice(&dev_info)
                                .map_err(|e| format!("绑定音频设备失败: {e:?}"))?;
                            log_info(&format!("已切换音频输出设备: {id}"));
                        }
                        Err(e) => {
                            log_warn(&format!(
                                "指定音频设备无效或已拔出，自动回退系统默认设备: {e:?}"
                            ));
                            player
                                .SetAudioDevice(None)
                                .map_err(|e| format!("回退默认设备失败: {e:?}"))?;
                        }
                    }
                }
                _ => {
                    player
                        .SetAudioDevice(None)
                        .map_err(|e| format!("设置默认音频设备失败: {e:?}"))?;
                    log_info("音频输出设备已设为跟随系统默认");
                }
            }
            Ok(())
        }
        #[cfg(not(windows))]
        {
            let _ = device_id;
            Ok(())
        }
    }
}
