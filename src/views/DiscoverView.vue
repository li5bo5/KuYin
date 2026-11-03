<template>
  <div class="view-container">
    <div class="view-header">
      <div>
        <h1 class="view-title">音乐发现</h1>
        <p class="view-subtitle">私人 FM 智能漫游 · 专属精选推荐</p>
      </div>
    </div>

    <!-- 私人 FM 智能漫游卡片 -->
    <section class="fm-banner">
      <div class="fm-cover-wrap">
        <img
          v-if="displayFmCover && !fmCoverErr"
          :src="displayFmCover"
          alt="FM Cover"
          loading="lazy"
          decoding="async"
          class="fm-cover"
          @error="fmCoverErr = true"
        />
        <div v-else class="fm-cover-fallback">
          <SvgIcon name="radio" :size="30" />
        </div>
      </div>

      <div class="fm-info">
        <div class="fm-tag">
          <SvgIcon name="radio" :size="13" />
          <span>私人 FM 随心漫游</span>
        </div>
        <h3 class="fm-song-title">
          {{ activeFmTrack?.name || '点击右侧按钮开启你的专属私人电台' }}
        </h3>
        <p class="fm-song-artist">
          {{ activeFmTrack?.singer || '不限风格 · 播完自动续拉新歌' }}
        </p>
      </div>

      <div class="fm-controls">
        <button
          class="fluent-btn fluent-btn-primary fm-play-btn"
          :disabled="fmLoading"
          @click="toggleFmPlay"
        >
          <SvgIcon :name="isFmPlaying ? 'pause' : 'play'" :size="15" />
          <span>{{ isFmPlaying ? '暂停漫游' : '开始漫游' }}</span>
        </button>
        <button
          class="fluent-btn fm-icon-btn"
          :disabled="fmLoading"
          @click="nextFmSong"
          title="下一首 FM"
        >
          <SvgIcon name="skip-forward" :size="15" />
        </button>
        <button
          class="fluent-btn fm-icon-btn"
          :disabled="!activeFmTrack"
          @click="likeFmSong"
          title="收藏当前 FM 歌曲"
        >
          <SvgIcon name="heart" :size="15" />
        </button>
        <button
          class="fluent-btn fm-icon-btn"
          :disabled="fmLoading"
          @click="skipDislikeFm"
          title="不喜欢并跳过"
        >
          <SvgIcon name="trash" :size="15" />
        </button>
      </div>
    </section>

    <!-- 歌单发现区域 -->
    <section class="plaza-section">
      <div class="plaza-header">
        <div class="section-title-wrap">
          <SvgIcon name="vinyl" :size="18" />
          <h2 class="section-title">歌单发现</h2>
        </div>
        <button
          class="fluent-btn refresh-btn"
          :disabled="loading"
          @click="refreshActiveSection"
          title="刷新歌单发现"
        >
          <SvgIcon name="refresh" :size="14" />
          <span>刷新列表</span>
        </button>
      </div>

      <div v-if="loading" class="loading-state">正在加载歌单发现...</div>
      <div v-else-if="allCards.length === 0" class="loading-state">
        暂无内容，请点击刷新重试
      </div>

      <div v-else class="card-grid" ref="gridRef">
        <div
          v-for="card in displayCards"
          :key="card.id"
          class="media-card"
          @click="openCard(card)"
        >
          <div class="media-cover-wrap">
            <img
              v-if="card.cover && !brokenCardCovers[card.id]"
              :src="card.cover"
              alt="cover"
              loading="lazy"
              decoding="async"
              class="media-cover"
              @error="brokenCardCovers[card.id] = true"
            />
            <div v-else class="media-fallback">
              <SvgIcon name="vinyl" :size="28" />
            </div>
          </div>
          <div class="media-title" :title="card.name">{{ card.name }}</div>
          <div v-if="card.sub" class="media-sub" :title="card.sub">{{ card.sub }}</div>
        </div>
      </div>

      <!-- 严格每页 3 行换页，显示总页数 -->
      <div v-if="showPager" class="fluent-pagination discover-pager">
        <button
          class="fluent-btn pager-btn"
          :disabled="page <= 1 || loading"
          @click="changePage(page - 1)"
        >
          <SvgIcon name="chevron-left" :size="14" class="pg-icon" />
          <span class="pg-text">上一页</span>
        </button>
        <span class="fluent-page-info">第 {{ page }} / {{ totalPages }} 页</span>
        <button
          class="fluent-btn pager-btn"
          :disabled="page >= totalPages || loading"
          @click="changePage(page + 1)"
        >
          <span class="pg-text">下一页</span>
          <SvgIcon name="chevron-right" :size="14" class="pg-icon" />
        </button>
      </div>
    </section>
  </div>
