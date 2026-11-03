<template>
  <div class="view-container">
    <div class="top-bar">
      <div class="left-group">
        <button class="fluent-btn fluent-btn-auto" @click="appStore.activePlaylist = null">
          <SvgIcon name="chevron-left" :size="15" />
          返回
        </button>
      </div>
    </div>

    <template v-if="appStore.activePlaylist">
      <div class="playlist-hero">
        <div class="hero-cover-box">
          <img
            v-if="safeHeroCover && !heroCoverErr"
            :src="safeHeroCover"
            alt="cover"
            loading="lazy"
            decoding="async"
            class="hero-cover"
            @error="heroCoverErr = true"
          />
          <div v-else class="hero-placeholder">
            <SvgIcon name="vinyl" :size="32" />
          </div>
        </div>
        <div class="hero-meta">
          <h2 class="hero-name">{{ appStore.activePlaylist.name }}</h2>
          <p class="hero-desc">
            {{ appStore.activePlaylist.intro || `本页 ${filteredSongs.length} 首曲目` }}
          </p>
          <div class="hero-actions">
            <button
              class="fluent-btn fluent-btn-primary"
              :disabled="filteredSongs.length === 0 || playAllLoading || addQueueLoading"
              @click="playAll"
            >
              <SvgIcon name="play" :size="14" />
              {{ playAllLoading ? '加载之中' : '播放全部' }}
            </button>

            <button
              class="fluent-btn"
              :disabled="filteredSongs.length === 0 || playAllLoading || addQueueLoading"
              @click="appendAllToQueue"
            >
              <SvgIcon name="playlist-add" :size="14" />
              {{ addQueueLoading ? '添加之中' : '添加队列' }}
            </button>

            <button
              v-if="canCollectExternalPlaylist"
              class="fluent-btn"
              @click="handleCollectExternal"
            >
              <SvgIcon name="heart" :size="14" />
              收藏歌单
            </button>

            <input
              v-model="filterText"
              class="fluent-input filter-input"
              placeholder="快速筛选歌名/歌手..."
            />
          </div>
        </div>
      </div>

      <div v-if="detailLoading" class="loading-state">正在加载曲目列表...</div>
      <div v-else-if="filteredSongs.length === 0" class="loading-state">暂无匹配曲目</div>
      <div v-else class="list-wrapper">
        <VirtualSongList
          v-if="isUserCloudDetail"
          :songs="filteredSongs"
          :removable-list-id="appStore.activePlaylist.listid"
          @song-removed="handleSongRemoved"
        />
        <VirtualSongList
          v-else
          :songs="filteredSongs"
          :server-page="detailPage"
          :server-total="filterText.trim() ? 0 : detailTotal"
          :server-has-more="detailHasMore && !filterText.trim()"
          @page-change="handleDetailPageChange"
        />
      </div>
    </template>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch, onMounted } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import VirtualSongList from '../components/VirtualSongList.vue';
import SvgIcon from '../components/SvgIcon.vue';
import { useAppStore } from '../stores/app';
import { usePlayerStore, type Track } from '../stores/player';
import { extractTracksFromResponse, formatSafeCover } from '../utils/trackMapper';

const PAGE_SIZE = 20;

const appStore = useAppStore();
const playerStore = usePlayerStore();

const playlistSongs = ref<Track[]>([]);
const detailPage = ref(1);
const detailTotal = ref(0);
const detailHasMore = ref(false);
const filterText = ref('');
const detailLoading = ref(false);
const playAllLoading = ref(false);
const addQueueLoading = ref(false);
const heroCoverErr = ref(false);

const safeHeroCover = computed(() =>
  formatSafeCover(appStore.activePlaylist?.cover, 240)
);

const isUserCloudDetail = computed(
  () => !!(appStore.activePlaylist?.isUserList && appStore.activePlaylist?.listid)
);

const canCollectExternalPlaylist = computed(() => {
  const meta = appStore.activePlaylist;
  return (
    meta &&
    !meta.isUserList &&
    (meta.kind === 'playlist' || !meta.kind) &&
    !!meta.id
  );
});

