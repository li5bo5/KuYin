<template>
  <header class="titlebar" data-tauri-drag-region>
    <div class="logo-area" data-tauri-drag-region>
      <!-- 用户提供的 icons/icon.svg 折纸 K 品牌矢量图标 -->
      <svg class="brand-logo" viewBox="0 0 1000 1000" aria-hidden="true">
        <defs>
          <linearGradient id="tbStemGrad" x1="195" y1="195" x2="355" y2="805" gradientUnits="userSpaceOnUse">
            <stop offset="0%" stop-color="#57B8FC" />
            <stop offset="50%" stop-color="#299EF5" />
            <stop offset="100%" stop-color="#40C2FA" />
          </linearGradient>
          <linearGradient id="tbUpperGrad" x1="220" y1="580" x2="695" y2="195" gradientUnits="userSpaceOnUse">
            <stop offset="0%" stop-color="#33C7E0" />
            <stop offset="55%" stop-color="#7AD1FC" />
            <stop offset="100%" stop-color="#A3E3FF" />
          </linearGradient>
          <linearGradient id="tbLowerGrad" x1="220" y1="400" x2="695" y2="805" gradientUnits="userSpaceOnUse">
            <stop offset="0%" stop-color="#3DD1CC" />
            <stop offset="45%" stop-color="#1FD9B8" />
            <stop offset="100%" stop-color="#8FF2DB" />
          </linearGradient>
          <linearGradient id="tbPlayGrad" x1="480" y1="320" x2="815" y2="650" gradientUnits="userSpaceOnUse">
            <stop offset="0%" stop-color="#2CC9BC" />
            <stop offset="100%" stop-color="#52EDD6" />
          </linearGradient>
        </defs>
        <rect x="195" y="195" width="160" height="610" rx="28" fill="url(#tbStemGrad)" />
        <path d="M 545 195 L 685 195 Q 702 195 690 208 L 315 650 Q 298 670 272 670 L 195 670 L 195 520 L 495 215 Q 515 195 545 195 Z" fill="url(#tbUpperGrad)" />
        <path d="M 195 330 L 272 330 Q 298 330 315 350 L 690 792 Q 702 805 685 805 L 545 805 Q 515 805 495 785 L 195 480 Z" fill="url(#tbLowerGrad)" />
        <path d="M 480 328 Q 480 288 515 308 L 792 478 Q 825 500 792 522 L 515 692 Q 480 712 480 672 Z" fill="url(#tbPlayGrad)" />
      </svg>
      <span class="logo-text" data-tauri-drag-region>酷音 KuYin</span>
      <button class="vip-btn" @click="handleClaimVip" title="每日免费签到领取概念版畅听 VIP">
        <SvgIcon name="crown" :size="12" class="vip-crown" />
        <span class="vip-status">{{ displayVipText }}</span>
      </button>
    </div>

    <!-- 架构红线（六.2）：300ms 防抖搜索输入 + 实时搜索建议浮层 -->
    <div class="search-box" ref="searchBoxRef">
      <SvgIcon name="search" :size="14" class="search-prefix-icon" />
      <input
        ref="searchInputRef"
        v-model="inputKeyword"
        class="fluent-input search-input"
        placeholder="搜索歌曲、歌手、专辑、歌单... (↑↓选择 / Esc返回)"
        @focus="handleFocusSuggest"
        @input="handleDebouncedInput"
        @keydown="handleSearchKeydown"
      />
      <button
        v-if="inputKeyword"
        class="search-clear-btn"
        @click="clearSearch"
        title="清空输入"
      >
        <SvgIcon name="close" :size="12" />
      </button>

      <div v-if="showSuggestions && suggestions.length > 0" class="suggest-dropdown">
        <button
          v-for="(tip, idx) in suggestions"
          :key="idx"
          :class="['suggest-item', { 'is-active': selectedSuggestIdx === idx }]"
          @mouseenter="selectedSuggestIdx = idx"
          @mousedown.prevent="selectSuggestion(tip)"
        >
          <SvgIcon name="search" :size="13" />
          <span class="suggest-text">{{ tip }}</span>
        </button>
      </div>
    </div>

    <!-- 右上角：用户信息 -> 设置按钮（紧贴用户信息右侧） -> 主题 -> 窗口三键 -->
    <div class="window-controls">
      <button
        class="account-btn"
        @click="appStore.showLoginModal = true"
        :title="appStore.authProfile.is_logged_in ? '查看账号信息' : '点击登录酷狗账号'"
      >
        <img
          v-if="appStore.authProfile.is_logged_in && safeAvatar && !avatarErr"
          :src="safeAvatar"
          alt="avatar"
          loading="lazy"
          decoding="async"
          class="mini-avatar"
          @error="avatarErr = true"
        />
        <SvgIcon v-else name="user" :size="15" />
        <span class="account-name">
          {{ appStore.authProfile.is_logged_in ? appStore.authProfile.nickname : '登录' }}
        </span>
      </button>

      <!-- 用户信息右侧紧邻设置按钮 -->
      <button
        :class="['control-btn', { active: appStore.currentTab === 'settings' }]"
        @click="appStore.setTab('settings')"
        title="应用设置"
      >
        <SvgIcon name="settings" :size="16" />
      </button>

      <button
        class="control-btn"
        @click="appStore.toggleTheme"
        :title="appStore.resolvedTheme === 'dark' ? '切换浅色模式' : '切换深色模式'"
      >
        <SvgIcon :name="appStore.resolvedTheme === 'dark' ? 'sun' : 'moon'" :size="16" />
      </button>

      <span class="win-sep"></span>

      <button class="control-btn" @click="minimizeWindow" title="最小化到任务栏 (0ms 秒开)">
        <SvgIcon name="minimize" :size="15" />
      </button>
      <button class="control-btn" @click="maximizeWindow" title="最大化 / 还原">
        <SvgIcon name="maximize" :size="14" />
      </button>
      <button
        class="control-btn close-btn"
        @click="closeWindow"
        title="关闭隐藏到系统托盘"
      >
        <SvgIcon name="close" :size="15" />
      </button>
    </div>
  </header>
