use rusqlite::Connection;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::logger::{log_error, log_info, log_warn};

pub struct BackupManager;

impl BackupManager {
    /// 执行数据库自检并在健康时自动生成 .bak 备份；损坏时自动从 .bak 自愈恢复
    pub fn verify_and_auto_heal<P: AsRef<Path>>(db_path: P) -> Result<(), String> {
        let db_path = db_path.as_ref();
        let bak_path = PathBuf::from(format!("{}.bak", db_path.display()));

        if !db_path.exists() {
            // 首次启动数据库尚未创建，无需自检
            return Ok(());
        }

        // 打开并执行 PRAGMA integrity_check 完整性自检
        let is_healthy = match Connection::open(db_path) {
            Ok(conn) => {
                let check_res: Result<String, _> =
                    conn.query_row("PRAGMA integrity_check;", [], |row| row.get(0));
                matches!(check_res, Ok(ref s) if s == "ok")
            }
            Err(_) => false,
        };

        if is_healthy {
            if let Err(e) = fs::copy(db_path, &bak_path) {
                log_warn(&format!("自动生成数据库 .bak 备份失败: {e:?}"));
            } else {
                log_info("SQLite 数据库完整性检查通过，已更新 .bak 备份");
            }
            Ok(())
        } else {
            log_error("检测到 SQLite 数据库损坏，启动 .bak 自愈恢复流程...");
            if bak_path.exists() {
                fs::copy(&bak_path, db_path)
                    .map_err(|e| format!("从 .bak 自愈恢复数据库失败: {e:?}"))?;
                log_info("成功从 .bak 备份恢复数据库");
                Ok(())
            } else {
                log_warn("未发现可用的 .bak 备份，隔离损坏文件并重建数据库");
                let ts = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let corrupted_path = format!("{}.corrupted.{ts}", db_path.display());
                let _ = fs::rename(db_path, corrupted_path);
                Ok(())
            }
        }
    }

    /// 手动立即备份当前 SQLite 数据库至 `.bak` 文件
    pub fn manual_backup_now<P: AsRef<Path>>(db_path: P) -> Result<String, String> {
        let db_path = db_path.as_ref();
        if !db_path.exists() {
            return Err("数据库文件尚未生成".to_string());
        }
        let bak_path = PathBuf::from(format!("{}.bak", db_path.display()));
        fs::copy(db_path, &bak_path)
            .map_err(|e| format!("手动备份数据库失败: {e:?}"))?;
        log_info(&format!("已完成手动数据库备份: {}", bak_path.display()));
        Ok(bak_path.display().to_string())
    }
}