const filteredSongs = computed(() => {
  const kw = filterText.value.trim().toLowerCase();
  if (!kw) return playlistSongs.value;
  return playlistSongs.value.filter(
    (s) =>
      s.name.toLowerCase().includes(kw) || s.singer.toLowerCase().includes(kw)
  );
});

function extractTotalCount(res: any): number {
  const songsData = res?.songs?.data ?? res?.songs;
  const infoItem = Array.isArray(res?.info?.data)
    ? res.info.data[0]
    : (res?.info?.data ?? res?.info);
  const d = res?.data ?? res;
  const candidates = [
    songsData?.count,
    songsData?.total,
    songsData?.total_count,
    infoItem?.count,
    infoItem?.song_count,
    infoItem?.total,
    d?.total,
    d?.count,
    d?.total_count,
    res?.total,
    appStore.activePlaylist?.totalCount,
  ];
  for (const val of candidates) {
    const n = Number(val);
    if (Number.isFinite(n) && n > 0) return n;
  }
  return 0;
}

async function handleCollectExternal() {
  const meta = appStore.activePlaylist;
  if (!meta) return;
  if (!appStore.authProfile.is_logged_in) {
    appStore.showLoginModal = true;
    playerStore.showToast('请先登录账号后再收藏歌单');
    return;
  }
  try {
    await invoke('create_or_collect_playlist', {
      name: meta.name,
      listType: 1,
      listCreateUserid: meta.createUserId ?? null,
      listCreateListid: meta.listid ?? null,
      listCreateGid: meta.id,
    });
    playerStore.showToast(`已收藏歌单「${meta.name}」`);
    await appStore.loadUserPlaylists();
  } catch (err: any) {
    playerStore.showToast(`收藏歌单失败：${String(err)}`);
  }
}

async function loadActivePlaylistTracks() {
  const meta = appStore.activePlaylist;
  if (!meta) return;
  detailLoading.value = true;
  heroCoverErr.value = false;
  try {
    if (meta.isUserList && meta.listid) {
      const res = await invoke<any>('get_user_playlist_tracks', {
        listid: meta.listid,
        page: 1,
        pagesize: 300,
      });
      playlistSongs.value = extractTracksFromResponse(res);
      detailTotal.value = 0;
      detailHasMore.value = false;
    } else if (meta.kind === 'artist' && meta.id) {
      const res = await invoke<any>('get_artist_songs', {
        authorId: meta.id,
        page: detailPage.value,
        pagesize: PAGE_SIZE,
      });
      const list = extractTracksFromResponse(res);
      const total = extractTotalCount(res);
      playlistSongs.value = list;
      detailTotal.value = total;
      detailHasMore.value = total > 0 ? detailPage.value * PAGE_SIZE < total : list.length >= PAGE_SIZE;
    } else if (meta.kind === 'album' && meta.id) {
      const res = await invoke<any>('get_album_songs', {
        albumId: meta.id,
        page: detailPage.value,
        pagesize: PAGE_SIZE,
      });
      const list = extractTracksFromResponse(res);
      const total = extractTotalCount(res);
      playlistSongs.value = list;
      detailTotal.value = total;
      detailHasMore.value = total > 0 ? detailPage.value * PAGE_SIZE < total : list.length >= PAGE_SIZE;
    } else if (meta.id) {
      const res = await invoke<any>('get_playlist_detail', {
        id: meta.id,
        page: detailPage.value,
        pagesize: PAGE_SIZE,
      });
      const list = extractTracksFromResponse(res);
      const total = extractTotalCount(res);
      playlistSongs.value = list;
      detailTotal.value = total;
      detailHasMore.value = total > 0 ? detailPage.value * PAGE_SIZE < total : list.length >= PAGE_SIZE;
    }
  } catch (err) {
    console.warn('加载曲目列表失败:', err);
    playlistSongs.value = [];
    detailTotal.value = 0;
    detailHasMore.value = false;
  } finally {
    detailLoading.value = false;
  }
}