</template>

<script setup lang="ts">
import { ref, computed, watch, onMounted, onUnmounted } from 'vue';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { invoke } from '@tauri-apps/api/core';
import { useAppStore } from '../stores/app';
import { usePlayerStore } from '../stores/player';
import { formatSafeCover } from '../utils/trackMapper';
import SvgIcon from './SvgIcon.vue';

const appStore = useAppStore();
const playerStore = usePlayerStore();
const inputKeyword = ref('');
const customVipTip = ref('');
const avatarErr = ref(false);
const suggestions = ref<string[]>([]);
const showSuggestions = ref(false);
const selectedSuggestIdx = ref(-1);
const searchBoxRef = ref<HTMLElement | null>(null);
const searchInputRef = ref<HTMLInputElement | null>(null);

const appWindow = getCurrentWindow();
let debounceTimer: number | null = null;

const safeAvatar = computed(() => formatSafeCover(appStore.authProfile.pic, 100));
const displayVipText = computed(() => {
  if (customVipTip.value) return customVipTip.value;
  return appStore.vipActive ? 'VIP畅听中' : '签到领VIP';
});

watch(
  () => appStore.authProfile.pic,
  () => {
    avatarErr.value = false;
  }
);

function minimizeWindow() {
  appWindow.minimize();
}

async function maximizeWindow() {
  const isMax = await appWindow.isMaximized();
  if (isMax) {
    appWindow.unmaximize();
  } else {
    appWindow.maximize();
  }
}

async function closeWindow() {
  await invoke('close_main_window');
}

