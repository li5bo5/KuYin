import { defineStore } from 'pinia';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { formatSafeCover, parseLrcText, type LyricLine } from '../utils/trackMapper';

export interface Track {
  hash: string;
  name: string;
  singer: string;
  album_id?: string;
  album_audio_id?: number;
  duration: number;
  cover_url?: string;
  fileid?: number;
}

export type PlaybackMode = 'sequence' | 'loop' | 'single' | 'shuffle';

export interface PlayerSnapshot {
  current_track: Track | null;
  is_playing: boolean;
  is_buffering: boolean;
  position_ms: number;
  duration_ms: number;
  volume: number;
  mode: PlaybackMode;
  quality: string;
  queue_count: number;
  current_index: number | null;
}

let visibilityHandler: (() => void) | null = null;
let toastUnlisten: UnlistenFn | null = null;
let toastClearTimer: number | null = null;

export const usePlayerStore = defineStore('player', {
  state: () => ({
    snapshot: {
      current_track: null,
      is_playing: false,
      is_buffering: false,
      position_ms: 0,
      duration_ms: 0,
      volume: 1.0,
      mode: 'sequence' as PlaybackMode,
      quality: '320',
      queue_count: 0,
      current_index: null,
    } as PlayerSnapshot,
    snapshotUpdatedAt: 0,
    queueTracks: [] as Track[],
    lyricLines: [] as LyricLine[],
    lyricLoading: false,
    loadedLyricHash: '',
    showLyrics: false,
    showQueue: false,
    toastMessage: '',
    pollTimer: null as number | null,
  }),

  getters: {
    currentTrack: (state) => state.snapshot.current_track,
    isPlaying: (state) => state.snapshot.is_playing,
    isBuffering: (state) => state.snapshot.is_buffering,
    positionMs: (state) => state.snapshot.position_ms,
    durationMs: (state) => state.snapshot.duration_ms,
    volume: (state) => state.snapshot.volume,
    mode: (state) => state.snapshot.mode,
    quality: (state) => state.snapshot.quality || '320',
    queueCount: (state) => state.snapshot.queue_count,
    currentIndex: (state) => state.snapshot.current_index,
    // 强制缩略图保护：将酷狗 {size} 替换为 100
    safeCoverUrl: (state) => {
      return formatSafeCover(state.snapshot.current_track?.cover_url, 100);
    },
    currentLyricIndex: (state): number => {
      const lines = state.lyricLines;
      if (lines.length === 0) return -1;
      const pos = state.snapshot.position_ms;
      let idx = 0;
      for (let i = 0; i < lines.length; i++) {
        if (pos >= lines[i].timeMs) {
          idx = i;
        } else {
          break;
        }
      }
      return idx;
    },
    currentLyricText(): string {
      const idx = this.currentLyricIndex;
      if (idx >= 0 && idx < this.lyricLines.length) {
        return this.lyricLines[idx].text;
      }
      return '';
    },
  },

  actions: {
    showToast(msg: string) {
      this.toastMessage = msg;
      if (toastClearTimer) {
        clearTimeout(toastClearTimer);
      }
      toastClearTimer = window.setTimeout(() => {
        this.toastMessage = '';
      }, 3200);
    },

    async loadLyricForTrack(track: Track | null, force = false) {
      if (!track || !track.hash) {
        this.lyricLines = [];
        this.loadedLyricHash = '';
        return;
      }
      const normHash = track.hash.toLowerCase();
      if (!force && this.loadedLyricHash === normHash) {
        return;
      }
      this.loadedLyricHash = normHash;
      this.lyricLoading = true;
      try {
        const durMs = this.snapshot.duration_ms || track.duration * 1000 || 200000;
        const keyword = track.singer
          ? `${track.singer} - ${track.name}`
          : track.name;
        const lrcText = await invoke<string>('get_lyric', {
          hash: track.hash,
          durationMs: durMs,
          keyword,
          albumAudioId: track.album_audio_id ?? null,
        });
        if (this.snapshot.current_track?.hash.toLowerCase() === normHash) {
          this.lyricLines = parseLrcText(lrcText);
        }
      } catch {
        if (this.snapshot.current_track?.hash.toLowerCase() === normHash) {
          this.lyricLines = [];
        }
      } finally {
        if (this.snapshot.current_track?.hash.toLowerCase() === normHash) {
          this.lyricLoading = false;
        }
      }
    },

    // 架构红线（一.2）：前端仅拉取 Rust 状态快照，绝不维护本地计时器或播放引擎
    async fetchSnapshot() {
      if (typeof document !== 'undefined' && document.hidden) {
        return;
      }
      try {
        const snap = await invoke<PlayerSnapshot>('get_snapshot');
        const prevHash = this.snapshot.current_track?.hash || '';
        this.snapshot = snap;
        this.snapshotUpdatedAt = performance.now();
        const nextHash = snap.current_track?.hash || '';
        if (nextHash && nextHash !== prevHash) {
          this.loadLyricForTrack(snap.current_track);
        } else if (!nextHash) {
          this.lyricLines = [];
          this.loadedLyricHash = '';
        }
      } catch (err) {
        console.warn('同步播放器快照失败:', err);
      }
    },

    async refreshQueue() {
      try {
        this.queueTracks = await invoke<Track[]>('get_queue');
      } catch (err) {
        console.warn('获取播放队列失败:', err);
      }
    },

    async startPolling() {
      if (this.pollTimer) return;
      await this.fetchSnapshot();
      if (this.snapshot.current_track) {
        this.loadLyricForTrack(this.snapshot.current_track);
      }

      this.pollTimer = window.setInterval(() => {
        this.fetchSnapshot();
      }, 500);

      // 第 1 级窗口休眠配合：最小化到任务栏 (document.hidden) 时零轮询挂起，唤醒时 0ms 即时同步
      if (!visibilityHandler && typeof document !== 'undefined') {
        visibilityHandler = () => {
          if (!document.hidden) {
            this.fetchSnapshot();
          }
        };
        document.addEventListener('visibilitychange', visibilityHandler);
      }

      if (!toastUnlisten) {
        toastUnlisten = await listen<string>('player-toast', (ev) => {
          if (ev.payload) {
            this.showToast(ev.payload);
          }
        });
      }
    },

    stopPolling() {
      if (this.pollTimer) {
        clearInterval(this.pollTimer);
        this.pollTimer = null;
      }
      if (visibilityHandler && typeof document !== 'undefined') {
        document.removeEventListener('visibilitychange', visibilityHandler);
        visibilityHandler = null;
      }
      if (toastUnlisten) {
        toastUnlisten();
        toastUnlisten = null;
      }
    },

    async togglePlay() {
      try {
        if (this.snapshot.is_playing) {
          await invoke('pause');
        } else {
          await invoke('play');
        }
        await this.fetchSnapshot();
      } catch (err: any) {
        this.showToast(String(err));
      }
    },

    async next() {
      try {
        await invoke('next_track');
        await this.fetchSnapshot();
        if (this.showQueue) await this.refreshQueue();
      } catch (err: any) {
        this.showToast(String(err));
      }
    },

    async previous() {
      try {
        await invoke('previous_track');
        await this.fetchSnapshot();
        if (this.showQueue) await this.refreshQueue();
      } catch (err: any) {
        this.showToast(String(err));
      }
    },

    async seek(positionMs: number) {
      await invoke('seek', { positionMs });
      await this.fetchSnapshot();
    },

    async setVolume(volume: number) {
      await invoke('set_volume', { volume: Math.max(0, Math.min(1, volume)) });
      await this.fetchSnapshot();
    },

    async setMode(mode: PlaybackMode) {
      await invoke('set_mode', { mode });
      await this.fetchSnapshot();
    },

    async setQuality(quality: string) {
      await invoke('set_quality', { quality });
      await this.fetchSnapshot();
      this.showToast(`首选音质已切换为 ${quality === 'flac' ? '无损 FLAC' : quality + 'K'}`);
    },

    async playSong(track: Track) {
      try {
        await invoke('play_song', { track });
        await this.fetchSnapshot();
        if (this.showQueue) await this.refreshQueue();
      } catch (err: any) {
        this.showToast(String(err));
      }
    },

    async playQueueIndex(index: number) {
      try {
        await invoke('play_queue_index', { index });
        await this.fetchSnapshot();
      } catch (err: any) {
        this.showToast(String(err));
      }
    },

    async setQueue(tracks: Track[], startIndex: number = 0) {
      try {
        await invoke('set_queue', { tracks, startIndex });
        await this.fetchSnapshot();
        if (this.showQueue) await this.refreshQueue();
      } catch (err: any) {
        this.showToast(String(err));
      }
    },

    async insertNext(track: Track) {
      await invoke('insert_next_queue', { track });
      await this.fetchSnapshot();
      if (this.showQueue) await this.refreshQueue();
      this.showToast(`已添加到下一首播放：${track.name}`);
    },

    async appendSong(track: Track) {
      try {
        await invoke('append_song_to_queue', { track });
        await this.fetchSnapshot();
        if (this.showQueue) await this.refreshQueue();
        this.showToast(`已添加到队列：${track.name}`);
      } catch (err: any) {
        this.showToast(String(err));
      }
    },

    async appendQueue(tracks: Track[]) {
      if (!tracks || tracks.length === 0) return;
      try {
        const count = await invoke<number>('append_queue', { tracks });
        await this.fetchSnapshot();
        if (this.showQueue) await this.refreshQueue();
        this.showToast(`已添加 ${count || tracks.length} 首`);
      } catch (err: any) {
        this.showToast(String(err));
      }
    },

    async removeFromQueue(index: number) {
      await invoke('remove_from_queue', { index });
      await this.refreshQueue();
      await this.fetchSnapshot();
    },

    async moveQueueItem(from: number, to: number) {
      await invoke('move_queue_item', { from, to });
      await this.refreshQueue();
      await this.fetchSnapshot();
    },

    async clearQueue() {
      await invoke('clear_queue');
      this.queueTracks = [];
      await this.fetchSnapshot();
    },

    toggleLyrics() {
      this.showLyrics = !this.showLyrics;
      if (this.showLyrics && this.snapshot.current_track && this.lyricLines.length === 0) {
        this.loadLyricForTrack(this.snapshot.current_track, true);
      }
    },

    async toggleQueue() {
      this.showQueue = !this.showQueue;
      if (this.showQueue) {
        await this.refreshQueue();
      }
    },
  },
});