function handleDetailPageChange(targetPage: number) {
  detailPage.value = targetPage;
  loadActivePlaylistTracks();
}

function handleSongRemoved(hash: string) {
  playlistSongs.value = playlistSongs.value.filter((s) => s.hash !== hash);
  appStore.loadUserPlaylists();
}

async function resolveAllCurrentTracks(): Promise<Track[]> {
  const meta = appStore.activePlaylist;
  if (!meta || filteredSongs.value.length === 0) return [];

  if (
    filterText.value.trim() ||
    meta.isUserList ||
    (!detailHasMore.value && detailPage.value === 1)
  ) {
    return filteredSongs.value;
  }

  const all: Track[] = [];
  const seen = new Set<string>();
  for (let p = 1; p <= 3; p++) {
    let res: any;
    if (meta.kind === 'artist' && meta.id) {
      res = await invoke<any>('get_artist_songs', {
        authorId: meta.id,
        page: p,
        pagesize: 100,
      });
    } else if (meta.kind === 'album' && meta.id) {
      res = await invoke<any>('get_album_songs', {
        albumId: meta.id,
        page: p,
        pagesize: 100,
      });
    } else if (meta.id) {
      res = await invoke<any>('get_playlist_detail', {
        id: meta.id,
        page: p,
        pagesize: 100,
      });
    }
    const list = extractTracksFromResponse(res);
    for (const t of list) {
      if (!seen.has(t.hash)) {
        seen.add(t.hash);
        all.push(t);
      }
    }
    if (list.length < 100) break;
  }

  return all.length > 0 ? all : filteredSongs.value;
}

async function playAll() {
  if (!appStore.activePlaylist || filteredSongs.value.length === 0) return;
  playAllLoading.value = true;
  try {
    const finalQueue = await resolveAllCurrentTracks();
    if (finalQueue.length > 0) {
      await playerStore.setQueue(finalQueue, 0);
    }
  } catch {
    await playerStore.setQueue(filteredSongs.value, 0);
  } finally {
    playAllLoading.value = false;
  }
}

async function appendAllToQueue() {
  if (!appStore.activePlaylist || filteredSongs.value.length === 0) return;
  addQueueLoading.value = true;
  try {
    const finalQueue = await resolveAllCurrentTracks();
    if (finalQueue.length > 0) {
      await playerStore.appendQueue(finalQueue);
    }
  } catch {
    await playerStore.appendQueue(filteredSongs.value);
  } finally {
    addQueueLoading.value = false;
  }
}

watch(
  () => appStore.activePlaylist,
  (val) => {
    if (val) {
      detailPage.value = 1;
      detailTotal.value = 0;
      filterText.value = '';
      loadActivePlaylistTracks();
    }
  },
  { deep: true }
);

onMounted(() => {
  if (appStore.activePlaylist) {
    loadActivePlaylistTracks();
  }
});
</script>

<style scoped>
.view-container {
  width: 100%;
  height: 100%;
  padding: 20px 24px;
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.top-bar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  flex-shrink: 0;
}

.playlist-hero {
  display: flex;
  gap: 16px;
  align-items: center;
  padding: 12px 16px;
  background: var(--bg-card);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-lg);
  flex-shrink: 0;
}

.hero-cover-box {
  width: 82px;
  height: 82px;
  border-radius: var(--radius-md);
  overflow: hidden;
  flex-shrink: 0;
  background: var(--bg-app);
  border: 1px solid var(--border-subtle);
}

.hero-cover {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.hero-placeholder {
  width: 100%;
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-tertiary);
}

.hero-meta {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.hero-name {
  font-size: 17px;
  font-weight: 700;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.hero-desc {
  font-size: 12px;
  color: var(--text-secondary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.hero-actions {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

.filter-input {
  max-width: 220px;
  height: 30px;
  font-size: 12px;
}

.list-wrapper {
  flex: 1;
  min-height: 0;
}

.loading-state {
  margin-top: 64px;
  text-align: center;
  color: var(--text-tertiary);
}
</style>
