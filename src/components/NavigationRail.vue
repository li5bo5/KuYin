<template>
  <nav class="navigation-rail">
    <div class="nav-items">
      <button
        v-for="item in navList"
        :key="item.tab"
        :class="['nav-btn', { active: appStore.currentTab === item.tab }]"
        @click="appStore.setTab(item.tab)"
      >
        <SvgIcon :name="item.icon" :size="20" class="nav-icon" />
        <span class="nav-label">{{ item.label }}</span>
      </button>
    </div>
  </nav>
</template>

<script setup lang="ts">
import { useAppStore, type NavTab } from '../stores/app';
import SvgIcon from './SvgIcon.vue';

const appStore = useAppStore();

// 按用户需求：FM 已并入「发现」，「设置」已移至右上角用户信息右侧；全部采用 EchoMusic Tabler 矢量图标
const navList: Array<{ tab: NavTab; label: string; icon: string }> = [
  { tab: 'discover', label: '发现', icon: 'compass' },
  { tab: 'daily', label: '推荐', icon: 'sparkles' },
  { tab: 'rankings', label: '榜单', icon: 'trophy' },
  { tab: 'library', label: '歌单', icon: 'vinyl' },
  { tab: 'history', label: '历史', icon: 'clock' },
];
</script>

<style scoped>
.navigation-rail {
  width: var(--nav-width);
  height: 100%;
  background: var(--bg-nav);
  border-right: 1px solid var(--border-divider);
  display: flex;
  flex-direction: column;
  align-items: center;
  padding: 12px 0;
  backdrop-filter: blur(20px);
}

.nav-items {
  display: flex;
  flex-direction: column;
  gap: 6px;
  width: 100%;
  align-items: center;
}

.nav-btn {
  width: 54px;
  height: 52px;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 4px;
  border-radius: var(--radius-md);
  color: var(--text-secondary);
  transition: all var(--duration-fast);
}

.nav-btn:hover {
  background: var(--bg-card-hover);
  color: var(--text-primary);
}

.nav-btn.active {
  background: var(--bg-card);
  color: var(--accent-color);
  font-weight: 600;
  border: 1px solid var(--border-subtle);
  box-shadow: 0 2px 6px rgba(0, 0, 0, 0.04);
}

.nav-label {
  font-size: 11px;
}
</style>
