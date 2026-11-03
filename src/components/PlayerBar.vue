<template>
  <footer class="player-bar" @wheel.prevent="handleWheelVolume">
    <!-- 左下角歌曲信息区：点击封面直接展开/收起歌词（已按要求取消原“词”按钮） -->
    <div class="track-info">
      <div
        class="track-cover-box"
        @click="playerStore.toggleLyrics"
        :title="playerStore.showLyrics ? '点击封面收起歌词' : '点击封面查看歌词'"
      >
        <img
          v-if="playerStore.safeCoverUrl && !coverErr"
          :src="playerStore.safeCoverUrl"
          alt="cover"
          loading="lazy"
          decoding="async"
          class="track-cover"
          @error="coverErr = true"
        />
        <div v-else class="track-cover-placeholder">
          <SvgIcon name="music" :size="20" />
        </div>
        <div class="cover-lyric-mask">
          <SvgIcon :name="playerStore.showLyrics ? 'chevron-down' : 'chevron-up'" :size="18" />
        </div>
      </div>

      <div class="track-meta" @click="playerStore.toggleLyrics" title="点击展开/收起歌词">
        <div class="track-title-row">
          <span class="track-name">{{ playerStore.currentTrack?.name || '酷音 KuYin' }}</span>
          <button
            v-if="playerStore.currentTrack"
            class="like-btn"
            @click.stop="handleLikeCurrent"
            title="收藏这首歌到默认歌单"
          >
            <SvgIcon name="heart" :size="15" />
          </button>
        </div>
        <span class="track-artist">
          {{ playerStore.currentTrack?.singer || '点击歌曲即可畅听 · 点击封面查看歌词' }}
        </span>
      </div>
    </div>

    <!-- 中间核心播放控制与进度条（整洁三键，无“词”按钮） -->
    <div class="playback-controls">
      <div class="button-group">
        <button class="ctrl-icon-btn" @click="playerStore.previous" title="上一首">
          <SvgIcon name="skip-back" :size="18" />
        </button>

        <button
          class="play-main-btn"
          @click="playerStore.togglePlay"
          :title="playerStore.isPlaying ? '暂停' : '播放'"
        >
          <span v-if="playerStore.isBuffering" class="buffer-dot"></span>
          <SvgIcon v-else :name="playerStore.isPlaying ? 'pause' : 'play'" :size="18" />
        </button>

        <button class="ctrl-icon-btn" @click="playerStore.next" title="下一首">
          <SvgIcon name="skip-forward" :size="18" />
        </button>
      </div>

      <div class="progress-bar-wrapper">
        <span class="time-label">{{ formatTime(displayPositionMs) }}</span>
        <div
          class="progress-slider-container"
          @pointerdown="onProgressPointerDown"
          @pointermove="onProgressPointerMove"
          @pointerup="onProgressPointerUp"
          @pointercancel="onProgressPointerUp"
        >
          <div class="progress-track">
            <div
              :class="['progress-fill', { 'no-transition': isDraggingProgress }]"
              :style="{ width: progressPercent + '%' }"
            ></div>
          </div>
        </div>
        <span class="time-label">{{ formatTime(playerStore.durationMs) }}</span>
      </div>
    </div>

    <!-- 右侧：播放模式下拉框 + 音质选择下拉框 + 音量 + 播放队列 -->
    <div class="volume-controls">
      <FluentSelect
        class="mode-select"
        :model-value="playerStore.mode"
        :options="modeOptions"
        placement="top"
        title="选择播放模式"
        @change="handleModeSelect"
      />

      <FluentSelect
        class="quality-select"
        :model-value="playerStore.quality"
        :options="qualityOptions"
        placement="top"
        title="选择播放音质"
        @change="handleQualitySelect"
      />

      <button
        class="vol-btn"
        @click="toggleMute"
        :title="playerStore.volume === 0 ? '取消静音' : '静音'"
      >
        <SvgIcon
          :name="
            playerStore.volume === 0
              ? 'volume-off'
              : playerStore.volume < 0.5
                ? 'volume-low'
                : 'volume'
          "
          :size="17"
        />
      </button>
      <div
        class="vol-slider-container"
        @pointerdown="onVolPointerDown"
        @pointermove="onVolPointerMove"
        @pointerup="onVolPointerUp"
        @pointercancel="onVolPointerUp"
      >
        <div class="progress-track">
          <div
            :class="['progress-fill', { 'no-transition': isDraggingVol }]"
            :style="{ width: displayVolPercent + '%' }"
          ></div>
        </div>
      </div>
      <span class="vol-num-label">{{ displayVolPercent }}</span>

      <button
        :class="['queue-btn', { active: playerStore.showQueue }]"
        @click="playerStore.toggleQueue"
        title="打开/收起播放队列"
      >
        <SvgIcon name="playlist" :size="16" />
        <span class="q-badge">{{ playerStore.queueCount }}</span>
      </button>
    </div>
  </footer>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { usePlayerStore, type PlaybackMode } from '../stores/player';
