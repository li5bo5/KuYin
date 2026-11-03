<template>
  <div class="view-container">
    <div class="view-header">
      <div>
        <h1 class="view-title">排行榜</h1>
        <p class="view-subtitle">酷狗官方热门榜单</p>
      </div>
      <div class="header-btns">
        <button
          class="fluent-btn"
          :disabled="loading || songsLoading"
          @click="syncRankings"
        >
          <SvgIcon name="refresh" :size="14" />
          同步榜单
        </button>
        <button
          class="fluent-btn fluent-btn-primary"
          :disabled="!currentRankId || playAllLoading || addQueueLoading"
          @click="playWholeRank"
        >
          <SvgIcon name="play" :size="14" />
          {{ playAllLoading ? '加载之中' : '播放全部' }}
        </button>
        <button
          class="fluent-btn"
          :disabled="!currentRankId || playAllLoading || addQueueLoading"
          @click="appendWholeRank"
        >
          <SvgIcon name="playlist-add" :size="14" />
          {{ addQueueLoading ? '添加之中' : '添加队列' }}
        </button>
      </div>
    </div>

    <div v-if="loading" class="loading-state">正在拉取排行榜列表...</div>

    <div v-else class="rank-layout">
      <!-- 左侧榜单选择器（全部平铺可滚动） -->
      <div class="rank-sidebar">
        <div class="rank-selector">
          <button
            v-for="rank in rankList"
            :key="rank.id"
            :class="['rank-item-btn', { active: currentRankId === rank.id }]"
            @click="selectRank(rank.id)"
          >
            <SvgIcon name="trophy" :size="14" class="rank-icon" />
            <span class="rank-name">{{ rank.name }}</span>
          </button>
        </div>
      </div>

      <!-- 右侧榜单歌曲（每页 20 首翻页） -->
      <div class="rank-songs-area">
        <div v-if="songsLoading" class="loading-state">正在加载榜单曲目...</div>
        <div v-else-if="songs.length === 0" class="loading-state">暂无榜单曲目</div>
        <VirtualSongList
          v-else
          :songs="songs"
          :server-page="songPage"
          :server-total="songTotal"
          :server-has-more="songHasMore"
          @page-change="handleSongPageChange"
        />
      </div>
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

interface RankInfo {
  id: string;
  name: string;
}

const PAGE_SIZE = 20;

const playerStore = usePlayerStore();
const rankList = ref<RankInfo[]>([]);

const currentRankId = ref<string>('');
const songs = ref<Track[]>([]);
const songPage = ref(1);
const songTotal = ref(0);
const songHasMore = ref(false);
const loading = ref(true);
const songsLoading = ref(false);
const playAllLoading = ref(false);
const addQueueLoading = ref(false);

async function loadRanks(forceRefresh = false) {
  loading.value = true;
  try {
    const res = await invoke<any>('get_rankings', { refresh: forceRefresh });
    const items = res?.data?.info || res?.data || [];
    rankList.value = items
      .map((r: any) => ({
        id: String(r.rankid || r.id || ''),
        name: String(r.rankname || r.name || '排行榜'),
      }))
      .filter((r: RankInfo) => !!r.id);

    if (rankList.value.length > 0) {
      const targetId =
        currentRankId.value && rankList.value.some((r) => r.id === currentRankId.value)
          ? currentRankId.value
          : rankList.value[0].id;
      selectRank(targetId);
    }
  } catch (err) {
    console.warn('获取排行榜失败:', err);
  } finally {
    loading.value = false;
  }
}

async function syncRankings() {
  await loadRanks(true);
  playerStore.showToast('已同步酷狗官方最新热门榜单');
}

async function fetchRankSongs() {
  if (!currentRankId.value) return;
  songsLoading.value = true;
  try {
    const res = await invoke<any>('get_ranking_songs', {
      rankId: currentRankId.value,
      page: songPage.value,
      pagesize: PAGE_SIZE,
    });
    const list = extractTracksFromResponse(res);
    const d = res?.data ?? res;
    const total = Number(d?.total ?? d?.count ?? d?.total_count ?? res?.total ?? 0);
    songs.value = list.slice(0, PAGE_SIZE);
    songTotal.value = Number.isFinite(total) && total > 0 ? total : 0;
    songHasMore.value =
      songTotal.value > 0
        ? songPage.value * PAGE_SIZE < songTotal.value
        : list.length >= PAGE_SIZE;
  } catch (err) {
    console.warn('拉取榜单歌曲失败:', err);
    songs.value = [];
    songTotal.value = 0;
    songHasMore.value = false;
  } finally {
    songsLoading.value = false;
  }
}

