use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::logger::log_info;

#[derive(Clone, Debug)]
pub struct SavedPlaybackSnapshot {
    pub current_track_hash: Option<String>,
    pub position_ms: u64,
    pub volume: f64,
    pub mode: String,
    pub queue_json: String,
}

pub struct Database {
    conn: Mutex<Connection>,
    db_path: PathBuf,
}

impl Database {
    pub fn init<P: AsRef<Path>>(db_path: P) -> Result<Self, String> {
        let path_buf = db_path.as_ref().to_path_buf();
        if let Some(parent) = path_buf.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let conn = Connection::open(&path_buf)
            .map_err(|e| format!("打开 SQLite 数据库失败: {e:?}"))?;

        // 架构红线（六.4）：开启 WAL 模式与 NORMAL 同步，兼顾低 IO 与防坏库
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA temp_store = MEMORY;
             PRAGMA foreign_keys = ON;",
        )
        .map_err(|e| format!("配置 SQLite PRAGMA 失败: {e:?}"))?;

        // 创建核心表与 200 条历史硬上限 Trigger
        conn.execute_batch(
            "-- 基础键值配置表 (设置项、登录态、窗口坐标、设备指纹等)
            CREATE TABLE IF NOT EXISTS kv_settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );

            -- 播放状态与队列快照表 (低频落盘与断点恢复)
            CREATE TABLE IF NOT EXISTS playback_snapshot (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                current_track_hash TEXT,
                position_ms INTEGER NOT NULL DEFAULT 0,
                volume REAL NOT NULL DEFAULT 1.0,
                mode TEXT NOT NULL DEFAULT 'sequence',
                queue_json TEXT NOT NULL DEFAULT '[]',
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );

            -- 播放历史表 (硬性上限 200 条)
            CREATE TABLE IF NOT EXISTS playback_history (
                hash TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                singer TEXT NOT NULL,
                album_id TEXT,
                album_audio_id INTEGER,
                duration INTEGER NOT NULL,
                cover_url TEXT,
                played_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );

            -- 架构红线（六.4）：Trigger 自动淘汰最旧记录，确保历史表永不超过 200 条
            CREATE TRIGGER IF NOT EXISTS limit_playback_history_200
            AFTER INSERT ON playback_history
            BEGIN
                DELETE FROM playback_history
                WHERE hash IN (
                    SELECT hash FROM playback_history
                    ORDER BY played_at DESC
                    LIMIT -1 OFFSET 200
                );
            END;

