<template>
  <div class="song-list-shell" ref="shellRef">
    <!-- Ctrl+F 唤出的轻量列表内过滤栏 -->
    <div v-if="showFilterBar" class="inline-filter-bar">
      <SvgIcon name="search" :size="14" class="filter-icon" />
      <input
        ref="filterInputRef"
        v-model="localFilter"
        class="fluent-input filter-box"
        placeholder="在当前列表中筛选歌名或歌手 (Esc 关闭)..."
        @keydown.esc="closeFilterBar"
      />
      <button class="filter-close" @click="closeFilterBar" title="关闭筛选">
        <SvgIcon name="close" :size="14" />
      </button>
    </div>

    <!-- 定高 56px 歌曲列表视口（每页最多 20 首，挂载 DOM 节点恒定 ≤ 20 个，严守 ≤ 25 红线） -->
    <div class="virtual-list-container" ref="containerRef" @scroll="closeContextMenu">
      <div v-if="displaySongs.length === 0" class="empty-tip">
        暂无匹配曲目
      </div>
      <div
        v-for="(song, idx) in displaySongs"
        :key="`${song.hash}-${baseIndex + idx}`"
        :class="['song-row', { 'is-active': isCurrentSong(song) }]"
        @dblclick="handlePlaySong(idx)"
        @contextmenu.prevent="openContextMenu($event, song, idx)"
      >
        <span class="col-index">{{ baseIndex + idx + 1 }}</span>

        <!-- 架构红线（五.2）：强制替换 {size} 为 100 缩略图，携带 lazy/async 与 @error 占位兜底 -->
        <div class="col-cover">
          <img
            v-if="song.cover_url && !brokenCovers[song.hash]"
            :src="getSafeCover(song.cover_url)"
            alt="cover"
            loading="lazy"
            decoding="async"
            class="song-cover"
            @error="brokenCovers[song.hash] = true"
          />
          <div v-else class="song-cover-placeholder">
            <SvgIcon name="music" :size="16" />
          </div>
        </div>

        <div class="col-info">
          <div class="song-title" :title="song.name">{{ song.name }}</div>
          <div class="song-artist" :title="song.singer">{{ song.singer }}</div>
        </div>

        <div class="col-actions">
          <button
            class="action-btn"
            @click.stop="handlePlaySong(idx)"
            title="立即播放"
          >
            <SvgIcon name="play" :size="13" />
          </button>
          <button
            class="action-btn"
            @click.stop="playerStore.insertNext(song)"
            title="下一首播放"
          >
            <SvgIcon name="plus" :size="14" />
          </button>
          <button
            class="action-btn"
            @click.stop="playerStore.appendSong(song)"
            title="添加到队列"
          >
            <SvgIcon name="playlist-add" :size="14" />
          </button>
          <button
            class="action-btn"
            @click.stop="handleQuickLike(song)"
            title="收藏到默认歌单"
          >
            <SvgIcon name="heart" :size="14" />
          </button>
        </div>

        <span class="col-duration">{{ formatDuration(song.duration) }}</span>
      </div>
    </div>

    <!-- 全局 >20 自动翻页栏（支持本地分页与服务端分页） -->
    <div v-if="showPagination" class="fluent-pagination list-pager">
      <button
        class="fluent-btn pager-btn"
        :disabled="currentPage <= 1"
        @click="changePage(currentPage - 1)"
      >
        <SvgIcon name="chevron-left" :size="14" class="pg-icon" />
        <span class="pg-text">上一页</span>
      </button>

      <span class="fluent-page-info">
        <template v-if="totalPages > 0">
          第 {{ currentPage }} / {{ totalPages }} 页
        </template>
        <template v-else>
          第 {{ currentPage }} 页
        </template>
      </span>

      <button
        class="fluent-btn pager-btn"
        :disabled="!canGoNext"
        @click="changePage(currentPage + 1)"
      >
        <span class="pg-text">下一页</span>
        <SvgIcon name="chevron-right" :size="14" class="pg-icon" />
      </button>
    </div>

    <!-- 轻量右键上下文菜单 -->
    <div
      v-if="ctxMenu.visible && ctxMenu.song"
      class="ctx-menu"
      :style="{ left: ctxMenu.x + 'px', top: ctxMenu.y + 'px' }"
      @mousedown.stop
    >
      <button class="ctx-item" @click="ctxPlayNow">
        <SvgIcon name="play" :size="14" />
        <span>立即播放</span>
      </button>
      <button class="ctx-item" @click="ctxPlayNext">
        <SvgIcon name="plus" :size="14" />
        <span>下一首播放</span>
      </button>
      <button class="ctx-item" @click="ctxAppendQueue">
        <SvgIcon name="playlist-add" :size="14" />
        <span>添加到队列</span>
      </button>
      <button class="ctx-item" @click="ctxLikeDefault">
        <SvgIcon name="heart" :size="14" />
        <span>收藏到默认歌单</span>
      </button>

      <template v-if="appStore.userPlaylists.length > 0">
        <div class="ctx-divider"></div>
        <div class="ctx-group-label">添加到我的歌单</div>
        <button
          v-for="pl in appStore.userPlaylists.slice(0, 6)"
          :key="pl.listid"
          class="ctx-item sub"
          @click="ctxAddToPlaylist(pl.listid)"
        >
          <SvgIcon name="vinyl" :size="13" />
          <span class="ctx-ellipsis">{{ pl.name }}</span>
        </button>
      </template>

      <template v-if="removableListId && ctxMenu.song.fileid">
        <div class="ctx-divider"></div>
        <button class="ctx-item danger" @click="ctxRemoveFromPlaylist">
          <SvgIcon name="trash" :size="14" />
          <span>从当前歌单移除</span>
        </button>
      </template>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, reactive, computed, watch, nextTick, onMounted, onUnmounted } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { usePlayerStore, type Track } from '../stores/player';
