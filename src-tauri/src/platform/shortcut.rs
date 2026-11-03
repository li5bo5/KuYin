use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::storage::{log_info, log_warn, Database};
use crate::AppState;

use super::{TrayManager, WindowManager};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ShortcutConfig {
    pub play_pause: String,
    pub next: String,
    pub prev: String,
    pub vol_up: String,
    pub vol_down: String,
    pub toggle_window: String,
}

impl Default for ShortcutConfig {
    fn default() -> Self {
        Self {
            play_pause: "Ctrl+Alt+Space".to_string(),
            next: "Ctrl+Alt+Right".to_string(),
            prev: "Ctrl+Alt+Left".to_string(),
            vol_up: "Ctrl+Alt+Up".to_string(),
            vol_down: "Ctrl+Alt+Down".to_string(),
            toggle_window: "Ctrl+Alt+M".to_string(),
        }
    }
}

pub struct ShortcutManager;

impl ShortcutManager {
    pub fn load_config(db: &Database) -> ShortcutConfig {
        if let Some(raw) = db.get_setting("global_shortcuts") {
            if let Ok(cfg) = serde_json::from_str::<ShortcutConfig>(&raw) {
                return cfg;
            }
        }
        ShortcutConfig::default()
    }

    /// 注册或热更新全局快捷键，自动捕获热键冲突并返回冲突列表供界面提示
    pub fn apply_shortcuts(app: &AppHandle, cfg: &ShortcutConfig) -> Result<Vec<String>, String> {
        let gs = app.global_shortcut();
        let _ = gs.unregister_all();

        let entries = [
            ("play_pause", cfg.play_pause.as_str()),
            ("next", cfg.next.as_str()),
            ("prev", cfg.prev.as_str()),
            ("vol_up", cfg.vol_up.as_str()),
            ("vol_down", cfg.vol_down.as_str()),
            ("toggle_window", cfg.toggle_window.as_str()),
        ];

        let mut conflicts = Vec::new();
        for (action, key_str) in entries {
            let trimmed = key_str.trim();
            if trimmed.is_empty() {
                continue;
            }
            let Ok(shortcut) = trimmed.parse::<Shortcut>() else {
                conflicts.push(format!("{trimmed} (格式无效)"));
                continue;
            };

            let action_owned = action.to_string();
            let reg_res = gs.on_shortcut(shortcut, move |app_handle, _sc, event| {
                if event.state != ShortcutState::Pressed {
                    return;
                }
                let act = action_owned.clone();
                let h = app_handle.clone();
                match act.as_str() {
                    "toggle_window" => {
                        if let Some(win) = h.get_webview_window("main") {
                            if win.is_visible().unwrap_or(false)
                                && !win.is_minimized().unwrap_or(false)
                            {
                                let state = h.state::<AppState>();
                                WindowManager::save_window_bounds(&win, &state.db);
                                let _ = win.destroy();
                                return;
                            }
                        }
                        TrayManager::restore_or_create_window(&h);
                    }
                    _ => {
                        tauri::async_runtime::spawn(async move {
                            let state = h.state::<AppState>();
                            match act.as_str() {
                                "play_pause" => {
                                    let snap = state.coordinator.get_snapshot().await;
                                    if snap.is_playing {
                                        let _ = state.coordinator.pause().await;
                                    } else {
                                        let _ = state.coordinator.resume_or_play().await;
                                    }
                                }
                                "next" => {
                                    let _ = state.coordinator.next().await;
                                }
                                "prev" => {
                                    let _ = state.coordinator.previous().await;
                                }
                                "vol_up" => {
                                    let snap = state.coordinator.get_snapshot().await;
                                    let _ = state.coordinator.set_volume(snap.volume + 0.05).await;
                                }
                                "vol_down" => {
                                    let snap = state.coordinator.get_snapshot().await;
                                    let _ = state.coordinator.set_volume(snap.volume - 0.05).await;
                                }
                                _ => {}
                            }
                        });
                    }
                }
            });

            if let Err(e) = reg_res {
                log_warn(&format!("全局热键 {trimmed} 注册冲突或失败: {e:?}"));
                conflicts.push(trimmed.to_string());
            }
        }

        log_info(&format!(
            "全局快捷键注册完成 (冲突数: {})",
            conflicts.len()
        ));
        Ok(conflicts)
    }
}
