<template>
  <div class="view-container">
    <div class="view-header">
      <div class="header-left">
        <h1 class="view-title">播放历史</h1>
        <p class="view-subtitle">本地最近播放记录（上限自动保留最新 200 首）</p>
      </div>
      <div class="header-actions">
        <button class="fluent-btn" @click="loadHistory">
          <SvgIcon name="refresh" :size="14" />
          刷新列表
        </button>
        <button
          class="fluent-btn"
          :disabled="historySongs.length === 0"
          @click="handleClearHistory"
        >
          <SvgIcon name="trash" :size="14" />
          清空历史
        </button>
        <button
          class="fluent-btn fluent-btn-primary"
          @click="playAllHistory"
          :disabled="historySongs.length === 0"
        >
          <SvgIcon name="play" :size="14" />
          播放全部
        </button>
        <button
          class="fluent-btn"
          @click="appendAllHistory"
          :disabled="historySongs.length === 0"
        >
          <SvgIcon name="playlist-add" :size="14" />
          添加队列
        </button>
      </div>
    </div>

    <div v-if="loading" class="loading-state">
      <p>正在读取本地播放历史...</p>
    </div>

    <div v-else-if="historySongs.length === 0" class="empty-state">
      <p>暂无播放历史，去「发现」或「推荐」听听好歌吧</p>
    </div>

    <!-- 超过 20 首由 VirtualSongList 自动启用每页 20 首翻页 -->
    <div v-else class="list-wrapper">
      <VirtualSongList :songs="historySongs" />
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, watch, onMounted } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import VirtualSongList from '../components/VirtualSongList.vue';
import SvgIcon from '../components/SvgIcon.vue';
import { useAppStore } from '../stores/app';
import { usePlayerStore, type Track } from '../stores/player';
import { formatSafeCover } from '../utils/trackMapper';

const appStore = useAppStore();
const playerStore = usePlayerStore();
const historySongs = ref<Track[]>([]);
const loading = ref(true);

async function loadHistory() {
  loading.value = true;
  try {
    const list = await invoke<any[]>('get_history', { limit: 200 });
    historySongs.value = list.map((item: any) => ({
      hash: item.hash,
      name: item.name,
      singer: item.singer,
      album_id: item.album_id ?? undefined,
      album_audio_id: item.album_audio_id ?? undefined,
      duration: item.duration || 180,
      cover_url: item.cover_url ? formatSafeCover(item.cover_url, 100) : undefined,
    }));
  } catch (err) {
    console.warn('获取播放历史失败:', err);
  } finally {
    loading.value = false;
  }
}

async function handleClearHistory() {
  try {
    await invoke('clear_history');
    historySongs.value = [];
    playerStore.showToast('本地播放历史已清空');
  } catch (err: any) {
    playerStore.showToast(`清空失败：${String(err)}`);
  }
}

function playAllHistory() {
  if (historySongs.value.length > 0) {
    playerStore.setQueue(historySongs.value, 0);
  }
}

function appendAllHistory() {
  if (historySongs.value.length > 0) {
    playerStore.appendQueue(historySongs.value);
  }
}

watch(
  () => appStore.currentTab,
  (tab) => {
    if (tab === 'history') {
      loadHistory();
    }
  }
);

onMounted(() => {
  loadHistory();
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
}

.header-actions {
  display: flex;
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

.loading-state,
.empty-state {
  margin-top: 80px;
  text-align: center;
  color: var(--text-tertiary);
}

.list-wrapper {
  flex: 1;
  min-height: 0;
}
</style>