import { useAppStore } from '../stores/app';
import { formatSafeCover } from '../utils/trackMapper';
import SvgIcon from './SvgIcon.vue';

const props = withDefaults(
  defineProps<{
    songs: Track[];
    pageSize?: number;
    serverPage?: number;
    serverTotal?: number;
    serverHasMore?: boolean;
    removableListId?: number;
  }>(),
  {
    pageSize: 20,
    serverPage: undefined,
    serverTotal: 0,
    serverHasMore: false,
    removableListId: undefined,
  }
);

const emit = defineEmits<{
  (e: 'page-change', targetPage: number): void;
  (e: 'song-removed', hash: string): void;
}>();

const playerStore = usePlayerStore();
const appStore = useAppStore();

const shellRef = ref<HTMLElement | null>(null);
const containerRef = ref<HTMLDivElement | null>(null);
const filterInputRef = ref<HTMLInputElement | null>(null);

const localPage = ref(1);
const showFilterBar = ref(false);
const localFilter = ref('');
const brokenCovers = reactive<Record<string, boolean>>({});

const ctxMenu = reactive<{
  visible: boolean;
  x: number;
  y: number;
  song: Track | null;
  displayIdx: number;
}>({
  visible: false,
  x: 0,
  y: 0,
  song: null,
  displayIdx: 0,
});

const isServerMode = computed(() => typeof props.serverPage === 'number');

const filteredSongs = computed(() => {
  const kw = localFilter.value.trim().toLowerCase();
  if (!kw) return props.songs;
  return props.songs.filter(
    (s) => s.name.toLowerCase().includes(kw) || s.singer.toLowerCase().includes(kw)
  );
});

const effectivePageSize = computed(() => Math.min(20, Math.max(1, props.pageSize)));

const localTotalPages = computed(() =>
  Math.max(1, Math.ceil(filteredSongs.value.length / effectivePageSize.value))
);

const totalPages = computed(() => {
  if (isServerMode.value) {
    if (props.serverTotal && props.serverTotal > 0) {
      return Math.max(1, Math.ceil(props.serverTotal / effectivePageSize.value));
    }
    return 0;
  }
  return localTotalPages.value;
});

const currentPage = computed(() =>
  isServerMode.value ? (props.serverPage as number) : localPage.value
);

const baseIndex = computed(() => (currentPage.value - 1) * effectivePageSize.value);

const displaySongs = computed(() => {
  if (isServerMode.value) {
    return filteredSongs.value.slice(0, effectivePageSize.value);
  }
  const start = (localPage.value - 1) * effectivePageSize.value;
  return filteredSongs.value.slice(start, start + effectivePageSize.value);
});