</template>

<script setup lang="ts">
import { ref, reactive, computed, watch, onMounted, onUnmounted } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import SvgIcon from '../components/SvgIcon.vue';
import { useAppStore } from '../stores/app';
import { usePlayerStore, type Track } from '../stores/player';
import { extractTracksFromResponse, formatSafeCover } from '../utils/trackMapper';

interface MediaCard {
  id: string;
  name: string;
  sub: string;
  cover: string;
  kind: 'playlist';
  totalCount?: number;
}

const appStore = useAppStore();
const playerStore = usePlayerStore();

const gridRef = ref<HTMLElement | null>(null);
const cols = ref(5);
const rows = 3;
const pageSize = computed(() => cols.value * rows);

const allCards = ref<MediaCard[]>([]);
const page = ref(1);
const loading = ref(true);
const brokenCardCovers = reactive<Record<string, boolean>>({});

// 私人 FM 状态
const fmBatch = ref<Track[]>([]);
const lastFmTrack = ref<Track | null>(null);
const fmTrackHashes = new Set<string>();
const fmActiveMode = ref(false);
const fmLoading = ref(false);
const fmCoverErr = ref(false);

let resizeObserver: ResizeObserver | null = null;
let resizeTimer: number | null = null;

function registerFmHashes(tracks: Track[]) {
  for (const t of tracks) {
    if (t.hash) {
      fmTrackHashes.add(t.hash.toLowerCase());
    }
  }
}

const activeFmTrack = computed<Track | null>(() => {
  if (
    fmActiveMode.value &&
    playerStore.currentTrack &&
    fmTrackHashes.has(playerStore.currentTrack.hash.toLowerCase())
  ) {
    return playerStore.currentTrack;
  }
  return lastFmTrack.value || fmBatch.value[0] || null;
});

const displayFmCover = computed(() =>
  formatSafeCover(activeFmTrack.value?.cover_url, 240)
);

const isFmPlaying = computed(
  () =>
    fmActiveMode.value &&
    playerStore.isPlaying &&
    !!playerStore.currentTrack &&
    fmTrackHashes.has(playerStore.currentTrack.hash.toLowerCase())
);

const totalPages = computed(() =>
  Math.max(1, Math.ceil(allCards.value.length / pageSize.value))
);

const displayCards = computed(() => {
  const start = (page.value - 1) * pageSize.value;
  return allCards.value.slice(start, start + pageSize.value);
});

const showPager = computed(() => totalPages.value > 1);

watch(displayFmCover, () => {
  fmCoverErr.value = false;
});

// 监听全局当前曲目：若切到非私人 FM 曲目（如在歌单/排行榜点歌），立即退出漫游状态，按钮自动恢复为“开始漫游”
watch(
  () => playerStore.currentTrack,
  (curr) => {
    if (!curr) return;
    if (fmTrackHashes.has(curr.hash.toLowerCase())) {
      lastFmTrack.value = curr;
    } else if (fmActiveMode.value) {
      fmActiveMode.value = false;
    }
  }
);

// 当私人 FM 播放至当前批次末尾时，自动续拉下一批漫游曲目
watch(
  () => [playerStore.currentIndex, playerStore.queueCount],
  async ([idx, cnt]) => {
    if (
      fmActiveMode.value &&
      typeof idx === 'number' &&
      typeof cnt === 'number' &&
      cnt > 0 &&
      idx >= cnt - 1 &&
      !fmLoading.value
    ) {
      await appendMoreFmTracks();
    }
  }
);