function selectRank(rankId: string) {
  currentRankId.value = rankId;
  songPage.value = 1;
  songTotal.value = 0;
  fetchRankSongs();
}

function handleSongPageChange(target: number) {
  songPage.value = target;
  fetchRankSongs();
}

async function fetchAllRankSongs(rankId: string): Promise<Track[]> {
  const all: Track[] = [];
  const seen = new Set<string>();
  for (let p = 1; p <= 3; p++) {
    const res = await invoke<any>('get_ranking_songs', { rankId, page: p, pagesize: 100 });
    const list = extractTracksFromResponse(res);
    for (const t of list) {
      if (!seen.has(t.hash)) {
        seen.add(t.hash);
        all.push(t);
      }
    }
    if (list.length < 100) break;
  }
  return all;
}

// 播放全部：按每页 100 首最多拉取 3 页汇总去重后替换播放队列并从第一首开始播放
async function playWholeRank() {
  if (!currentRankId.value) return;
  const rankId = currentRankId.value;
  playAllLoading.value = true;
  try {
    const all = await fetchAllRankSongs(rankId);
    if (currentRankId.value !== rankId) return;
    if (all.length > 0) {
      await playerStore.setQueue(all, 0);
    } else {
      playerStore.showToast('未获取到榜单曲目');
    }
  } catch (err) {
    playerStore.showToast(`加载榜单全部曲目失败：${String(err)}`);
  } finally {
    playAllLoading.value = false;
  }
}

// 添加队列：将当前榜单全部歌曲追加到播放队列末尾，不切歌；提示“已添加 N 首”
async function appendWholeRank() {
  if (!currentRankId.value) return;
  const rankId = currentRankId.value;
  addQueueLoading.value = true;
  try {
    const all = await fetchAllRankSongs(rankId);
    if (currentRankId.value !== rankId) return;
    if (all.length > 0) {
      await playerStore.appendQueue(all);
    } else {
      playerStore.showToast('未获取到榜单曲目');
    }
  } catch (err) {
    playerStore.showToast(`加载榜单全部曲目失败：${String(err)}`);
  } finally {
    addQueueLoading.value = false;
  }
}

onMounted(() => {
  loadRanks();
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
  margin-bottom: 14px;
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

.rank-layout {
  flex: 1;
  display: flex;
  gap: 16px;
  min-height: 0;
}

.rank-sidebar {
  flex: 0 0 25vw;
  width: 25vw;
  min-width: 25vw;
  max-width: 25vw;
  display: flex;
  flex-direction: column;
  border-right: 1px solid var(--border-divider);
  padding-right: 8px;
}

.rank-selector {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 4px;
  overflow-y: auto;
}

.rank-item-btn {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 10px;
  border-radius: var(--radius-md);
  text-align: left;
  font-size: 12.5px;
  color: var(--text-secondary);
  transition: all var(--duration-fast);
}

.rank-item-btn:hover {
  background: var(--bg-card-hover);
  color: var(--text-primary);
}

.rank-item-btn.active {
  background: var(--bg-card);
  color: var(--accent-color);
  font-weight: 600;
  border: 1px solid var(--border-subtle);
}

.rank-icon {
  flex-shrink: 0;
  opacity: 0.75;
}

.rank-name {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.rank-mini-pager {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding-top: 8px;
  border-top: 1px solid var(--border-divider);
}

.mini-pg-btn {
  padding: 3px 8px;
}

.mini-pg-text {
  font-size: 11.5px;
  color: var(--text-secondary);
}

.rank-songs-area {
  flex: 1;
  min-height: 0;
}

.loading-state {
  margin-top: 80px;
  text-align: center;
  color: var(--text-tertiary);
}
</style>
