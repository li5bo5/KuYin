<template>
  <div class="view-container">
    <div class="top-bar">
      <div class="left-group">
        <h1 class="view-title">我的歌单</h1>
      </div>

      <div class="right-group" v-if="appStore.authProfile.is_logged_in">
        <div v-if="showCreateBox" class="create-inline">
          <input
            v-model="newPlaylistName"
            class="fluent-input create-input"
            placeholder="输入新歌单名称..."
            @keydown.enter="handleCreatePlaylist"
          />
          <button class="fluent-btn fluent-btn-auto fluent-btn-primary" @click="handleCreatePlaylist">创建</button>
          <button class="fluent-btn fluent-btn-auto" @click="showCreateBox = false">取消</button>
        </div>
        <button v-else class="fluent-btn" @click="showCreateBox = true">
          <SvgIcon name="plus" :size="14" />
          新建歌单
        </button>

        <button class="fluent-btn" @click="loadUserPlaylists">
          <SvgIcon name="refresh" :size="14" />
          刷新列表
        </button>
      </div>
    </div>

    <div v-if="!appStore.authProfile.is_logged_in" class="login-guide">
      <SvgIcon name="vinyl" :size="40" />
      <p>登录酷狗账号后可同步「我喜欢」与个人云歌单，也可在「发现」页点开任意精选歌单</p>
      <button class="fluent-btn fluent-btn-primary" @click="appStore.showLoginModal = true">
        立即登录
      </button>
    </div>

    <div v-else-if="listLoading" class="loading-state">正在同步云端歌单...</div>

    <div v-else-if="userPlaylists.length === 0" class="loading-state">
      暂无云歌单数据，点击右上角「新建歌单」创建一个吧
    </div>

    <template v-else>
      <div class="playlist-grid">
        <div
          v-for="pl in pagedUserPlaylists"
          :key="pl.listid"
          class="pl-card"
          @click="openUserPlaylist(pl)"
        >
          <div class="pl-cover-wrap">
            <img
              v-if="pl.pic && !brokenPlCovers[pl.listid]"
              :src="pl.pic"
              alt="cover"
              loading="lazy"
              decoding="async"
              class="pl-cover"
              @error="brokenPlCovers[pl.listid] = true"
            />
            <div v-else class="pl-cover-fallback">
              <SvgIcon :name="pl.isDefaultLike ? 'heart-filled' : 'vinyl'" :size="28" />
            </div>

            <button
              v-if="!pl.isDefaultLike"
              class="pl-del-btn"
              title="删除或取消收藏此歌单"
              @click.stop="handleDeletePlaylist(pl)"
            >
              <SvgIcon name="trash" :size="13" />
            </button>
          </div>
          <div class="pl-name" :title="pl.name">{{ pl.name }}</div>
          <div class="pl-count">{{ pl.count }} 首</div>
        </div>
      </div>

      <div v-if="plTotalPages > 1" class="fluent-pagination">
        <button
          class="fluent-btn pager-btn"
          :disabled="plPage <= 1"
          @click="plPage--"
        >
          <SvgIcon name="chevron-left" :size="14" class="pg-icon" />
          <span class="pg-text">上一页</span>
        </button>
        <span class="fluent-page-info">
          第 {{ plPage }} / {{ plTotalPages }} 页
        </span>
        <button
          class="fluent-btn pager-btn"
          :disabled="plPage >= plTotalPages"
          @click="plPage++"
        >
          <span class="pg-text">下一页</span>
          <SvgIcon name="chevron-right" :size="14" class="pg-icon" />
        </button>
      </div>
    </template>
  </div>
</template>

<script setup lang="ts">
import { ref, reactive, computed, watch, onMounted } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import SvgIcon from '../components/SvgIcon.vue';
import { useAppStore } from '../stores/app';
import { usePlayerStore } from '../stores/player';
import { formatSafeCover } from '../utils/trackMapper';

interface UserPlaylistCard {
  listid: number;
  globalId: string;
  name: string;
  count: number;
  pic: string;
  isDefaultLike: boolean;
}

const PAGE_SIZE = 20;

const appStore = useAppStore();
const playerStore = usePlayerStore();

const userPlaylists = ref<UserPlaylistCard[]>([]);
const plPage = ref(1);
const brokenPlCovers = reactive<Record<number, boolean>>({});
const listLoading = ref(false);

const showCreateBox = ref(false);
const newPlaylistName = ref('');

const plTotalPages = computed(() =>
  Math.max(1, Math.ceil(userPlaylists.value.length / PAGE_SIZE))
);

const pagedUserPlaylists = computed(() => {
  const start = (plPage.value - 1) * PAGE_SIZE;
  return userPlaylists.value.slice(start, start + PAGE_SIZE);
});

