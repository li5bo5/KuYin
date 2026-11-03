use tokio::sync::mpsc::UnboundedSender;

use crate::storage::{log_info, log_warn};

#[cfg(windows)]
use windows::core::{HSTRING, Interface, PCWSTR};
#[cfg(windows)]
use windows::Foundation::{TypedEventHandler, Uri};
#[cfg(windows)]
use windows::Media::Playback::MediaPlayer;
#[cfg(windows)]
use windows::Media::{
    MediaPlaybackStatus, MediaPlaybackType, SystemMediaTransportControls,
    SystemMediaTransportControlsButton, SystemMediaTransportControlsButtonPressedEventArgs,
};
#[cfg(windows)]
use windows::Storage::Streams::RandomAccessStreamReference;
#[cfg(windows)]
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED,
};
#[cfg(windows)]
use windows::Win32::UI::Shell::{
    IShellLinkW, SetCurrentProcessExplicitAppUserModelID, ShellLink,
};

pub const APP_USER_MODEL_ID: &str = "com.kuyin.music";

#[cfg(windows)]
mod prop_store {
    use core::ffi::c_void;
    use windows::core::GUID;

    const IID_IPROPERTYSTORE: GUID = GUID::from_u128(0x886d8eeb_8cf2_4446_8d02_cdba1dbdcf99);

    #[repr(C)]
    struct PropertyKey {
        fmtid: GUID,
        pid: u32,
    }

