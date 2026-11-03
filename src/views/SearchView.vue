<template>
  <div class="view-container">
    <div class="view-header">
      <div>
        <h1 class="view-title">搜索结果</h1>
        <p class="view-subtitle" v-if="appStore.searchKeyword">
          关键词：“{{ appStore.searchKeyword }}” · 第 {{ page }} 页
        </p>
        <p class="view-subtitle" v-else>请在顶部搜索栏输入歌名、歌手、专辑或歌单</p>
      </div>

      <div class="header-actions">
        <div class="type-tabs">
          <button
            v-for="t in typeList"
            :key="t.value"
            :class="['type-tab', { active: searchType === t.value }]"
            @click="switchType(t.value)"
          >
            {{ t.label }}
          </button>
        </div>

        <template v-if="searchType === 'song' && songs.length > 0">
          <button
            class="fluent-btn fluent-btn-primary"
            @click="playAll"
          >
            <SvgIcon name="play" :size="14" />
            播放全部
          </button>
          <button
            class="fluent-btn"
            @click="appendAll"
          >
            <SvgIcon name="playlist-add" :size="14" />
            添加队列
          </button>
        </template>
      </div>
    </div>

    <div v-if="loading" class="loading-state">
      <p>正在检索酷狗概念版全网曲库...</p>
    </div>

    <!-- 单曲结果（内置每页 20 首翻页） -->
    <template v-else-if="searchType === 'song'">
      <div v-if="songs.length === 0" class="empty-state">暂无相关单曲结果</div>
      <div v-else class="list-wrapper">
        <VirtualSongList
          :songs="songs"
          :server-page="page"
          :server-total="totalCount"
          :server-has-more="hasMore"
          @page-change="handlePageChange"
        />
      </div>
    </template>

    <!-- 歌手 / 专辑 / 歌单 卡片网格结果（每页 20 个，超过 20 个启用翻页） -->
    <template v-else>
      <div v-if="entities.length === 0" class="empty-state">暂无相关搜索结果</div>
      <template v-else>
        <div class="entity-grid">
          <div
            v-for="item in entities"
            :key="item.id"
            class="entity-card"
            @click="openEntity(item)"
          >
            <div :class="['entity-cover-wrap', { round: searchType === 'author' }]">
              <img
                v-if="item.cover && !brokenEntityCovers[item.id]"
                :src="item.cover"
                alt="cover"
                loading="lazy"
                decoding="async"
                class="entity-cover"
                @error="brokenEntityCovers[item.id] = true"
              />
              <div v-else class="entity-fallback">
                <SvgIcon
                  :name="
                    searchType === 'author'
                      ? 'user'
                      : searchType === 'album'
                        ? 'disc'
                        : 'vinyl'
                  "
                  :size="28"
                />
              </div>
            </div>
            <div class="entity-title" :title="item.name">{{ item.name }}</div>
            <div class="entity-sub" :title="item.sub">{{ item.sub }}</div>
          </div>
        </div>

        <div v-if="page > 1 || hasMore" class="fluent-pagination">
          <button
            class="fluent-btn pager-btn"
            :disabled="page <= 1 || loading"
            @click="handlePageChange(page - 1)"
          >
            <SvgIcon name="chevron-left" :size="14" class="pg-icon" />
            <span class="pg-text">上一页</span>
          </button>
          <span class="fluent-page-info">
            <template v-if="entityTotalPages > 0">
              第 {{ page }} / {{ entityTotalPages }} 页
            </template>
            <template v-else>
              第 {{ page }} 页
            </template>
          </span>
          <button
            class="fluent-btn pager-btn"
            :disabled="!hasMore || loading"
            @click="handlePageChange(page + 1)"
          >
            <span class="pg-text">下一页</span>
            <SvgIcon name="chevron-right" :size="14" class="pg-icon" />
          </button>
        </div>
      </template>
    </template>
  </div>
</template>

<script setup lang="ts">
import { ref, reactive, computed, watch, onMounted } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import VirtualSongList from '../components/VirtualSongList.vue';
import SvgIcon from '../components/SvgIcon.vue';
import { useAppStore } from '../stores/app';
import { usePlayerStore, type Track } from '../stores/player';
import {
  extractTracksFromResponse,
  formatSafeCover,
  stripEmTags,
} from '../utils/trackMapper';

type SearchCategory = 'song' | 'author' | 'album' | 'special';

interface SearchEntity {
  id: string;
  name: string;
  sub: string;
  cover: string;
  kind: 'artist' | 'album' | 'playlist';
}

const PAGE_SIZE = 20;

const typeList: Array<{ label: string; value: SearchCategory }> = [
  { label: '单曲', value: 'song' },
  { label: '歌手', value: 'author' },
  { label: '专辑', value: 'album' },
  { label: '歌单', value: 'special' },
];

const appStore = useAppStore();
const playerStore = usePlayerStore();
const searchType = ref<SearchCategory>('song');
const songs = ref<Track[]>([]);
const entities = ref<SearchEntity[]>([]);
const brokenEntityCovers = reactive<Record<string, boolean>>({});
const loading = ref(false);
const page = ref(1);
const totalCount = ref(0);
const hasMore = ref(false);

const entityTotalPages = computed(() =>
  totalCount.value > 0 ? Math.max(1, Math.ceil(totalCount.value / PAGE_SIZE)) : 0
);

