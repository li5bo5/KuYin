use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Track {
    pub hash: String,
    pub name: String,
    pub singer: String,
    pub album_id: Option<String>,
    pub album_audio_id: Option<i64>,
    pub duration: u32,
    pub cover_url: Option<String>,
    #[serde(default)]
    pub fileid: Option<i64>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackMode {
    #[default]
    Sequence, // 顺序播放
    Loop,     // 列表循环
    Single,   // 单曲循环
    Shuffle,  // 随机播放
}

impl PlaybackMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            PlaybackMode::Sequence => "sequence",
            PlaybackMode::Loop => "loop",
            PlaybackMode::Single => "single",
            PlaybackMode::Shuffle => "shuffle",
        }
    }

    pub fn from_db_str(s: &str) -> Self {
        match s {
            "loop" => PlaybackMode::Loop,
            "single" => PlaybackMode::Single,
            "shuffle" => PlaybackMode::Shuffle,
            _ => PlaybackMode::Sequence,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlayerSnapshot {
    pub current_track: Option<Track>,
    pub is_playing: bool,
    pub is_buffering: bool,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub volume: f64,
    pub mode: PlaybackMode,
    pub quality: String,
    pub queue_count: usize,
    pub current_index: Option<usize>,
}
