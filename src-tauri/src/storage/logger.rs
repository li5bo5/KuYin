// 架构红线（六.4）：滚动错误日志记录（单文件上限 5 MB，最多保留 2 个日志文件）
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_LOG_FILE_BYTES: u64 = 5 * 1024 * 1024; // 单文件硬性上限 5 MB

struct RollingFileLogger {
    log_dir: PathBuf,
    current_path: PathBuf,
    backup_path: PathBuf,
    lock: Mutex<()>,
}

static GLOBAL_LOGGER: OnceLock<RollingFileLogger> = OnceLock::new();

impl RollingFileLogger {
    fn new(app_data_dir: PathBuf) -> Result<Self, String> {
        let log_dir = app_data_dir.join("logs");
        fs::create_dir_all(&log_dir).map_err(|e| format!("创建日志目录失败: {e}"))?;
        let current_path = log_dir.join("kuyin.log");
        let backup_path = log_dir.join("kuyin.1.log");

        // 清理多余历史日志，确保最多仅保留 2 个日志文件 (kuyin.log 与 kuyin.1.log)
        if let Ok(entries) = fs::read_dir(&log_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p != current_path && p != backup_path {
                    let _ = fs::remove_file(p);
                }
            }
        }

        Ok(Self {
            log_dir,
            current_path,
            backup_path,
            lock: Mutex::new(()),
        })
    }

    fn write_entry(&self, level: &str, message: &str) {
        let Ok(_guard) = self.lock.lock() else {
            return;
        };

        // 检查当前文件大小，达到 5MB 时轮转至 kuyin.1.log（自动覆盖旧备份，严控最多 2 个文件）
        if let Ok(meta) = fs::metadata(&self.current_path) {
            if meta.len() >= MAX_LOG_FILE_BYTES {
                let _ = fs::remove_file(&self.backup_path);
                let _ = fs::rename(&self.current_path, &self.backup_path);
            }
        }

        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let line = format!("[{ts}] [{level}] {message}\n");

        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.current_path)
        {
            let _ = file.write_all(line.as_bytes());
        }
    }
}

pub fn init_logger(app_data_dir: PathBuf) -> Result<PathBuf, String> {
    let logger = RollingFileLogger::new(app_data_dir)?;
    let dir = logger.log_dir.clone();
    let _ = GLOBAL_LOGGER.set(logger);
    log_info("酷音 KuYin 滚动日志系统已启动（单文件上限 5MB，最多保留 2 个文件）");
    Ok(dir)
}

pub fn log_info(msg: &str) {
    if let Some(logger) = GLOBAL_LOGGER.get() {
        logger.write_entry("INFO", msg);
    }
}

pub fn log_warn(msg: &str) {
    if let Some(logger) = GLOBAL_LOGGER.get() {
        logger.write_entry("WARN", msg);
    }
}

pub fn log_error(msg: &str) {
    if let Some(logger) = GLOBAL_LOGGER.get() {
        logger.write_entry("ERROR", msg);
    }
}

pub fn get_log_dir() -> Option<&'static Path> {
    GLOBAL_LOGGER.get().map(|l| l.log_dir.as_path())
}