import { useAppStore } from '../stores/app';
import SvgIcon from './SvgIcon.vue';
import FluentSelect from './FluentSelect.vue';

const modeOptions = [
  { value: 'sequence', label: '顺序播放' },
  { value: 'loop', label: '列表循环' },
  { value: 'single', label: '单曲循环' },
  { value: 'shuffle', label: '随机播放' },
];

const qualityOptions = [
  { value: '128', label: '标准 128K' },
  { value: '320', label: '高品 320K' },
  { value: 'flac', label: '无损 FLAC' },
];

const playerStore = usePlayerStore();
const appStore = useAppStore();
const previousVolume = ref(0.8);
const coverErr = ref(false);

const isDraggingProgress = ref(false);
const dragProgressRatio = ref(0);

const isDraggingVol = ref(false);
const dragVolRatio = ref(1);
let volRafPending = false;

watch(
  () => playerStore.safeCoverUrl,
  () => {
    coverErr.value = false;
  }
);

const displayPositionMs = computed(() => {
  if (isDraggingProgress.value) {
    return Math.floor(dragProgressRatio.value * playerStore.durationMs);
  }
  return playerStore.positionMs;
});

const progressPercent = computed(() => {
  if (isDraggingProgress.value) {
    return dragProgressRatio.value * 100;
  }
  if (!playerStore.durationMs) return 0;
  return Math.min(100, (playerStore.positionMs / playerStore.durationMs) * 100);
});

const displayVolPercent = computed(() => {
  const v = isDraggingVol.value ? dragVolRatio.value : playerStore.volume;
  return Math.round(Math.max(0, Math.min(1, v)) * 100);
});

function computePointerRatio(e: PointerEvent): number {
  const container = e.currentTarget as HTMLElement;
  const rect = container.getBoundingClientRect();
  if (rect.width <= 0) return 0;
  return Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width));
}