const showPagination = computed(() => {
  if (isServerMode.value) {
    return (props.serverPage ?? 1) > 1 || props.serverHasMore || props.songs.length > effectivePageSize.value;
  }
  return filteredSongs.value.length > effectivePageSize.value;
});

const canGoNext = computed(() => {
  if (isServerMode.value) {
    return props.serverHasMore;
  }
  return localPage.value < localTotalPages.value;
});

watch(
  () => [props.songs, localFilter.value],
  () => {
    if (!isServerMode.value && localPage.value > localTotalPages.value) {
      localPage.value = 1;
    }
  }
);

function changePage(target: number) {
  if (target < 1) return;
  closeContextMenu();
  if (containerRef.value) {
    containerRef.value.scrollTop = 0;
  }
  if (isServerMode.value) {
    emit('page-change', target);
  } else if (target <= localTotalPages.value) {
    localPage.value = target;
  }
}

function isCurrentSong(song: Track): boolean {
  return playerStore.currentTrack?.hash === song.hash;
}

function getSafeCover(url: string): string {
  return formatSafeCover(url, 100);
}

function formatDuration(seconds: number): string {
  const m = Math.floor(seconds / 60);
  const s = Math.floor(seconds % 60);
  return `${m}:${s < 10 ? '0' : ''}${s}`;
}

function handlePlaySong(displayIdx: number) {
  const song = displaySongs.value[displayIdx];
  if (song) {
    playerStore.playSong(song);
  }
}

async function handleQuickLike(song: Track) {
  try {
    const msg = await appStore.favoriteTrackToDefault(song);
    playerStore.showToast(msg);
  } catch (err: any) {
    playerStore.showToast(err?.message || String(err));
  }
}

function openContextMenu(e: MouseEvent, song: Track, idx: number) {
  const menuW = 196;
  const menuH = 240;
  ctxMenu.x = Math.min(e.clientX, window.innerWidth - menuW - 12);
  ctxMenu.y = Math.min(e.clientY, window.innerHeight - menuH - 12);
  ctxMenu.song = song;
  ctxMenu.displayIdx = idx;
  ctxMenu.visible = true;
}

function closeContextMenu() {
  ctxMenu.visible = false;
}

function ctxPlayNow() {
  handlePlaySong(ctxMenu.displayIdx);
  closeContextMenu();
}

function ctxPlayNext() {
  if (ctxMenu.song) {
    playerStore.insertNext(ctxMenu.song);
  }
  closeContextMenu();
}

function ctxAppendQueue() {
  if (ctxMenu.song) {
    playerStore.appendSong(ctxMenu.song);
  }
  closeContextMenu();
}

async function ctxLikeDefault() {
  if (ctxMenu.song) {
    await handleQuickLike(ctxMenu.song);
  }
  closeContextMenu();
}

async function ctxAddToPlaylist(listid: number) {
  if (!ctxMenu.song) return;
  const song = ctxMenu.song;
  closeContextMenu();
  try {
    const msg = await appStore.favoriteTrackToPlaylist(listid, song);
    playerStore.showToast(msg);
  } catch (err: any) {
    playerStore.showToast(err?.message || String(err));
  }
}

async function ctxRemoveFromPlaylist() {
  if (!ctxMenu.song || !props.removableListId || !ctxMenu.song.fileid) return;
  const song = ctxMenu.song;
  closeContextMenu();
  try {
    await invoke('remove_playlist_song', {
      listid: props.removableListId,
      fileid: song.fileid,
    });
    playerStore.showToast(`已从歌单移除《${song.name}》`);
    emit('song-removed', song.hash);
  } catch (err: any) {
    playerStore.showToast(`移除失败：${String(err)}`);
  }
}

function closeFilterBar() {
  showFilterBar.value = false;
  localFilter.value = '';
}

function handleGlobalKeydown(e: KeyboardEvent) {
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'f') {
    if (shellRef.value && shellRef.value.offsetParent !== null) {
      e.preventDefault();
      showFilterBar.value = true;
      nextTick(() => {
        filterInputRef.value?.focus();
      });
    }
  }
}

onMounted(() => {
  window.addEventListener('mousedown', closeContextMenu);
  window.addEventListener('keydown', handleGlobalKeydown);
});