function updateGridCols() {
  const width = gridRef.value?.clientWidth || 700;
  // 单个卡片最小宽度 140px，间隙 14px
  const calculatedCols = Math.max(3, Math.floor((width + 14) / (140 + 14)));
  if (calculatedCols !== cols.value) {
    cols.value = calculatedCols;
    if (page.value > totalPages.value) {
      page.value = totalPages.value;
    }
  }
}

async function fetchFmTracks(autoPlay = false) {
  fmLoading.value = true;
  try {
    const res = await invoke<any>('get_personal_fm');
    const list = extractTracksFromResponse(res);
    if (list.length > 0) {
      fmBatch.value = list;
      registerFmHashes(list);
      if (!lastFmTrack.value) {
        lastFmTrack.value = list[0];
      }
      if (autoPlay) {
        fmActiveMode.value = true;
        lastFmTrack.value = list[0];
        await playerStore.setQueue(list, 0);
      }
    } else if (autoPlay) {
      playerStore.showToast('暂未获取到 FM 推荐曲目，请稍后重试');
    }
  } catch (err: any) {
    if (autoPlay) {
      playerStore.showToast(`加载私人 FM 失败：${String(err)}`);
    }
  } finally {
    fmLoading.value = false;
  }
}

async function appendMoreFmTracks() {
  fmLoading.value = true;
  try {
    const res = await invoke<any>('get_personal_fm');
    const more = extractTracksFromResponse(res);
    const slice = more.slice(0, 5);
    registerFmHashes(slice);
    if (slice.length > 0) {
      await invoke('append_queue', { tracks: slice });
    }
    await playerStore.fetchSnapshot();
  } catch {
    // 忽略静默续拉单次失败
  } finally {
    fmLoading.value = false;
  }
}

async function toggleFmPlay() {
  if (isFmPlaying.value) {
    await playerStore.togglePlay();
    return;
  }
  if (
    fmActiveMode.value &&
    playerStore.currentTrack &&
    fmTrackHashes.has(playerStore.currentTrack.hash.toLowerCase())
  ) {
    await playerStore.togglePlay();
    return;
  }
  if (fmBatch.value.length > 0) {
    registerFmHashes(fmBatch.value);
    lastFmTrack.value = fmBatch.value[0];
    fmActiveMode.value = true;
    await playerStore.setQueue(fmBatch.value, 0);
  } else {
    await fetchFmTracks(true);
  }
}

async function nextFmSong() {
  if (
    !fmActiveMode.value ||
    playerStore.queueCount <= 1 ||
    !playerStore.currentTrack ||
    !fmTrackHashes.has(playerStore.currentTrack.hash.toLowerCase())
  ) {
    await fetchFmTracks(true);
  } else {
    await playerStore.next();
  }
}

async function skipDislikeFm() {
  playerStore.showToast('已跳过该曲目，为你切换下一首漫游推荐');
  await nextFmSong();
}

async function likeFmSong() {
  if (!activeFmTrack.value) return;
  try {
    const msg = await appStore.favoriteTrackToDefault(activeFmTrack.value);
    playerStore.showToast(msg);
  } catch (err: any) {
    playerStore.showToast(err?.message || String(err));
  }
}

async function loadSectionData(forceRefresh = false) {
  loading.value = true;
  try {
    const res = await invoke<any>('get_top_playlists', {
      page: 1,
      pagesize: 30,
      refresh: forceRefresh,
    });
    const rawList: any[] = res?.data?.special_list || res?.data?.list || [];
    const mapped: MediaCard[] = rawList
      .map((p: any) => {
        const cnt = Number(p.song_count ?? p.count ?? p.total ?? 0);
        return {
          id: String(p.global_collection_id || p.specialid || ''),
          name: String(p.specialname || p.name || '精选歌单'),
          sub: String(p.nickname || p.intro || ''),
          cover: formatSafeCover(p.flexible_cover || p.imgurl || p.cover, 240),
          kind: 'playlist' as const,
          totalCount: Number.isFinite(cnt) && cnt > 0 ? cnt : undefined,
        };
      })
      .filter((p) => !!p.id);

    allCards.value = mapped;
    if (page.value > totalPages.value) {
      page.value = 1;
    }
  } catch (err) {
    console.warn('加载歌单发现失败:', err);
    allCards.value = [];
  } finally {
    loading.value = false;
  }
}

