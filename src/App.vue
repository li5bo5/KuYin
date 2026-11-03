<template>
  <div class="app-layout" :data-theme="appStore.resolvedTheme">
    <!-- 顶部全局轻提示胶囊 -->
    <div v-if="playerStore.toastMessage" class="fluent-toast">
      {{ playerStore.toastMessage }}
    </div>

    <TitleBar />
    <div class="main-body">
      <NavigationRail />
      <main class="page-content">
        <DiscoverView v-show="appStore.currentTab === 'discover' && !appStore.activePlaylist" />
        <DailyRecView v-show="appStore.currentTab === 'daily' && !appStore.activePlaylist" />
        <RankingsView v-show="appStore.currentTab === 'rankings' && !appStore.activePlaylist" />
        <LibraryView v-show="appStore.currentTab === 'library' && !appStore.activePlaylist" />
        <SearchView v-show="appStore.currentTab === 'search' && !appStore.activePlaylist" />
        <HistoryView v-show="appStore.currentTab === 'history' && !appStore.activePlaylist" />
        <SettingsView v-show="appStore.currentTab === 'settings' && !appStore.activePlaylist" />
        <PlaylistDetailView v-if="appStore.activePlaylist" />
      </main>
    </div>
    <PlayerBar />
    <LyricOverlay />
    <QueueDrawer />
    <LoginModal />
  </div>
</template>

<script setup lang="ts">
import { onMounted, onUnmounted } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { useAppStore, type NavTab } from './stores/app';
import { usePlayerStore } from './stores/player';

import TitleBar from './components/TitleBar.vue';
import NavigationRail from './components/NavigationRail.vue';
import PlayerBar from './components/PlayerBar.vue';
import LyricOverlay from './components/LyricOverlay.vue';
import QueueDrawer from './components/QueueDrawer.vue';
import LoginModal from './components/LoginModal.vue';

import DiscoverView from './views/DiscoverView.vue';
import DailyRecView from './views/DailyRecView.vue';
import RankingsView from './views/RankingsView.vue';
import LibraryView from './views/LibraryView.vue';
import SearchView from './views/SearchView.vue';
import HistoryView from './views/HistoryView.vue';
import SettingsView from './views/SettingsView.vue';
import PlaylistDetailView from './views/PlaylistDetailView.vue';

const appStore = useAppStore();
const playerStore = usePlayerStore();
let unlistenTrayNav: UnlistenFn | null = null;

onMounted(async () => {
  await appStore.initApp();
  playerStore.startPolling();
  try {
    unlistenTrayNav = await listen<string>('tray-navigate', (event) => {
      if (event.payload) {
        appStore.setTab(event.payload as NavTab);
      }
    });
    const pendingTab = await invoke<string | null>('take_pending_nav_tab');
    if (pendingTab) {
      appStore.setTab(pendingTab as NavTab);
    }
  } catch {}
  try {
    await invoke('frontend_ready');
  } catch {}
});

onUnmounted(() => {
  playerStore.stopPolling();
  if (unlistenTrayNav) {
    unlistenTrayNav();
    unlistenTrayNav = null;
  }
});
</script>

<style scoped>
.app-layout {
  width: 100vw;
  height: 100vh;
  display: flex;
  flex-direction: column;
  background-color: var(--bg-app);
  overflow: hidden;
  position: relative;
}

.fluent-toast {
  position: fixed;
  top: 54px;
  left: 50%;
  transform: translateX(-50%);
  padding: 6px 16px;
  border-radius: var(--radius-full);
  background: var(--bg-popover);
  border: 1px solid var(--border-subtle);
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.16);
  font-size: 12.5px;
  font-weight: 500;
  color: var(--text-primary);
  z-index: 400;
  pointer-events: none;
}

.main-body {
  flex: 1;
  display: flex;
  min-height: 0;
  position: relative;
}

.page-content {
  flex: 1;
  height: 100%;
  overflow: hidden;
  background: transparent;
  position: relative;
}
</style>