onUnmounted(() => {
  window.removeEventListener('mousedown', closeContextMenu);
  window.removeEventListener('keydown', handleGlobalKeydown);
});
</script>

<style scoped>
.song-list-shell {
  width: 100%;
  height: 100%;
  display: flex;
  flex-direction: column;
  min-height: 0;
  position: relative;
}

.inline-filter-bar {
  display: flex;
  align-items: center;
  position: relative;
  margin: 0 8px 8px;
  flex-shrink: 0;
}

.filter-icon {
  position: absolute;
  left: 10px;
  color: var(--text-tertiary);
}

.filter-box {
  height: 30px;
  padding-left: 30px;
  padding-right: 28px;
  font-size: 12px;
}

.filter-close {
  position: absolute;
  right: 8px;
  color: var(--text-tertiary);
  display: flex;
  align-items: center;
}

.virtual-list-container {
  flex: 1;
  width: 100%;
  overflow-y: auto;
  min-height: 0;
}

.empty-tip {
  margin-top: 48px;
  text-align: center;
  color: var(--text-tertiary);
  font-size: 13px;
}

.song-row {
  height: var(--song-item-height);
  display: flex;
  align-items: center;
  padding: 0 14px;
  gap: 12px;
  border-radius: var(--radius-md);
  margin: 0 4px;
  transition: background-color var(--duration-fast);
  cursor: pointer;
}

.song-row:hover {
  background: var(--bg-card-hover);
}

.song-row.is-active {
  background: var(--bg-card);
  border: 1px solid var(--border-subtle);
}

.song-row.is-active .song-title {
  color: var(--accent-color);
  font-weight: 600;
}

.col-index {
  width: 28px;
  font-size: 12px;
  color: var(--text-tertiary);
  text-align: center;
  font-variant-numeric: tabular-nums;
}

.col-cover {
  width: 40px;
  height: 40px;
  border-radius: var(--radius-sm);
  overflow: hidden;
  flex-shrink: 0;
  background: var(--bg-card);
  border: 1px solid var(--border-subtle);
}

.song-cover {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.song-cover-placeholder {
  width: 100%;
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-tertiary);
}

.col-info {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 3px;
}

.song-title {
  font-size: 13px;
  font-weight: 500;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.song-artist {
  font-size: 11.5px;
  color: var(--text-secondary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.col-actions {
  display: flex;
  align-items: center;
  gap: 6px;
  opacity: 0;
  transition: opacity var(--duration-fast);
}

.song-row:hover .col-actions {
  opacity: 1;
}

.action-btn {
  width: 28px;
  height: 28px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: var(--radius-full);
  color: var(--text-secondary);
  background: var(--bg-card);
  border: 1px solid var(--border-subtle);
}

.action-btn:hover {
  background: var(--accent-color);
  color: var(--accent-text);
  border-color: transparent;
}

.col-duration {
  font-size: 11.5px;
  color: var(--text-tertiary);
  width: 44px;
  text-align: right;
  font-variant-numeric: tabular-nums;
}

.list-pager {
  border-top: 1px solid var(--border-divider);
  padding: 10px 12px 2px;
}

.pager-btn {
  padding: 4px 12px;
  font-size: 12px;
  gap: 4px;
}

.ctx-menu {
  position: fixed;
  width: 196px;
  background: var(--bg-popover);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-md);
  box-shadow: 0 12px 32px rgba(0, 0, 0, 0.18);
  padding: 5px;
  z-index: 350;
}

.ctx-item {
  width: 100%;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 10px;
  border-radius: var(--radius-sm);
  font-size: 12.5px;
  color: var(--text-primary);
  text-align: left;
}

.ctx-item.sub {
  font-size: 12px;
  color: var(--text-secondary);
}

.ctx-item:hover {
  background: var(--bg-card-hover);
  color: var(--accent-color);
}

.ctx-item.danger {
  color: #dc2626;
}

.ctx-item.danger:hover {
  background: rgba(220, 38, 38, 0.12);
  color: #dc2626;
}

.ctx-divider {
  height: 1px;
  background: var(--border-divider);
  margin: 4px 0;
}

.ctx-group-label {
  font-size: 10.5px;
  color: var(--text-tertiary);
  padding: 3px 10px;
}

.ctx-ellipsis {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>