            CREATE TRIGGER IF NOT EXISTS limit_playback_history_update_200
            AFTER UPDATE ON playback_history
            BEGIN
                DELETE FROM playback_history
                WHERE hash IN (
                    SELECT hash FROM playback_history
                    ORDER BY played_at DESC
                    LIMIT -1 OFFSET 200
                );
            END;",
        )
        .map_err(|e| format!("初始化数据表与 Trigger 失败: {e:?}"))?;

        log_info("SQLite 数据库初始化成功 (WAL 模式 + 200 条历史 Trigger 就绪)");

        Ok(Self {
            conn: Mutex::new(conn),
            db_path: path_buf,
        })
    }

    #[allow(dead_code)]
    pub fn get_db_path(&self) -> &Path {
        &self.db_path
    }

    pub fn get_setting(&self, key: &str) -> Option<String> {
        let Ok(conn) = self.conn.lock() else {
            return None;
        };
        let mut stmt = conn
            .prepare_cached("SELECT value FROM kv_settings WHERE key = ?1")
            .ok()?;
        stmt.query_row(params![key], |row| row.get(0)).ok()
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), String> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| "数据库锁获取失败".to_string())?;
        conn.execute(
            "INSERT INTO kv_settings (key, value, updated_at)
             VALUES (?1, ?2, CURRENT_TIMESTAMP)
             ON CONFLICT(key) DO UPDATE SET value = ?2, updated_at = CURRENT_TIMESTAMP",
            params![key, value],
        )
        .map_err(|e| format!("写入配置失败: {e:?}"))?;
        Ok(())
    }

    pub fn delete_setting(&self, key: &str) -> Result<(), String> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| "数据库锁获取失败".to_string())?;
        conn.execute("DELETE FROM kv_settings WHERE key = ?1", params![key])
            .map_err(|e| format!("删除配置失败: {e:?}"))?;
        Ok(())
    }

    pub fn save_playback_snapshot(
        &self,
        track_hash: Option<&str>,
        position_ms: u64,
        volume: f64,
        mode: &str,
        queue_json: &str,
    ) -> Result<(), String> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| "数据库锁获取失败".to_string())?;
        conn.execute(
            "INSERT INTO playback_snapshot (id, current_track_hash, position_ms, volume, mode, queue_json, updated_at)
             VALUES (1, ?1, ?2, ?3, ?4, ?5, CURRENT_TIMESTAMP)
             ON CONFLICT(id) DO UPDATE SET
                current_track_hash = ?1,
                position_ms = ?2,
                volume = ?3,
                mode = ?4,
                queue_json = ?5,
                updated_at = CURRENT_TIMESTAMP",
            params![track_hash, position_ms as i64, volume, mode, queue_json],
        )
        .map_err(|e| format!("保存播放快照失败: {e:?}"))?;
        Ok(())
    }

    pub fn load_playback_snapshot(&self) -> Option<SavedPlaybackSnapshot> {
        let conn = self.conn.lock().ok()?;
        let mut stmt = conn
            .prepare(
                "SELECT current_track_hash, position_ms, volume, mode, queue_json
                 FROM playback_snapshot WHERE id = 1",
            )
            .ok()?;

        stmt.query_row([], |row| {
            let current_track_hash: Option<String> = row.get(0)?;
            let pos_i64: i64 = row.get(1)?;
            let volume: f64 = row.get(2)?;
            let mode: String = row.get(3)?;
            let queue_json: String = row.get(4)?;
            Ok(SavedPlaybackSnapshot {
                current_track_hash,
                position_ms: pos_i64.max(0) as u64,
                volume: volume.clamp(0.0, 1.0),
                mode,
                queue_json,
            })
        })
        .ok()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_history(
        &self,
        hash: &str,
        name: &str,
        singer: &str,
        album_id: Option<&str>,
        album_audio_id: Option<i64>,
        duration: u32,
        cover_url: Option<&str>,
    ) -> Result<(), String> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| "数据库锁获取失败".to_string())?;
        conn.execute(
            "INSERT INTO playback_history (hash, name, singer, album_id, album_audio_id, duration, cover_url, played_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, CURRENT_TIMESTAMP)
             ON CONFLICT(hash) DO UPDATE SET
                name = ?2,
                singer = ?3,
                album_id = ?4,
                album_audio_id = ?5,
                duration = ?6,
                cover_url = ?7,
                played_at = CURRENT_TIMESTAMP",
            params![hash, name, singer, album_id, album_audio_id, duration, cover_url],
        )
        .map_err(|e| format!("记录播放历史失败: {e:?}"))?;
        Ok(())
    }

    pub fn get_history(&self, limit: u32) -> Result<Vec<serde_json::Value>, String> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| "数据库锁获取失败".to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT hash, name, singer, album_id, album_audio_id, duration, cover_url, played_at
                 FROM playback_history
                 ORDER BY played_at DESC
                 LIMIT ?1",
            )
            .map_err(|e| format!("查询播放历史准备失败: {e:?}"))?;

        let rows = stmt
            .query_map(params![limit.min(200)], |row| {
                let hash: String = row.get(0)?;
                let name: String = row.get(1)?;
                let singer: String = row.get(2)?;
                let album_id: Option<String> = row.get(3)?;
                let album_audio_id: Option<i64> = row.get(4)?;
                let duration: u32 = row.get(5)?;
                let cover_url: Option<String> = row.get(6)?;
                let played_at: String = row.get(7)?;

                Ok(serde_json::json!({
                    "hash": hash,
                    "name": name,
                    "singer": singer,
                    "album_id": album_id,
                    "album_audio_id": album_audio_id,
                    "duration": duration,
                    "cover_url": cover_url,
                    "played_at": played_at,
                }))
            })
            .map_err(|e| format!("查询历史执行失败: {e:?}"))?;

        let mut list = Vec::new();
        for item in rows.flatten() {
            list.push(item);
        }
        Ok(list)
    }

    pub fn clear_history(&self) -> Result<(), String> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| "数据库锁获取失败".to_string())?;
        conn.execute("DELETE FROM playback_history", [])
            .map_err(|e| format!("清空历史记录失败: {e:?}"))?;
        Ok(())
    }

    /// 截断回收 SQLite WAL 预写日志临时文件体积
    pub fn checkpoint_wal(&self) {
        if let Ok(conn) = self.conn.lock() {
            let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
        }
    }
}