async function doSearch() {
  if (!appStore.searchKeyword.trim()) {
    songs.value = [];
    entities.value = [];
    totalCount.value = 0;
    hasMore.value = false;
    return;
  }

  loading.value = true;
  try {
    const res = await invoke<any>('search', {
      keyword: appStore.searchKeyword,
      page: page.value,
      pagesize: PAGE_SIZE,
      searchType: searchType.value,
    });

    const d = res?.data ?? res;
    const rawTotal = Number(d?.total ?? d?.count ?? d?.total_count ?? res?.total ?? 0);
    totalCount.value = Number.isFinite(rawTotal) && rawTotal > 0 ? rawTotal : 0;

    if (searchType.value === 'song') {
      const list = extractTracksFromResponse(res);
      songs.value = list.slice(0, PAGE_SIZE);
      hasMore.value =
        totalCount.value > 0 ? page.value * PAGE_SIZE < totalCount.value : list.length >= PAGE_SIZE;
    } else {
      const rawLists: any[] =
        res?.data?.lists || res?.data?.info || res?.data?.list || [];
      if (searchType.value === 'author') {
        entities.value = rawLists
          .slice(0, PAGE_SIZE)
          .map((a: any) => ({
            id: String(a.AuthorId || a.singerid || a.author_id || ''),
            name: stripEmTags(String(a.AuthorName || a.singername || '歌手')),
            sub: `单曲 ${a.AudioCount ?? a.songcount ?? 0}`,
            cover: formatSafeCover(a.Avatar || a.imgurl, 240),
            kind: 'artist' as const,
          }))
          .filter((a) => !!a.id);
      } else if (searchType.value === 'album') {
        entities.value = rawLists
          .slice(0, PAGE_SIZE)
          .map((al: any) => ({
            id: String(al.albumid || al.album_id || ''),
            name: stripEmTags(String(al.albumname || al.album_name || '专辑')),
            sub: stripEmTags(String(al.singer || al.singername || '')),
            cover: formatSafeCover(al.img || al.imgurl || al.sizable_cover, 240),
            kind: 'album' as const,
          }))
          .filter((al) => !!al.id);
      } else {
        entities.value = rawLists
          .slice(0, PAGE_SIZE)
          .map((sp: any) => ({
            id: String(sp.gid || sp.global_collection_id || sp.specialid || ''),
            name: stripEmTags(String(sp.specialname || sp.name || '歌单')),
            sub: `${sp.song_count ?? sp.suid ?? ''} 首`,
            cover: formatSafeCover(sp.img || sp.imgurl, 240),
            kind: 'playlist' as const,
          }))
          .filter((sp) => !!sp.id);
      }
      hasMore.value =
        totalCount.value > 0
          ? page.value * PAGE_SIZE < totalCount.value
          : rawLists.length >= PAGE_SIZE;
    }
  } catch (err) {
    console.warn('搜索请求结束或取消:', err);
  } finally {
    loading.value = false;
  }
}

function switchType(type: SearchCategory) {
  if (searchType.value === type) return;
  searchType.value = type;
  page.value = 1;
  doSearch();
}

function handlePageChange(target: number) {
  if (target < 1) return;
  page.value = target;
  doSearch();
}

function openEntity(item: SearchEntity) {
  appStore.openPlaylistDetail({
    id: item.id,
    name: item.name,
    cover: item.cover,
    intro: item.sub,
    kind: item.kind,
    isUserList: false,
  });
}

function playAll() {
  if (songs.value.length > 0) {
    playerStore.setQueue(songs.value, 0);
  }
}

function appendAll() {
  if (songs.value.length > 0) {
    playerStore.appendQueue(songs.value);
  }
}

watch(
  () => appStore.searchKeyword,
  () => {
    page.value = 1;
    doSearch();
  }
);

onMounted(() => {
  if (appStore.searchKeyword) {
    doSearch();
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

.view-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 12px;
  flex-shrink: 0;
}

.header-actions {
  display: flex;
  align-items: center;
  gap: 10px;
}

.type-tabs {
  display: inline-flex;
  gap: 4px;
  background: var(--bg-card);
  border: 1px solid var(--border-subtle);
  padding: 3px;
  border-radius: var(--radius-md);
}

.type-tab {
  padding: 4px 12px;
  border-radius: var(--radius-sm);
  font-size: 12px;
  color: var(--text-secondary);
}

.type-tab.active {
  background: var(--accent-color);
  color: var(--accent-text);
  font-weight: 600;
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
  margin-top: 72px;
  text-align: center;
  color: var(--text-tertiary);
}

.list-wrapper {
  flex: 1;
  min-height: 0;
}

.entity-grid {
  flex: 1;
  overflow-y: auto;
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(132px, 1fr));
  gap: 14px;
  align-content: start;
}

.entity-card {
  padding: 10px;
  background: var(--bg-card);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-md);
  cursor: pointer;
  transition: transform var(--duration-fast), background-color var(--duration-fast);
}

.entity-card:hover {
  background: var(--bg-card-hover);
  transform: translateY(-2px);
}

.entity-cover-wrap {
  width: 100%;
  aspect-ratio: 1 / 1;
  border-radius: var(--radius-sm);
  overflow: hidden;
  background: var(--bg-app);
  margin-bottom: 8px;
}

.entity-cover-wrap.round {
  border-radius: var(--radius-full);
}

.entity-cover {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.entity-fallback {
  width: 100%;
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-tertiary);
}

.entity-title {
  font-size: 12.5px;
  font-weight: 600;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.entity-sub {
  font-size: 11px;
  color: var(--text-tertiary);
  margin-top: 2px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>