    // System.AppUserModel.ID ({9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3}, 5)
    const PKEY_APPUSERMODEL_ID: PropertyKey = PropertyKey {
        fmtid: GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3),
        pid: 5,
    };

    #[repr(C)]
    struct RawPropVariantWs {
        vt: u16,
        w_reserved1: u16,
        w_reserved2: u16,
        w_reserved3: u16,
        pwsz_val: *const u16,
        _pad: usize,
    }

    pub unsafe fn set_shortcut_aumid(shell_link_raw: *mut c_void, aumid_wide: *const u16) {
        if shell_link_raw.is_null() || aumid_wide.is_null() {
            return;
        }
        let vtbl = *(shell_link_raw as *const *const usize);
        let qi: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> i32 =
            core::mem::transmute(*vtbl.add(0));

        let mut store_ptr: *mut c_void = core::ptr::null_mut();
        if qi(shell_link_raw, &IID_IPROPERTYSTORE, &mut store_ptr) >= 0 && !store_ptr.is_null() {
            let store_vtbl = *(store_ptr as *const *const usize);
            // IPropertyStore::SetValue 位于槽位 6，Commit 位于槽位 7
            let set_value_fn: unsafe extern "system" fn(
                *mut c_void,
                *const PropertyKey,
                *const RawPropVariantWs,
            ) -> i32 = core::mem::transmute(*store_vtbl.add(6));
            let commit_fn: unsafe extern "system" fn(*mut c_void) -> i32 =
                core::mem::transmute(*store_vtbl.add(7));
            let release_fn: unsafe extern "system" fn(*mut c_void) -> u32 =
                core::mem::transmute(*store_vtbl.add(2));

            let propvar = RawPropVariantWs {
                vt: 31, // VT_LPWSTR
                w_reserved1: 0,
                w_reserved2: 0,
                w_reserved3: 0,
                pwsz_val: aumid_wide,
                _pad: 0,
            };
            let _ = set_value_fn(store_ptr, &PKEY_APPUSERMODEL_ID, &propvar);
            let _ = commit_fn(store_ptr);
            let _ = release_fn(store_ptr);
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum SmtcAction {
    Play,
    Pause,
    Next,
    Previous,
}

pub struct SmtcManager;

impl SmtcManager {
    /// 在进程启动首行（初始化 MediaPlayer 之前）锁定进程级 AppUserModelID，
    /// 并静默确保开始菜单存在绑定了相同 AUMID 的 `.lnk` 快捷方式，解决便携版 Win+A 显示“未知应用”问题
    pub fn ensure_app_identity() {
        #[cfg(windows)]
        unsafe {
            let aumid_h = HSTRING::from(APP_USER_MODEL_ID);
            let _ = SetCurrentProcessExplicitAppUserModelID(PCWSTR(aumid_h.as_ptr()));

            let Ok(exe_path) = std::env::current_exe() else {
                return;
            };
            let Some(appdata) = std::env::var_os("APPDATA") else {
                return;
            };
            let programs_dir = std::path::PathBuf::from(appdata)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs");
            if std::fs::create_dir_all(&programs_dir).is_err() {
                return;
            }
            let lnk_path = programs_dir.join("酷音 KuYin.lnk");

            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let res: Result<(), String> = (|| {
                let shell_link: IShellLinkW =
                    CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
                        .map_err(|e| format!("创建 IShellLinkW 失败: {e:?}"))?;

                let exe_str = exe_path.to_string_lossy();
                let exe_h = HSTRING::from(exe_str.as_ref());
                let _ = shell_link.SetPath(PCWSTR(exe_h.as_ptr()));
                let _ = shell_link.SetIconLocation(PCWSTR(exe_h.as_ptr()), 0);
                if let Some(parent) = exe_path.parent() {
                    let dir_h = HSTRING::from(parent.to_string_lossy().as_ref());
                    let _ = shell_link.SetWorkingDirectory(PCWSTR(dir_h.as_ptr()));
                }
                let desc_h = HSTRING::from("酷音 KuYin");
                let _ = shell_link.SetDescription(PCWSTR(desc_h.as_ptr()));

                prop_store::set_shortcut_aumid(shell_link.as_raw(), aumid_h.as_ptr());

                let persist_file: IPersistFile = shell_link
                    .cast()
                    .map_err(|e| format!("转换 IPersistFile 失败: {e:?}"))?;
                let lnk_h = HSTRING::from(lnk_path.to_string_lossy().as_ref());
                persist_file
                    .Save(PCWSTR(lnk_h.as_ptr()), true)
                    .map_err(|e| format!("保存开始菜单快捷方式失败: {e:?}"))?;
                Ok(())
            })();
            if let Err(err) = res {
                log_warn(&format!("同步 SMTC 开始菜单快捷方式标识失败: {err}"));
            }
            CoUninitialize();
        }
    }

    /// 启用原生 Windows SMTC 并双向绑定硬件媒体键/蓝牙耳机控制事件
    #[cfg(windows)]
    pub fn setup_smtc(
        player: &MediaPlayer,
        tx: UnboundedSender<SmtcAction>,
    ) -> Result<(), String> {
        let smtc = player
            .SystemMediaTransportControls()
            .map_err(|e| format!("获取原生 SMTC 失败: {e:?}"))?;

        smtc.SetIsEnabled(true)
            .map_err(|e| format!("启用 SMTC 失败: {e:?}"))?;
        smtc.SetIsPlayEnabled(true)
            .map_err(|e| format!("开启 Play 失败: {e:?}"))?;
        smtc.SetIsPauseEnabled(true)
            .map_err(|e| format!("开启 Pause 失败: {e:?}"))?;
        smtc.SetIsNextEnabled(true)
            .map_err(|e| format!("开启 Next 失败: {e:?}"))?;
        smtc.SetIsPreviousEnabled(true)
            .map_err(|e| format!("开启 Previous 失败: {e:?}"))?;
        let _ = smtc.SetPlaybackStatus(MediaPlaybackStatus::Stopped);

        if let Ok(updater) = smtc.DisplayUpdater() {
            let _ = updater.SetAppMediaId(&HSTRING::from(APP_USER_MODEL_ID));
            let _ = updater.SetType(MediaPlaybackType::Music);
            let _ = updater.Update();
        }

        smtc.ButtonPressed(&TypedEventHandler::<
            SystemMediaTransportControls,
            SystemMediaTransportControlsButtonPressedEventArgs,
        >::new(move |_sender, args| {
            if let Some(a) = args.as_ref() {
                if let Ok(btn) = a.Button() {
                    if btn == SystemMediaTransportControlsButton::Play {
                        let _ = tx.send(SmtcAction::Play);
                    } else if btn == SystemMediaTransportControlsButton::Pause {
                        let _ = tx.send(SmtcAction::Pause);
                    } else if btn == SystemMediaTransportControlsButton::Next {
                        let _ = tx.send(SmtcAction::Next);
                    } else if btn == SystemMediaTransportControlsButton::Previous {
                        let _ = tx.send(SmtcAction::Previous);
                    }
                }
            }
            Ok(())
        }))
        .map_err(|e| format!("绑定 SMTC ButtonPressed 失败: {e:?}"))?;

        log_info("原生 Windows SMTC 系统媒体传输控件双向绑定就绪");
        Ok(())
    }

    /// 同步 SMTC 播放/暂停状态
    #[cfg(windows)]
    pub fn update_playback_status(player: &MediaPlayer, is_playing: bool) {
        if let Ok(smtc) = player.SystemMediaTransportControls() {
            let status = if is_playing {
                MediaPlaybackStatus::Playing
            } else {
                MediaPlaybackStatus::Paused
            };
            let _ = smtc.SetPlaybackStatus(status);
        }
    }

    /// 更新 SMTC 媒体卡片展示元数据（歌名、歌手、240px 缩略图封面）
    #[cfg(windows)]
    pub fn update_metadata(
        player: &MediaPlayer,
        title: &str,
        artist: &str,
        cover_url: Option<&str>,
    ) -> Result<(), String> {
        let smtc = player
            .SystemMediaTransportControls()
            .map_err(|e| format!("获取 SMTC 失败: {e:?}"))?;

        let updater = smtc
            .DisplayUpdater()
            .map_err(|e| format!("获取 DisplayUpdater 失败: {e:?}"))?;

        let _ = updater.SetAppMediaId(&HSTRING::from(APP_USER_MODEL_ID));
        updater
            .SetType(MediaPlaybackType::Music)
            .map_err(|e| format!("设置 SMTC 类型失败: {e:?}"))?;

        let music_prop = updater
            .MusicProperties()
            .map_err(|e| format!("获取 MusicProperties 失败: {e:?}"))?;

        music_prop
            .SetTitle(&HSTRING::from(title))
            .map_err(|e| format!("设置标题失败: {e:?}"))?;
        music_prop
            .SetArtist(&HSTRING::from(artist))
            .map_err(|e| format!("设置歌手失败: {e:?}"))?;

        if let Some(raw_cover) = cover_url {
            let safe_cover = raw_cover.replace("{size}", "240");
            if safe_cover.starts_with("http") {
                if let Ok(uri) = Uri::CreateUri(&HSTRING::from(safe_cover)) {
                    if let Ok(stream_ref) = RandomAccessStreamReference::CreateFromUri(&uri) {
                        let _ = updater.SetThumbnail(&stream_ref);
                    }
                }
            }
        }

        updater
            .Update()
            .map_err(|e| format!("刷新 SMTC 浮层失败: {e:?}"))?;
        Ok(())
    }
}
