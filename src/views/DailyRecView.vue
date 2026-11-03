<template>
  <div class="view-container">
    <div class="view-header">
      <div class="header-left">
        <h1 class="view-title">每日推荐</h1>
        <p class="view-subtitle">依据你的听歌口味定制 · 每日清晨更新</p>
      </div>
      <div class="header-btns">
        <button class="fluent-btn" @click="loadSongs">
          <SvgIcon name="refresh" :size="14" />
          刷新列表
        </button>
        <button
          class="fluent-btn fluent-btn-primary"
          @click="playAll"
          :disabled="songList.length === 0"
        >
          <SvgIcon name="play" :size="14" />
          播放全部
        </button>
        <button
          class="fluent-btn"
          @click="appendAll"
          :disabled="songList.length === 0"
        >
          <SvgIcon name="playlist-add" :size="14" />
          添加队列
        </button>
      </div>
    </div>

    <div v-if="loading" class="loading-state">正在生成定制推荐曲目...</div>
    <div v-else-if="songList.length === 0" class="loading-state">暂无推荐曲目</div>
    <!-- 超过 20 首由 VirtualSongList 自动启用每页 20 首翻页 -->
    <div v-else class="list-wrapper">
      <VirtualSongList :songs="songList" />
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import VirtualSongList from '../components/VirtualSongList.vue';
import SvgIcon from '../components/SvgIcon.vue';
import { usePlayerStore, type Track } from '../stores/player';
import { extractTracksFromResponse } from '../utils/trackMapper';

const playerStore = usePlayerStore();
const songList = ref<Track[]>([]);
const loading = ref(true);

async function loadSongs() {
  loading.value = true;
  try {
    const res = await invoke<any>('get_daily_recommend');
    songList.value = extractTracksFromResponse(res);
  } catch (err) {
    console.warn('获取每日推荐失败:', err);
  } finally {
    loading.value = false;
  }
}

function playAll() {
  if (songList.value.length > 0) {
    playerStore.setQueue(songList.value, 0);
  }
}

function appendAll() {
  if (songList.value.length > 0) {
    playerStore.appendQueue(songList.value);
  }
}

onMounted(() => {
  loadSongs();
});
</script>

<style scoped>
.view-container {
  width: 100%;
  height: 100%;
  padding: 20px 24px;
  display: flex;
  flex-direction: column;
}

.view-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 16px;
  gap: 12px;
}

.header-btns {
  display: flex;
  align-items: center;
  gap: 8px;
}

.view-title {
  font-size: 22px;
  font-weight: 700;
}

.view-subtitle {
  font-size: 12px;
  color: var(--text-secondary);
  margin-top: 2px;
}

.loading-state {
  margin-top: 80px;
  text-align: center;
  color: var(--text-tertiary);
}

.list-wrapper {
  flex: 1;
  min-height: 0;
}
</style>
