<template>
  <div v-if="playerStore.showQueue" class="queue-drawer-mask" @click.self="playerStore.toggleQueue">
    <aside class="queue-drawer">
      <div class="drawer-header">
        <div>
          <h3 class="drawer-title">播放队列</h3>
          <span class="drawer-sub">共 {{ playerStore.queueTracks.length }} 首曲目</span>
        </div>
        <div class="header-actions">
          <button
            class="fluent-btn"
            :disabled="playerStore.queueTracks.length === 0"
            @click="playerStore.clearQueue"
          >
            <SvgIcon name="trash" :size="14" />
            清空队列
          </button>
        </div>
      </div>

      <div v-if="playerStore.queueTracks.length === 0" class="empty-queue">
        当前播放队列为空
      </div>

      <!-- 超过 20 首自动启用每页 20 首翻页，且屏内挂载 DOM 严格 ≤ 20 行 -->
      <div v-else class="queue-list">
        <div
          v-for="(item, i) in pagedQueueItems"
          :key="`${item.hash}-${pageStart + i}`"
          :class="['queue-row', { active: pageStart + i === playerStore.currentIndex }]"
          @dblclick="playerStore.playQueueIndex(pageStart + i)"
        >
          <div class="q-meta" @click="playerStore.playQueueIndex(pageStart + i)">
            <span class="q-name" :title="item.name">{{ item.name }}</span>
            <span class="q-singer" :title="item.singer">{{ item.singer }}</span>
          </div>
          <div class="q-actions">
            <button
              class="q-act-btn"
              title="上移"
              :disabled="pageStart + i === 0"
              @click.stop="playerStore.moveQueueItem(pageStart + i, pageStart + i - 1)"
            >
              <SvgIcon name="arrow-up" :size="13" />
            </button>
            <button
              class="q-act-btn"
              title="下移"
              :disabled="pageStart + i >= playerStore.queueTracks.length - 1"
              @click.stop="playerStore.moveQueueItem(pageStart + i, pageStart + i + 1)"
            >
              <SvgIcon name="arrow-down" :size="13" />
            </button>
            <button
              class="q-act-btn danger"
              title="从队列移除"
              @click.stop="playerStore.removeFromQueue(pageStart + i)"
            >
              <SvgIcon name="close" :size="13" />
            </button>
          </div>
        </div>
      </div>

      <div v-if="totalPages > 1" class="fluent-pagination queue-pager">
        <button class="fluent-btn pager-btn" :disabled="page <= 1" @click="page--">
          <SvgIcon name="chevron-left" :size="14" class="pg-icon" />
          <span class="pg-text">上一页</span>
        </button>
        <span class="fluent-page-info">第 {{ page }} / {{ totalPages }} 页</span>
        <button class="fluent-btn pager-btn" :disabled="page >= totalPages" @click="page++">
          <span class="pg-text">下一页</span>
          <SvgIcon name="chevron-right" :size="14" class="pg-icon" />
        </button>
      </div>
    </aside>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch } from 'vue';
import { usePlayerStore } from '../stores/player';
import SvgIcon from './SvgIcon.vue';

const playerStore = usePlayerStore();
const PAGE_SIZE = 20;
const page = ref(1);

const totalPages = computed(() =>
  Math.max(1, Math.ceil(playerStore.queueTracks.length / PAGE_SIZE))
);
const pageStart = computed(() => (page.value - 1) * PAGE_SIZE);
const pagedQueueItems = computed(() =>
  playerStore.queueTracks.slice(pageStart.value, pageStart.value + PAGE_SIZE)
);

watch(totalPages, (tp) => {
  if (page.value > tp) {
    page.value = tp;
  }
});

watch(
  () => playerStore.showQueue,
  (shown) => {
    if (shown && playerStore.currentIndex !== null) {
      page.value = Math.floor(playerStore.currentIndex / PAGE_SIZE) + 1;
    }
  }
);
</script>

<style scoped>
.queue-drawer-mask {
  position: absolute;
  top: var(--titlebar-height);
  left: 0;
  right: 0;
  bottom: var(--player-height);
  background: rgba(0, 0, 0, 0.18);
  display: flex;
  justify-content: flex-end;
  z-index: 95;
}

.queue-drawer {
  width: 350px;
  height: 100%;
  background: var(--bg-player);
  border-left: 1px solid var(--border-subtle);
  backdrop-filter: blur(28px);
  display: flex;
  flex-direction: column;
  padding: 16px;
}

.drawer-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding-bottom: 12px;
  border-bottom: 1px solid var(--border-divider);
}

.drawer-title {
  font-size: 15px;
  font-weight: 700;
}

.drawer-sub {
  font-size: 11px;
  color: var(--text-tertiary);
}

.header-actions {
  display: flex;
  align-items: center;
  gap: 6px;
}

.empty-queue {
  margin-top: 80px;
  text-align: center;
  font-size: 13px;
  color: var(--text-tertiary);
}

.queue-list {
  flex: 1;
  overflow-y: auto;
  margin-top: 8px;
}

.queue-row {
  height: 56px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 10px;
  border-radius: var(--radius-md);
  cursor: pointer;
}

.queue-row:hover {
  background: var(--bg-card-hover);
}

.queue-row.active .q-name {
  color: var(--accent-color);
  font-weight: 600;
}

.q-meta {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.q-name {
  font-size: 13px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.q-singer {
  font-size: 11px;
  color: var(--text-secondary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.q-actions {
  display: flex;
  align-items: center;
  gap: 2px;
  opacity: 0;
  transition: opacity var(--duration-fast);
}

.queue-row:hover .q-actions {
  opacity: 1;
}

.q-act-btn {
  width: 24px;
  height: 24px;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-tertiary);
  border-radius: var(--radius-sm);
}

.q-act-btn:disabled {
  opacity: 0.3;
  cursor: not-allowed;
}

.q-act-btn:not(:disabled):hover {
  background: var(--bg-card);
  color: var(--text-primary);
}

.q-act-btn.danger:hover {
  background: rgba(232, 17, 35, 0.14);
  color: #e81123;
}

.queue-pager {
  border-top: 1px solid var(--border-divider);
  padding: 10px 0 0;
}

.pager-btn {
  padding: 4px 8px;
}
</style>