async function loadUserPlaylists() {
  if (!appStore.authProfile.is_logged_in) {
    userPlaylists.value = [];
    return;
  }
  listLoading.value = true;
  try {
    const res = await invoke<any>('get_user_playlists', { page: 1, pagesize: 50 });
    const rawInfo = res?.data?.info || [];
    userPlaylists.value = rawInfo
      .map((item: any) => ({
        listid: Number(item.listid || 0),
        globalId: String(item.list_create_gid || item.global_collection_id || ''),
        name: String(item.name || '我的歌单'),
        count: Number(item.count || 0),
        pic: formatSafeCover(item.pic || item.union_cover, 240),
        isDefaultLike:
          String(item.name || '').includes('我喜欢') || Number(item.sort) === 1,
      }))
      .filter((p: UserPlaylistCard) => p.listid > 0);
    appStore.loadUserPlaylists();
  } catch (err) {
    console.warn('同步用户歌单失败:', err);
  } finally {
    listLoading.value = false;
  }
}

async function handleCreatePlaylist() {
  const name = newPlaylistName.value.trim();
  if (!name) return;
  try {
    await invoke('create_or_collect_playlist', {
      name,
      listType: 0,
      listCreateUserid: null,
      listCreateListid: null,
      listCreateGid: null,
    });
    newPlaylistName.value = '';
    showCreateBox.value = false;
    playerStore.showToast(`已创建歌单「${name}」`);
    await loadUserPlaylists();
  } catch (err: any) {
    playerStore.showToast(`创建歌单失败：${String(err)}`);
  }
}

async function handleDeletePlaylist(pl: UserPlaylistCard) {
  try {
    await invoke('delete_playlist', { listid: pl.listid });
    playerStore.showToast(`已移除歌单「${pl.name}」`);
    await loadUserPlaylists();
  } catch (err: any) {
    playerStore.showToast(`删除歌单失败：${String(err)}`);
  }
}

function openUserPlaylist(pl: UserPlaylistCard) {
  appStore.openPlaylistDetail({
    id: pl.globalId,
    name: pl.name,
    cover: pl.pic,
    intro: `云端歌单 · 共 ${pl.count} 首`,
    kind: 'playlist',
    isUserList: true,
    listid: pl.listid,
  });
}

watch(
  () => appStore.activePlaylist,
  (val, oldVal) => {
    if (!val && oldVal && appStore.authProfile.is_logged_in) {
      loadUserPlaylists();
    }
  }
);

watch(
  () => appStore.authProfile.is_logged_in,
  (logged) => {
    if (logged) {
      loadUserPlaylists();
    } else {
      userPlaylists.value = [];
    }
  }
);

onMounted(() => {
  if (appStore.authProfile.is_logged_in) {
    loadUserPlaylists();
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

.view-title {
  font-size: 22px;
  font-weight: 700;
}

.right-group {
  display: flex;
  align-items: center;
  gap: 8px;
}

.create-inline {
  display: flex;
  align-items: center;
  gap: 6px;
}

.create-input {
  width: 170px;
  height: 30px;
  font-size: 12px;
}

.login-guide {
  margin-top: 72px;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 14px;
  color: var(--text-secondary);
  font-size: 13px;
  text-align: center;
}

.loading-state {
  margin-top: 64px;
  text-align: center;
  color: var(--text-tertiary);
}

.playlist-grid {
  flex: 1;
  overflow-y: auto;
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(136px, 1fr));
  gap: 14px;
  align-content: start;
}

.pl-card {
  padding: 10px;
  background: var(--bg-card);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-md);
  cursor: pointer;
  transition: transform var(--duration-fast), background-color var(--duration-fast);
}

.pl-card:hover {
  background: var(--bg-card-hover);
  transform: translateY(-2px);
}

.pl-cover-wrap {
  position: relative;
  width: 100%;
  aspect-ratio: 1 / 1;
  border-radius: var(--radius-sm);
  overflow: hidden;
  background: var(--bg-app);
  margin-bottom: 8px;
}

.pl-cover {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.pl-cover-fallback {
  width: 100%;
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--accent-color);
}

.pl-del-btn {
  position: absolute;
  top: 6px;
  right: 6px;
  width: 26px;
  height: 26px;
  border-radius: var(--radius-full);
  background: rgba(0, 0, 0, 0.55);
  color: #ffffff;
  display: flex;
  align-items: center;
  justify-content: center;
  opacity: 0;
  transition: opacity var(--duration-fast);
}

.pl-card:hover .pl-del-btn {
  opacity: 1;
}

.pl-del-btn:hover {
  background: #e81123;
}

.pl-name {
  font-size: 13px;
  font-weight: 600;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.pl-count {
  font-size: 11px;
  color: var(--text-tertiary);
  margin-top: 2px;
}
</style>