function changePage(target: number) {
  if (target < 1 || target > totalPages.value) return;
  page.value = target;
}

function refreshActiveSection() {
  page.value = 1;
  fetchFmTracks(false);
  loadSectionData(true);
}

function openCard(card: MediaCard) {
  appStore.openPlaylistDetail({
    id: card.id,
    name: card.name,
    cover: card.cover,
    intro: card.sub,
    kind: card.kind,
    isUserList: false,
    totalCount: card.totalCount,
  });
}

onMounted(() => {
  fetchFmTracks(false);
  loadSectionData();

  if (typeof ResizeObserver !== 'undefined') {
    resizeObserver = new ResizeObserver(() => {
      if (resizeTimer) window.clearTimeout(resizeTimer);
      resizeTimer = window.setTimeout(updateGridCols, 120);
    });
    if (gridRef.value) {
      resizeObserver.observe(gridRef.value);
    }
  }
});

onUnmounted(() => {
  if (resizeObserver) {
    resizeObserver.disconnect();
    resizeObserver = null;
  }
  if (resizeTimer) {
    window.clearTimeout(resizeTimer);
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
  gap: 16px;
  overflow-y: auto;
}

.view-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  flex-shrink: 0;
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

.fm-banner {
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 14px 18px;
  background: linear-gradient(135deg, rgba(0, 103, 192, 0.1), rgba(31, 217, 184, 0.1));
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-lg);
  flex-shrink: 0;
}

.fm-cover-wrap {
  width: 72px;
  height: 72px;
  border-radius: var(--radius-md);
  overflow: hidden;
  background: var(--bg-card);
  border: 1px solid var(--border-subtle);
  flex-shrink: 0;
}

.fm-cover {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.fm-cover-fallback {
  width: 100%;
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--accent-color);
}

.fm-info {
  flex: 1;
  min-width: 0;
}

.fm-tag {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  font-size: 11px;
  font-weight: 600;
  color: var(--accent-color);
  margin-bottom: 4px;
}

.fm-song-title {
  font-size: 15px;
  font-weight: 700;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.fm-song-artist {
  font-size: 12px;
  color: var(--text-secondary);
  margin-top: 2px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.fm-controls {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}

.fm-icon-btn {
  width: 32px;
  height: 32px;
  padding: 0;
}

.plaza-section {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.plaza-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.section-title-wrap {
  display: flex;
  align-items: center;
  gap: 8px;
  color: var(--text-primary);
}

.section-title {
  font-size: 16px;
  font-weight: 600;
}

.loading-state {
  margin-top: 48px;
  text-align: center;
  color: var(--text-tertiary);
}

.card-grid {
  display: grid;
  grid-template-columns: repeat(v-bind(cols), minmax(0, 1fr));
  gap: 14px;
}

.media-card {
  padding: 10px;
  background: var(--bg-card);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-md);
  cursor: pointer;
  transition: transform var(--duration-fast), background-color var(--duration-fast);
}

.media-card:hover {
  background: var(--bg-card-hover);
  transform: translateY(-2px);
}

.media-cover-wrap {
  width: 100%;
  aspect-ratio: 1 / 1;
  border-radius: var(--radius-sm);
  overflow: hidden;
  background: var(--bg-app);
  margin-bottom: 8px;
}

.media-cover {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.media-fallback {
  width: 100%;
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-tertiary);
}

.media-title {
  font-size: 12.5px;
  font-weight: 600;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.media-sub {
  font-size: 11px;
  color: var(--text-tertiary);
  margin-top: 2px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.discover-pager {
  padding: 8px 0 4px;
}

.pager-btn {
  padding: 4px 12px;
  font-size: 12px;
}
</style>