function onProgressPointerDown(e: PointerEvent) {
  (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  isDraggingProgress.value = true;
  dragProgressRatio.value = computePointerRatio(e);
}

function onProgressPointerMove(e: PointerEvent) {
  if (!isDraggingProgress.value) return;
  dragProgressRatio.value = computePointerRatio(e);
}

function onProgressPointerUp(e: PointerEvent) {
  if (!isDraggingProgress.value) return;
  dragProgressRatio.value = computePointerRatio(e);
  isDraggingProgress.value = false;
  const targetMs = Math.floor(dragProgressRatio.value * playerStore.durationMs);
  playerStore.seek(targetMs);
}

function scheduleVolumeApply(vol: number) {
  if (vol > 0) {
    previousVolume.value = vol;
  }
  if (volRafPending) return;
  volRafPending = true;
  requestAnimationFrame(() => {
    volRafPending = false;
    playerStore.setVolume(dragVolRatio.value);
  });
}

function onVolPointerDown(e: PointerEvent) {
  (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  isDraggingVol.value = true;
  dragVolRatio.value = computePointerRatio(e);
  scheduleVolumeApply(dragVolRatio.value);
}

function onVolPointerMove(e: PointerEvent) {
  if (!isDraggingVol.value) return;
  dragVolRatio.value = computePointerRatio(e);
  scheduleVolumeApply(dragVolRatio.value);
}

function onVolPointerUp(e: PointerEvent) {
  if (!isDraggingVol.value) return;
  dragVolRatio.value = computePointerRatio(e);
  isDraggingVol.value = false;
  if (dragVolRatio.value > 0) {
    previousVolume.value = dragVolRatio.value;
  }
  playerStore.setVolume(dragVolRatio.value);
}

function handleModeSelect(val: string) {
  playerStore.setMode(val as PlaybackMode);
}

function handleQualitySelect(val: string) {
  playerStore.setQuality(val);
}

async function handleLikeCurrent() {
  if (!playerStore.currentTrack) return;
  try {
    const msg = await appStore.favoriteTrackToDefault(playerStore.currentTrack);
    playerStore.showToast(msg);
  } catch (err: any) {
    playerStore.showToast(err?.message || String(err));
  }
}

function handleWheelVolume(e: WheelEvent) {
  const delta = e.deltaY < 0 ? 0.05 : -0.05;
  const next = Math.max(0, Math.min(1, playerStore.volume + delta));
  if (next > 0) {
    previousVolume.value = next;
  }
  playerStore.setVolume(next);
}

function toggleMute() {
  if (playerStore.volume > 0) {
    previousVolume.value = playerStore.volume;
    playerStore.setVolume(0);
  } else {
    playerStore.setVolume(previousVolume.value || 0.8);
  }
}

function formatTime(ms: number): string {
  const totalSecs = Math.floor(ms / 1000);
  const m = Math.floor(totalSecs / 60);
  const s = totalSecs % 60;
  return `${m}:${s < 10 ? '0' : ''}${s}`;
}
</script>

<style scoped>
.player-bar {
  height: var(--player-height);
  background: var(--bg-player);
  border-top: 1px solid var(--border-divider);
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 16px;
  gap: 12px;
  backdrop-filter: blur(25px);
  -webkit-backdrop-filter: blur(25px);
  z-index: 90;
}

.track-info {
  display: flex;
  align-items: center;
  gap: 12px;
  flex: 0 1 260px;
  min-width: 0;
}

.track-cover-box {
  position: relative;
  width: 48px;
  height: 48px;
  border-radius: var(--radius-md);
  overflow: hidden;
  background: var(--bg-card);
  border: 1px solid var(--border-subtle);
  flex-shrink: 0;
  cursor: pointer;
}

.track-cover {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.track-cover-placeholder {
  width: 100%;
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-tertiary);
}

.cover-lyric-mask {
  position: absolute;
  inset: 0;
  background: rgba(0, 0, 0, 0.42);
  color: #ffffff;
  display: flex;
  align-items: center;
  justify-content: center;
  opacity: 0;
  transition: opacity var(--duration-fast);
}

.track-cover-box:hover .cover-lyric-mask {
  opacity: 1;
}

.track-meta {
  display: flex;
  flex-direction: column;
  gap: 3px;
  min-width: 0;
  cursor: pointer;
}

.track-title-row {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
}

.track-name {
  font-size: 13px;
  font-weight: 600;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.like-btn {
  color: var(--text-tertiary);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 2px;
  border-radius: var(--radius-full);
  flex-shrink: 0;
}

.like-btn:hover {
  color: #ef4444;
  background: var(--bg-card-hover);
}

.track-artist {
  font-size: 11px;
  color: var(--text-secondary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.playback-controls {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 5px;
  flex: 1 1 380px;
  max-width: 460px;
}

.button-group {
  display: flex;
  align-items: center;
  gap: 16px;
}

.ctrl-icon-btn {
  color: var(--text-secondary);
  width: 32px;
  height: 32px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: var(--radius-full);
  transition: all var(--duration-fast);
}

.ctrl-icon-btn:hover {
  color: var(--text-primary);
  background: var(--bg-card-hover);
}

.play-main-btn {
  width: 38px;
  height: 38px;
  border-radius: var(--radius-full);
  background: var(--accent-color);
  color: var(--accent-text);
  display: flex;
  align-items: center;
  justify-content: center;
  transition: transform var(--duration-fast), background var(--duration-fast);
}

.play-main-btn:hover {
  background: var(--accent-hover);
  transform: scale(1.06);
}

.buffer-dot {
  width: 14px;
  height: 14px;
  border: 2px solid currentColor;
  border-top-color: transparent;
  border-radius: 50%;
  animation: spin 0.75s linear infinite;
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}

.progress-bar-wrapper {
  width: 100%;
  display: flex;
  align-items: center;
  gap: 8px;
}

.time-label {
  font-size: 11px;
  color: var(--text-tertiary);
  width: 36px;
  text-align: center;
  font-variant-numeric: tabular-nums;
}

.progress-slider-container {
  flex: 1;
  height: 14px;
  display: flex;
  align-items: center;
  cursor: pointer;
}

.progress-track {
  width: 100%;
  height: 4px;
  background: var(--border-subtle);
  border-radius: var(--radius-full);
  position: relative;
  overflow: hidden;
}

.progress-fill {
  height: 100%;
  background: var(--accent-color);
  border-radius: var(--radius-full);
  transition: width 100ms linear;
}

.progress-fill.no-transition {
  transition: none;
}

.volume-controls {
  display: flex;
  align-items: center;
  gap: 8px;
  flex: 0 0 auto;
  justify-content: flex-end;
}

.mode-select,
.quality-select {
  height: 28px;
}

.vol-btn {
  width: 28px;
  height: 28px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: var(--radius-full);
  color: var(--text-secondary);
}

.vol-btn:hover {
  background: var(--bg-card-hover);
  color: var(--text-primary);
}

.vol-slider-container {
  width: 66px;
  height: 14px;
  display: flex;
  align-items: center;
  cursor: pointer;
}

.vol-num-label {
  font-size: 11px;
  color: var(--text-secondary);
  width: 24px;
  text-align: left;
  font-variant-numeric: tabular-nums;
}

.queue-btn {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  height: 28px;
  padding: 0 9px;
  border-radius: var(--radius-md);
  font-size: 12px;
  color: var(--text-secondary);
  background: var(--bg-card);
  border: 1px solid var(--border-subtle);
}

.queue-btn.active,
.queue-btn:hover {
  color: var(--accent-color);
  background: var(--bg-card-hover);
}

.q-badge {
  font-size: 11px;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
}
</style>
