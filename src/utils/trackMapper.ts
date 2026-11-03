import type { Track } from '../stores/player';

export function stripEmTags(text: string): string {
  return text.replace(/<\/?em>/gi, '').trim();
}

export function normalizeDurationSec(raw: unknown): number {
  const num = Number(raw);
  if (!Number.isFinite(num) || num <= 0) return 200;
  // 酷狗部分接口返回毫秒 (timelen > 1500)，部分返回秒 (duration <= 1500)
  return num > 1500 ? Math.round(num / 1000) : Math.round(num);
}

/**
 * 架构红线（五.2）：封面分辨率硬限制，强制将 {size} 替换为 100 或 240 / 400 缩略图
 */
export function formatSafeCover(
  url: string | undefined | null,
  size: 100 | 240 | 400 = 100
): string {
  if (!url || typeof url !== 'string') return '';
  return url
    .replace(/\{size\}/g, String(size))
    .replace(/^http:\/\//i, 'https://');
}

/**
 * 统一将酷狗各业务网关（搜索、每日推荐、新歌、排行榜、私人FM、歌单、专辑）的异构字段映射为标准轻量 Track
 */
export function mapRawToTrack(item: any): Track | null {
  if (!item || typeof item !== 'object') return null;

  const hash: string =
    item.hash ||
    item.FileHash ||
    item.SQFileHash ||
    item.HQFileHash ||
    item.deprecated?.hash ||
    item.audio_info?.hash ||
    item.audio_info?.hash_320 ||
    item.audio_info?.hash_128 ||
    '';

  if (!hash || typeof hash !== 'string') return null;

  let rawName: string =
    item.ori_audio_name ||
    item.SongName ||
    item.OriSongName ||
    item.songname ||
    item.audio_name ||
    item.base?.audio_name ||
    item.name ||
    item.filename ||
    item.FileName ||
    '';

  let rawSinger: string =
    item.author_name ||
    item.SingerName ||
    item.singername ||
    item.base?.author_name ||
    item.singer ||
    (Array.isArray(item.Singers)
      ? item.Singers.map((s: any) => s?.name).filter(Boolean).join(' / ')
      : '') ||
    '';

  rawName = stripEmTags(String(rawName));
  rawSinger = stripEmTags(String(rawSinger));

  // 歌单曲目常见 "歌手 - 歌名" 合并在 name 字段中的情况
  if (!rawSinger && rawName.includes(' - ')) {
    const parts = rawName.split(' - ');
    rawSinger = parts[0].trim();
    rawName = parts.slice(1).join(' - ').trim();
  }

  const albumIdRaw =
    item.album_id ??
    item.AlbumID ??
    item.album_info?.album_id ??
    item.albuminfo?.id;
  const albumAudioIdRaw =
    item.album_audio_id ??
    item.AlbumAudioID ??
    item.MixSongID ??
    item.mixsongid ??
    item.base?.album_audio_id;

  const durationRaw =
    item.duration ??
    item.Duration ??
    item.time_length ??
    item.timelen ??
    item.deprecated?.duration ??
    item.audio_info?.duration ??
    200;

  const coverRaw: string | undefined =
    item.sizable_cover ||
    item.Image ||
    item.trans_param?.union_cover ||
    item.imgurl ||
    item.album_img ||
    item.cover ||
    item.albuminfo?.cover ||
    undefined;

  const fileIdRaw = item.fileid ?? item.FileId ?? item.file_id;

  return {
    hash: hash.toLowerCase(),
    name: rawName || '未知曲目',
    singer: rawSinger || '未知歌手',
    album_id: albumIdRaw !== undefined && albumIdRaw !== null ? String(albumIdRaw) : undefined,
    album_audio_id:
      albumAudioIdRaw !== undefined && albumAudioIdRaw !== null && !Number.isNaN(Number(albumAudioIdRaw))
        ? Number(albumAudioIdRaw)
        : undefined,
    duration: normalizeDurationSec(durationRaw),
    cover_url: coverRaw ? formatSafeCover(coverRaw, 100) : undefined,
    fileid:
      fileIdRaw !== undefined && fileIdRaw !== null && !Number.isNaN(Number(fileIdRaw))
        ? Number(fileIdRaw)
        : undefined,
  };
}

export function extractTracksFromResponse(res: any): Track[] {
  if (!res) return [];
  const data = res.data ?? res;

  let candidates: any[] = [];
  if (Array.isArray(data)) {
    candidates = data;
  } else if (Array.isArray(data?.song_list)) {
    candidates = data.song_list;
  } else if (Array.isArray(data?.songlist)) {
    candidates = data.songlist;
  } else if (Array.isArray(data?.lists)) {
    candidates = data.lists;
  } else if (Array.isArray(data?.songs)) {
    candidates = data.songs;
  } else if (Array.isArray(data?.info)) {
    candidates = data.info;
  } else if (Array.isArray(data?.list)) {
    candidates = data.list;
  } else if (Array.isArray(res?.songs?.data?.info)) {
    candidates = res.songs.data.info;
  } else if (Array.isArray(res?.songs?.data?.songs)) {
    candidates = res.songs.data.songs;
  }

  const tracks: Track[] = [];
  for (const item of candidates) {
    const mapped = mapRawToTrack(item);
    if (mapped) {
      tracks.push(mapped);
    }
  }
  return tracks;
}

export interface LyricLine {
  timeMs: number;
  text: string;
}

/**
 * 解析由 Rust 后端解密并转换为纯文本的 LRC 歌词（兼容多时间戳、无毫秒及变长时间格式）
 */
export function parseLrcText(raw: string): LyricLine[] {
  const result: LyricLine[] = [];
  if (!raw) return result;

  const timeTagRegex = /\[(\d{1,3}):(\d{1,2})(?:[.:](\d{1,3}))?\]/g;

  for (const line of raw.split(/\r?\n/)) {
    const trimmed = line.trim();
    if (!trimmed) continue;

    const matches = Array.from(trimmed.matchAll(timeTagRegex));
    if (matches.length > 0) {
      const text = trimmed.replace(timeTagRegex, '').trim();
      if (!text) continue;

      for (const match of matches) {
        const min = parseInt(match[1], 10);
        const sec = parseInt(match[2], 10);
        let ms = 0;
        if (match[3]) {
          const fracStr = match[3];
          if (fracStr.length === 1) {
            ms = parseInt(fracStr, 10) * 100;
          } else if (fracStr.length === 2) {
            ms = parseInt(fracStr, 10) * 10;
          } else {
            ms = parseInt(fracStr.slice(0, 3), 10);
          }
        }
        result.push({
          timeMs: min * 60000 + sec * 1000 + ms,
          text,
        });
      }
    }
  }

  result.sort((a, b) => a.timeMs - b.timeMs);
  return result;
}