async function fetchSuggestions(kw: string) {
  if (!kw) {
    suggestions.value = [];
    return;
  }
  try {
    const res = await invoke<any>('search_suggest', { keyword: kw });
    const list: string[] = [];
    const groups = res?.data;
    if (Array.isArray(groups)) {
      for (const grp of groups) {
        const records = grp?.RecordDatas;
        if (Array.isArray(records)) {
          for (const rec of records) {
            const hint = String(rec?.HintInfo || '').trim();
            if (hint && !list.includes(hint)) {
              list.push(hint);
            }
            if (list.length >= 8) break;
          }
        }
        if (list.length >= 8) break;
      }
    }
    suggestions.value = list;
    selectedSuggestIdx.value = -1;
  } catch {
    suggestions.value = [];
    selectedSuggestIdx.value = -1;
  }
}

function handleFocusSuggest() {
  if (inputKeyword.value.trim()) {
    showSuggestions.value = true;
  }
}

function handleDebouncedInput() {
  if (debounceTimer) {
    clearTimeout(debounceTimer);
  }
  selectedSuggestIdx.value = -1;
  const kw = inputKeyword.value.trim();
  if (!kw) {
    suggestions.value = [];
    showSuggestions.value = false;
    return;
  }
  debounceTimer = window.setTimeout(() => {
    const latest = inputKeyword.value.trim();
    if (latest) {
      showSuggestions.value = true;
      fetchSuggestions(latest);
      appStore.triggerSearch(latest);
    }
  }, 300);
}

function handleImmediateSearch() {
  if (debounceTimer) {
    clearTimeout(debounceTimer);
    debounceTimer = null;
  }
  showSuggestions.value = false;
  selectedSuggestIdx.value = -1;
  const kw = inputKeyword.value.trim();
  if (kw) {
    appStore.triggerSearch(kw);
  }
}

function handleSearchKeydown(e: KeyboardEvent) {
  if (e.key === 'ArrowDown') {
    if (suggestions.value.length > 0) {
      e.preventDefault();
      showSuggestions.value = true;
      selectedSuggestIdx.value = (selectedSuggestIdx.value + 1) % suggestions.value.length;
    }
    return;
  }
  if (e.key === 'ArrowUp') {
    if (suggestions.value.length > 0) {
      e.preventDefault();
      showSuggestions.value = true;
      selectedSuggestIdx.value =
        selectedSuggestIdx.value <= 0
          ? suggestions.value.length - 1
          : selectedSuggestIdx.value - 1;
    }
    return;
  }
  if (e.key === 'Enter') {
    e.preventDefault();
    if (
      showSuggestions.value &&
      selectedSuggestIdx.value >= 0 &&
      selectedSuggestIdx.value < suggestions.value.length
    ) {
      selectSuggestion(suggestions.value[selectedSuggestIdx.value]);
    } else {
      handleImmediateSearch();
    }
    return;
  }
  if (e.key === 'Escape') {
    e.preventDefault();
    if (debounceTimer) {
      clearTimeout(debounceTimer);
      debounceTimer = null;
    }
    inputKeyword.value = '';
    suggestions.value = [];
    showSuggestions.value = false;
    selectedSuggestIdx.value = -1;
    searchInputRef.value?.blur();
    appStore.exitSearchToPrevious();
  }
}

function selectSuggestion(tip: string) {
  if (debounceTimer) {
    clearTimeout(debounceTimer);
    debounceTimer = null;
  }
  inputKeyword.value = tip;
  showSuggestions.value = false;
  selectedSuggestIdx.value = -1;
  appStore.triggerSearch(tip);
}

function clearSearch() {
  inputKeyword.value = '';
  suggestions.value = [];
  showSuggestions.value = false;
  selectedSuggestIdx.value = -1;
}

function handleClickOutside(e: MouseEvent) {
  if (searchBoxRef.value && !searchBoxRef.value.contains(e.target as Node)) {
    showSuggestions.value = false;
  }
}

async function handleClaimVip() {
  if (!appStore.authProfile.is_logged_in) {
    appStore.showLoginModal = true;
    playerStore.showToast('请先登录酷狗账号再领取概念版 VIP');
    return;
  }
  try {
    customVipTip.value = '领取中...';
    await invoke('claim_vip');
    appStore.vipActive = true;
    customVipTip.value = 'VIP已生效';
    playerStore.showToast('今日概念版畅听 VIP 领取成功！');
  } catch {
    customVipTip.value = '今日已领';
    playerStore.showToast('今日已完成签到或暂无需重复领取');
  }
}

