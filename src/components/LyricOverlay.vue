<template>
  <div v-if="playerStore.showLyrics" class="lyric-overlay">
    <div class="lyric-header">
      <div class="song-meta">
        <h2 class="title">{{ playerStore.currentTrack?.name || '暂未播放曲目' }}</h2>
        <p class="singer">{{ playerStore.currentTrack?.singer || '点击任意歌曲开始播放' }}</p>
      </div>
      <button class="close-lyric-btn" @click="playerStore.toggleLyrics" title="收起歌词">
        <SvgIcon name="chevron-down" :size="20" />
      </button>
    </div>

    <div class="lyric-viewport" ref="viewportRef">
      <div v-if="playerStore.lyricLoading" class="lyric-tip">
        <SvgIcon name="music" :size="24" />
        <span>正在加载解密歌词...</span>
      </div>
      <div v-else-if="playerStore.lyricLines.length === 0" class="lyric-tip">
        <SvgIcon name="disc" :size="24" />
        <span>纯音乐或暂无匹配歌词</span>
      </div>
      <div
        v-else
        class="lyric-content"
        :style="{ transform: `translateY(${offsetY}px)` }"
      >
        <div
          v-for="(line, idx) in playerStore.lyricLines"
          :key="idx"
          :class="['lyric-line', { 'is-active': displayIndex === idx }]"
          @click="playerStore.seek(line.timeMs)"
        >
          {{ line.text }}
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch, nextTick, onMounted, onUnmounted } from 'vue';
import { usePlayerStore } from '../stores/player';
import SvgIcon from './SvgIcon.vue';

const playerStore = usePlayerStore();
const viewportRef = ref<HTMLDivElement | null>(null);
const viewportHeight = ref(420);
const displayIndex = ref(0);

let lineTimer: number | null = null;
let anchorPosMs = 0;
let anchorTime = 0;

const LINE_HEIGHT = 44;

const offsetY = computed(() => {
  const activeIdx = Math.max(0, displayIndex.value);
  return viewportHeight.value / 2 - activeIdx * LINE_HEIGHT - LINE_HEIGHT / 2;
});

function clearLineTimer() {
  if (lineTimer !== null) {
    clearTimeout(lineTimer);
    lineTimer = null;
  }
}

function getEstimatedPosMs(): number {
  if (!playerStore.isPlaying || anchorTime <= 0) {
    return anchorPosMs;
  }
  return anchorPosMs + Math.max(0, performance.now() - anchorTime);
}

function scheduleNextLine() {
  clearLineTimer();
  if (!playerStore.showLyrics || !playerStore.isPlaying) {
    return;
  }
  const lines = playerStore.lyricLines;
  const nextIdx = displayIndex.value + 1;
  if (nextIdx < 0 || nextIdx >= lines.length) {
    return;
  }
  const nowPos = getEstimatedPosMs();
  const delay = Math.max(16, lines[nextIdx].timeMs - nowPos);
  lineTimer = window.setTimeout(() => {
    lineTimer = null;
    if (!playerStore.showLyrics || !playerStore.isPlaying) {
      return;
    }
    displayIndex.value = nextIdx;
    scheduleNextLine();
  }, delay);
}

function syncFromSnapshot(allowBackward = false) {
  anchorPosMs = playerStore.positionMs;
  anchorTime = performance.now();

  const lines = playerStore.lyricLines;
  if (lines.length === 0) {
    displayIndex.value = 0;
    clearLineTimer();
    return;
  }

  const snapIdx = Math.max(0, playerStore.currentLyricIndex);
  if (allowBackward) {
    displayIndex.value = snapIdx;
  } else {
    const currIdx = displayIndex.value;
    if (snapIdx >= currIdx) {
      displayIndex.value = snapIdx;
    } else if (currIdx < lines.length && lines[currIdx].timeMs - anchorPosMs > 1000) {
      // 用户向前 Seek 超过 1 秒时允许回退
      displayIndex.value = snapIdx;
    }
  }

  scheduleNextLine();
}

function updateViewportHeight() {
  if (viewportRef.value) {
    viewportHeight.value = viewportRef.value.clientHeight || 420;
  }
}

watch(
  () => playerStore.showLyrics,
  async (shown) => {
    if (shown) {
      await nextTick();
      updateViewportHeight();
      if (playerStore.currentTrack && playerStore.lyricLines.length === 0) {
        playerStore.loadLyricForTrack(playerStore.currentTrack, true);
      }
      syncFromSnapshot(true);
    } else {
      clearLineTimer();
    }
  },
  { immediate: true }
);

watch(
  () => [playerStore.snapshotUpdatedAt, playerStore.isPlaying],
  () => {
    if (playerStore.showLyrics) {
      syncFromSnapshot(false);
    } else {
      clearLineTimer();
    }
  }
);

watch(
  () => playerStore.lyricLines,
  () => {
    syncFromSnapshot(true);
  }
);

watch(
  () => playerStore.currentTrack?.hash,
  () => {
    displayIndex.value = 0;
    clearLineTimer();
    if (playerStore.showLyrics && playerStore.currentTrack) {
      playerStore.loadLyricForTrack(playerStore.currentTrack, true);
    }
  }
);

onMounted(() => {
  window.addEventListener('resize', updateViewportHeight);
});

onUnmounted(() => {
  clearLineTimer();
  window.removeEventListener('resize', updateViewportHeight);
});
</script>

<style scoped>
.lyric-overlay {
  position: absolute;
  left: var(--nav-width);
  top: var(--titlebar-height);
  right: 0;
  bottom: var(--player-height);
  background: var(--bg-app);
  backdrop-filter: blur(36px);
  -webkit-backdrop-filter: blur(36px);
  display: flex;
  flex-direction: column;
  z-index: 85;
  padding: 24px 32px;
}

.lyric-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 16px;
  flex-shrink: 0;
}

.song-meta .title {
  font-size: 20px;
  font-weight: 700;
  max-width: 600px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.song-meta .singer {
  font-size: 13px;
  color: var(--text-secondary);
  margin-top: 4px;
  max-width: 600px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.close-lyric-btn {
  width: 34px;
  height: 34px;
  border-radius: var(--radius-full);
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-secondary);
  background: var(--bg-card);
  border: 1px solid var(--border-subtle);
  transition: all var(--duration-fast);
}

.close-lyric-btn:hover {
  background: var(--bg-card-hover);
  color: var(--text-primary);
}

.lyric-viewport {
  flex: 1;
  overflow: hidden;
  position: relative;
  width: 100%;
}

.lyric-tip {
  position: absolute;
  top: 40%;
  left: 50%;
  transform: translate(-50%, -50%);
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 12px;
  color: var(--text-tertiary);
  font-size: 14px;
}

.lyric-content {
  position: absolute;
  top: 0;
  left: 0;
  width: 100%;
  display: flex;
  flex-direction: column;
  align-items: center;
  transition: transform 320ms cubic-bezier(0.22, 1, 0.36, 1);
  will-change: transform;
}

.lyric-line {
  flex-shrink: 0;
  min-height: 44px;
  height: 44px;
  line-height: 44px;
  font-size: 15px;
  color: var(--text-tertiary);
  cursor: pointer;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  max-width: 80%;
  text-align: center;
  transition: transform 260ms var(--ease-fluent), color 260ms var(--ease-fluent);
  padding: 0 18px;
  border-radius: var(--radius-sm);
  user-select: none;
  will-change: transform;
}

.lyric-line:hover {
  color: var(--text-secondary);
  background: var(--bg-card);
}

.lyric-line.is-active {
  font-weight: 700;
  color: var(--accent-color);
  transform: scale(1.16);
}
</style>