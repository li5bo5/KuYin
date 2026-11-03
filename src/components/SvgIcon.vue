<script setup lang="ts">
withDefaults(
  defineProps<{
    name: string;
    size?: number | string;
    strokeWidth?: number | string;
  }>(),
  {
    size: 18,
    strokeWidth: 1.85,
  }
);
</script>

<template>
  <!-- EchoMusic (Tabler Icons) 风格统一线性/填充矢量图标集（零外部依赖） -->
  <svg
    class="kuyin-svg-icon"
    :width="size"
    :height="size"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    :stroke-width="strokeWidth"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
  >
    <!-- 发现 (compass) -->
    <template v-if="name === 'compass'">
      <polyline points="8 16 10 10 16 8 14 14 8 16" />
      <circle cx="12" cy="12" r="9" />
    </template>

    <!-- 推荐 (sparkles / music-star) -->
    <template v-else-if="name === 'sparkles'">
      <path d="M16 18a2 2 0 0 1 2 2a2 2 0 0 1 2 -2a2 2 0 0 1 -2 -2a2 2 0 0 1 -2 2zm0 -12a2 2 0 0 1 2 2a2 2 0 0 1 2 -2a2 2 0 0 1 -2 -2a2 2 0 0 1 -2 2zm-7 12a6 6 0 0 1 6 -6a6 6 0 0 1 -6 -6a6 6 0 0 1 -6 6a6 6 0 0 1 6 6z" />
    </template>

    <!-- 排行榜 (trophy) -->
    <template v-else-if="name === 'trophy'">
      <line x1="8" y1="21" x2="16" y2="21" />
      <line x1="12" y1="17" x2="12" y2="21" />
      <line x1="7" y1="4" x2="17" y2="4" />
      <path d="M17 4v8a5 5 0 0 1 -10 0v-8" />
      <circle cx="5" cy="9" r="2" />
      <circle cx="19" cy="9" r="2" />
    </template>

    <!-- 歌单 / 唱片 (vinyl) -->
    <template v-else-if="name === 'vinyl'">
      <path d="M16 3.937a9 9 0 1 0 5 8.063" />
      <circle cx="12" cy="12" r="1" />
      <circle cx="20" cy="4" r="1" />
      <path d="M20 4l-3.5 10l-2.5 2" />
    </template>

    <!-- 历史 (clock) -->
    <template v-else-if="name === 'clock'">
      <circle cx="12" cy="12" r="9" />
      <polyline points="12 7 12 12 15 15" />
    </template>

    <!-- 私人 FM 电台 (radio) -->
    <template v-else-if="name === 'radio'">
      <path d="M14 3l-9.371 3.749a1 1 0 0 0 -.629 .928v11.323a1 1 0 0 0 1 1h14a1 1 0 0 0 1 -1v-11a1 1 0 0 0 -1 -1h-14.5" />
      <path d="M4 12h16" />
      <path d="M7 12v-2" />
      <path d="M17 16v.01" />
      <path d="M13 16v.01" />
    </template>

    <!-- 搜索 (search) -->
    <template v-else-if="name === 'search'">
      <circle cx="10" cy="10" r="7" />
      <line x1="21" y1="21" x2="15" y2="15" />
    </template>

    <!-- 设置 (settings) -->
    <template v-else-if="name === 'settings'">
      <path d="M10.325 4.317c.426 -1.756 2.924 -1.756 3.35 0a1.724 1.724 0 0 0 2.573 1.066c1.543 -.94 3.31 .826 2.37 2.37a1.724 1.724 0 0 0 1.065 2.572c1.756 .426 1.756 2.924 0 3.35a1.724 1.724 0 0 0 -1.066 2.573c.94 1.543 -.826 3.31 -2.37 2.37a1.724 1.724 0 0 0 -2.572 1.065c-.426 1.756 -2.924 1.756 -3.35 0a1.724 1.724 0 0 0 -2.573 -1.066c-1.543 .94 -3.31 -.826 -2.37 -2.37a1.724 1.724 0 0 0 -1.065 -2.572c-1.756 -.426 -1.756 -2.924 0 -3.35a1.724 1.724 0 0 0 1.066 -2.573c-.94 -1.543 .826 -3.31 2.37 -2.37c1 .608 2.296 .07 2.572 -1.065z" />
      <circle cx="12" cy="12" r="3" />
    </template>

    <!-- 用户 (user) -->
    <template v-else-if="name === 'user'">
      <circle cx="12" cy="7" r="4" />
      <path d="M6 21v-2a4 4 0 0 1 4 -4h4a4 4 0 0 1 4 4v2" />
    </template>

    <!-- 皇冠 / VIP (crown) -->
    <template v-else-if="name === 'crown'">
      <path d="M12 6l4 6l5 -4l-2 10h-14l-2 -10l5 4z" />
    </template>

    <!-- 播放 (play - filled) -->
    <template v-else-if="name === 'play'">
      <path d="M7 4v16l13 -8z" fill="currentColor" stroke="none" />
    </template>

    <!-- 暂停 (pause - filled) -->
    <template v-else-if="name === 'pause'">
      <rect x="6" y="5" width="4" height="14" rx="1" fill="currentColor" stroke="none" />
      <rect x="14" y="5" width="4" height="14" rx="1" fill="currentColor" stroke="none" />
    </template>

    <!-- 上一首 (player-skip-back) -->
    <template v-else-if="name === 'skip-back'">
      <path d="M20 5v14l-12 -7z" />
      <line x1="4" y1="5" x2="4" y2="19" />
    </template>

    <!-- 下一首 (player-skip-forward) -->
    <template v-else-if="name === 'skip-forward'">
      <path d="M4 5v14l12 -7z" />
      <line x1="20" y1="5" x2="20" y2="19" />
    </template>

    <!-- 空心喜欢 (heart) -->
    <template v-else-if="name === 'heart'">
      <path d="M19.5 12.572l-7.5 7.428l-7.5 -7.428a5 5 0 1 1 7.5 -6.566a5 5 0 1 1 7.5 6.572" />
    </template>

    <!-- 实心喜欢 (heart-filled) -->
    <template v-else-if="name === 'heart-filled'">
      <path
        d="M6.979 3.074a6 6 0 0 1 4.988 1.425l.037 .033l.034 -.03a6 6 0 0 1 4.733 -1.44l.246 .036a6 6 0 0 1 3.364 10.008l-.18 .185l-.048 .041l-7.45 7.379a1 1 0 0 1 -1.313 .082l-.094 -.082l-7.493 -7.422a6 6 0 0 1 3.176 -10.215z"
        fill="currentColor"
        stroke="none"
      />
    </template>

    <!-- 高音量 (volume) -->
    <template v-else-if="name === 'volume'">
      <path d="M15 8a5 5 0 0 1 0 8" />
      <path d="M17.7 5a9 9 0 0 1 0 14" />
      <path d="M6 15h-2a1 1 0 0 1 -1 -1v-4a1 1 0 0 1 1 -1h2l3.5 -4.5a.8 .8 0 0 1 1.5 .5v14a.8 .8 0 0 1 -1.5 .5l-3.5 -4.5" />
    </template>

    <!-- 低音量 (volume-low) -->
    <template v-else-if="name === 'volume-low'">
      <path d="M15 8a5 5 0 0 1 0 8" />
      <path d="M6 15h-2a1 1 0 0 1 -1 -1v-4a1 1 0 0 1 1 -1h2l3.5 -4.5a.8 .8 0 0 1 1.5 .5v14a.8 .8 0 0 1 -1.5 .5l-3.5 -4.5" />
    </template>

    <!-- 静音 (volume-off) -->
    <template v-else-if="name === 'volume-off'">
      <path d="M15 8a5 5 0 0 1 1.912 4.934m-1.377 2.602a5 5 0 0 1 -.535 .464" />
      <path d="M17.7 5a9 9 0 0 1 2.362 11.086m-1.676 2.299a9 9 0 0 1 -.686 .615" />
      <path d="M9.069 5.054l.431 -.554a.8 .8 0 0 1 1.5 .5v2m0 4v8a.8 .8 0 0 1 -1.5 .5l-3.5 -4.5h-2a1 1 0 0 1 -1 -1v-4a1 1 0 0 1 1 -1h2l1.294 -1.664" />
      <path d="M3 3l18 18" />
    </template>

    <!-- 播放列表 / 队列 (playlist) -->
    <template v-else-if="name === 'playlist'">
      <circle cx="14" cy="17" r="3" />
      <path d="M17 17v-13h4" />
      <path d="M13 5h-10" />
      <line x1="3" y1="9" x2="13" y2="9" />
      <path d="M9 13h-6" />
    </template>

    <!-- 添加到播放队列 (playlist-add) -->
    <template v-else-if="name === 'playlist-add'">
      <path d="M19 8h-14" />
      <path d="M5 12h9" />
      <path d="M11 16h-6" />
      <path d="M15 16h6" />
      <path d="M18 13v6" />
    </template>

    <!-- 音乐音符 (music) -->
    <template v-else-if="name === 'music'">
      <circle cx="6" cy="17" r="3" />
      <circle cx="16" cy="17" r="3" />
      <polyline points="9 17 9 4 19 4 19 17" />
      <line x1="9" y1="8" x2="19" y2="8" />
    </template>

    <!-- 专辑光盘 (disc) -->
    <template v-else-if="name === 'disc'">
      <circle cx="12" cy="12" r="9" />
      <circle cx="12" cy="12" r="2" />
      <path d="M7 12a5 5 0 0 1 5 -5" />
      <path d="M12 17a5 5 0 0 0 5 -5" />
    </template>

    <!-- 删除 / 垃圾桶 (trash) -->
    <template v-else-if="name === 'trash'">
      <line x1="4" y1="7" x2="20" y2="7" />
      <line x1="10" y1="11" x2="10" y2="17" />
      <line x1="14" y1="11" x2="14" y2="17" />
      <path d="M5 7l1 12a2 2 0 0 0 2 2h8a2 2 0 0 0 2 -2l1 -12" />
      <path d="M9 7v-3a1 1 0 0 1 1 -1h4a1 1 0 0 1 1 1v3" />
    </template>

    <!-- 加号 (plus) -->
    <template v-else-if="name === 'plus'">
      <line x1="12" y1="5" x2="12" y2="19" />
      <line x1="5" y1="12" x2="19" y2="12" />
    </template>

    <!-- 窗口最小化 (minus) -->
    <template v-else-if="name === 'minimize'">
      <line x1="5" y1="12" x2="19" y2="12" />
    </template>

    <!-- 窗口最大化 (square) -->
    <template v-else-if="name === 'maximize'">
      <rect x="5" y="5" width="14" height="14" rx="1.8" />
    </template>

    <!-- 关闭 / 叉号 (x) -->
    <template v-else-if="name === 'close'">
      <line x1="18" y1="6" x2="6" y2="18" />
      <line x1="6" y1="6" x2="18" y2="18" />
    </template>

    <!-- 太阳 / 浅色模式 (sun) -->
    <template v-else-if="name === 'sun'">
      <circle cx="12" cy="12" r="4" />
      <path d="M3 12h1m8 -9v1m8 8h1m-9 8v1m-6.4 -15.4l.7 .7m12.1 -.7l-.7 .7m0 11.4l.7 .7m-12.1 -.7l-.7 .7" />
    </template>

    <!-- 月亮 / 深色模式 (moon) -->
    <template v-else-if="name === 'moon'">
      <path d="M12 3c.132 0 .263 0 .393 0a7.5 7.5 0 0 0 7.92 12.446a9 9 0 1 1 -8.313 -12.454z" />
    </template>

    <!-- 文件夹打开 (folder-open) -->
    <template v-else-if="name === 'folder'">
      <path d="M5 19l2.757 -7.351a1 1 0 0 1 .936 -.649h12.307a1 1 0 0 1 .986 1.164l-.996 5.211a2 2 0 0 1 -1.964 1.625h-14.026a2 2 0 0 1 -2 -2v-11a2 2 0 0 1 2 -2h4l3 3h7a2 2 0 0 1 2 2v2" />
    </template>

    <!-- 刷新 (refresh) -->
    <template v-else-if="name === 'refresh'">
      <path d="M20 11a8.1 8.1 0 0 0 -15.5 -2m-.5 -4v4h4" />
      <path d="M4 13a8.1 8.1 0 0 0 15.5 2m.5 4v-4h-4" />
    </template>

    <!-- 方向箭头与分页 (chevron / arrow) -->
    <template v-else-if="name === 'chevron-left'">
      <polyline points="15 6 9 12 15 18" />
    </template>
    <template v-else-if="name === 'chevron-right'">
      <polyline points="9 6 15 12 9 18" />
    </template>
    <template v-else-if="name === 'chevron-up'">
      <polyline points="6 15 12 9 18 15" />
    </template>
    <template v-else-if="name === 'chevron-down'">
      <polyline points="6 9 12 15 18 9" />
    </template>
    <template v-else-if="name === 'arrow-up'">
      <line x1="12" y1="5" x2="12" y2="19" />
      <line x1="18" y1="11" x2="12" y2="5" />
      <line x1="6" y1="11" x2="12" y2="5" />
    </template>
    <template v-else-if="name === 'arrow-down'">
      <line x1="12" y1="5" x2="12" y2="19" />
      <line x1="18" y1="13" x2="12" y2="19" />
      <line x1="6" y1="13" x2="12" y2="19" />
    </template>

    <!-- 安全备份盾牌 (shield-check) -->
    <template v-else-if="name === 'shield-check'">
      <path d="M9 12l2 2l4 -4" />
      <path d="M12 3a12 12 0 0 0 8.5 3a12 12 0 0 1 -8.5 15a12 12 0 0 1 -8.5 -15a12 12 0 0 0 8.5 -3" />
    </template>

    <!-- 退出登录 (logout) -->
    <template v-else-if="name === 'logout'">
      <path d="M14 8v-2a2 2 0 0 0 -2 -2h-7a2 2 0 0 0 -2 2v12a2 2 0 0 0 2 2h7a2 2 0 0 0 2 -2v-2" />
      <path d="M7 12h14l-3 -3m0 6l3 -3" />
    </template>

    <!-- 扬声器输出设备 (speaker) -->
    <template v-else-if="name === 'speaker'">
      <rect x="5" y="3" width="14" height="18" rx="2" />
      <circle cx="12" cy="14" r="3" />
      <line x1="12" y1="7" x2="12" y2="7.01" />
    </template>

    <!-- 键盘快捷键 (keyboard) -->
    <template v-else-if="name === 'keyboard'">
      <rect x="2" y="6" width="20" height="12" rx="2" />
      <line x1="6" y1="10" x2="6" y2="10" />
      <line x1="10" y1="10" x2="10" y2="10" />
      <line x1="14" y1="10" x2="14" y2="10" />
      <line x1="18" y1="10" x2="18" y2="10" />
      <line x1="6" y1="14" x2="6" y2="14.01" />
      <line x1="18" y1="14" x2="18" y2="14.01" />
      <line x1="10" y1="14" x2="14" y2="14" />
    </template>

    <!-- GitHub 开源仓库 (github) -->
    <template v-else-if="name === 'github'">
      <path d="M9 19c-4.3 1.4 -4.3 -2.5 -6 -3m12 5v-3.5c0 -1 .1 -1.4 -.5 -2c2.8 -.3 5.5 -1.4 5.5 -6a4.6 4.6 0 0 0 -1.3 -3.2a4.2 4.2 0 0 0 -.1 -3.2s-1.1 -.3 -3.5 1.3a12.3 12.3 0 0 0 -6.2 0c-2.4 -1.6 -3.5 -1.3 -3.5 -1.3a4.2 4.2 0 0 0 -.1 3.2a4.6 4.6 0 0 0 -1.3 3.2c0 4.6 2.7 5.7 5.5 6c-.6 .6 -.6 1.2 -.5 2v3.5" />
    </template>

    <!-- 外部链接打开 (external-link) -->
    <template v-else-if="name === 'external-link'">
      <path d="M12 6h-6a2 2 0 0 0 -2 2v10a2 2 0 0 0 2 2h10a2 2 0 0 0 2 -2v-6" />
      <path d="M11 13l9 -9" />
      <path d="M15 4h5v5" />
    </template>

    <!-- 复制链接 (copy) -->
    <template v-else-if="name === 'copy'">
      <rect x="8" y="8" width="12" height="12" rx="2" />
      <path d="M16 8v-2a2 2 0 0 0 -2 -2h-8a2 2 0 0 0 -2 2v8a2 2 0 0 0 2 2h2" />
    </template>

    <!-- 默认兜底圆点 -->
    <template v-else>
      <circle cx="12" cy="12" r="8" />
    </template>
  </svg>
</template>

<style scoped>
.kuyin-svg-icon {
  display: inline-block;
  vertical-align: middle;
  flex-shrink: 0;
}
</style>