onMounted(() => {
  document.addEventListener('mousedown', handleClickOutside);
});

onUnmounted(() => {
  document.removeEventListener('mousedown', handleClickOutside);
  if (debounceTimer) {
    clearTimeout(debounceTimer);
  }
});
</script>

<style scoped>
.titlebar {
  height: var(--titlebar-height);
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 8px 0 14px;
  background: var(--bg-card);
  border-bottom: 1px solid var(--border-divider);
  backdrop-filter: blur(20px);
  -webkit-backdrop-filter: blur(20px);
  z-index: 120;
}

.logo-area {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}

.brand-logo {
  width: 22px;
  height: 22px;
  flex-shrink: 0;
}

.logo-text {
  font-size: 13.5px;
  font-weight: 700;
  letter-spacing: 0.3px;
}

.vip-btn {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 2px 9px;
  background: rgba(245, 158, 11, 0.14);
  border: 1px solid rgba(245, 158, 11, 0.36);
  border-radius: var(--radius-full);
  margin-left: 4px;
  transition: all var(--duration-fast);
}

.vip-btn:hover {
  background: rgba(245, 158, 11, 0.24);
}

.vip-crown {
  color: #d97706;
}

.vip-status {
  font-size: 11px;
  font-weight: 600;
  color: var(--text-secondary);
}

.search-box {
  position: relative;
  flex: 0 1 320px;
  margin: 0 12px;
  display: flex;
  align-items: center;
}

.search-prefix-icon {
  position: absolute;
  left: 11px;
  color: var(--text-tertiary);
  pointer-events: none;
}

.search-input {
  height: 30px;
  padding-left: 32px;
  padding-right: 28px;
  border-radius: var(--radius-full);
  font-size: 12.5px;
}

.search-clear-btn {
  position: absolute;
  right: 8px;
  width: 18px;
  height: 18px;
  border-radius: var(--radius-full);
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-tertiary);
}

.search-clear-btn:hover {
  background: var(--bg-card-hover);
  color: var(--text-primary);
}

.suggest-dropdown {
  position: absolute;
  top: 36px;
  left: 0;
  right: 0;
  background: var(--bg-popover);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-md);
  box-shadow: 0 10px 28px rgba(0, 0, 0, 0.16);
  padding: 4px;
  z-index: 200;
}

.suggest-item {
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

.suggest-item:hover,
.suggest-item.is-active {
  background: var(--bg-card-hover);
  color: var(--accent-color);
}

.suggest-text {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.window-controls {
  display: flex;
  align-items: center;
  height: 100%;
  gap: 2px;
  flex-shrink: 0;
}

.account-btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 4px 10px;
  margin-right: 2px;
  border-radius: var(--radius-full);
  background: var(--bg-card);
  border: 1px solid var(--border-subtle);
  font-size: 12px;
  color: var(--text-secondary);
  max-width: 136px;
  transition: all var(--duration-fast);
}

.account-btn:hover {
  background: var(--bg-card-hover);
  color: var(--text-primary);
}

.mini-avatar {
  width: 18px;
  height: 18px;
  border-radius: var(--radius-full);
  object-fit: cover;
}

.account-name {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.win-sep {
  width: 1px;
  height: 14px;
  background: var(--border-subtle);
  margin: 0 4px;
}

.control-btn {
  width: 34px;
  height: 30px;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-secondary);
  border-radius: var(--radius-sm);
  transition: all var(--duration-fast);
}

.control-btn:hover {
  background: var(--bg-card-hover);
  color: var(--text-primary);
}

.control-btn.active {
  color: var(--accent-color);
  background: var(--bg-card-hover);
}

.close-btn:hover {
  background: #e81123 !important;
  color: #ffffff !important;
}
</style>
