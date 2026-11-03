<template>
  <div class="view-container">
    <div class="view-header">
      <h1 class="view-title">设置中心</h1>
      <p class="view-subtitle">外观主题、自定义缓存位置、音频设备、关闭休眠与全局热键</p>
    </div>

    <div class="settings-content">
      <!-- 1. 外观主题与关闭窗口行为 -->
      <div class="setting-card">
        <h3 class="card-title">
          <SvgIcon name="sun" :size="17" />
          <span>界面外观与系统托盘策略</span>
        </h3>
        <p class="card-desc">默认采用清爽浅色云母主题；关闭主窗口默认隐藏至系统托盘并释放前端内存</p>

        <div class="setting-row">
          <span class="row-label">外观主题</span>
          <FluentSelect
            class="row-select"
            :model-value="appStore.theme"
            :options="themeOptions"
            @change="handleThemeChange"
          />
        </div>

        <div class="setting-row mt-row">
          <span class="row-label">关闭按钮</span>
          <FluentSelect
            v-model="closeBehavior"
            class="row-select"
            :options="closeBehaviorOptions"
            @change="handleCloseBehaviorChange"
          />
        </div>
      </div>

      <!-- 2. 自定义缓存与数据存储位置 -->
      <div class="setting-card">
        <h3 class="card-title">
          <SvgIcon name="folder" :size="17" />
          <span>自定义缓存与数据文件位置</span>
        </h3>
        <p class="card-desc">自定义本地配置数据库、歌单快照与运行日志存储目录（支持启动/退出自动回收过期临时缓存）</p>

        <div class="setting-row">
          <input
            v-model="storageDir"
            class="fluent-input dir-input"
            readonly
            :title="storageDir"
          />
          <button class="fluent-btn" @click="handlePickStorageDir">
            <SvgIcon name="folder" :size="14" />
            更改目录
          </button>
        </div>

        <div class="setting-row mt-row btn-wrap-row">
          <button class="fluent-btn" @click="openStorageDir">
            <SvgIcon name="folder" :size="14" />
            缓存目录
          </button>
          <button class="fluent-btn" @click="cleanTempCacheNow">
            <SvgIcon name="trash" :size="14" />
            清理缓存
          </button>
          <button class="fluent-btn" @click="backupNow">
            <SvgIcon name="shield-check" :size="14" />
            备份数据
          </button>
          <button class="fluent-btn" @click="openLogs">
            <SvgIcon name="clock" :size="14" />
            日志目录
          </button>
        </div>
      </div>

      <!-- 3. 原生音频输出与音质 -->
      <div class="setting-card">
        <h3 class="card-title">
          <SvgIcon name="speaker" :size="17" />
          <span>原生音频输出与音质偏好</span>
        </h3>
        <p class="card-desc">基于 Windows 11 内核 MediaPlayer 硬件解码与音频设备热切换</p>

        <div class="setting-row">
          <span class="row-label">输出设备</span>
          <FluentSelect
            v-model="selectedDeviceId"
            class="row-select"
            :options="deviceOptions"
            @change="handleDeviceChange"
          />
          <button class="fluent-btn" @click="loadDevices">
            <SvgIcon name="refresh" :size="14" />
            刷新设备
          </button>
        </div>

        <div class="setting-row mt-row">
          <span class="row-label">首选音质</span>
          <FluentSelect
            class="row-select"
            :model-value="playerStore.quality"
            :options="qualityOptions"
            @change="handleQualityChange"
          />
        </div>
      </div>

      <!-- 4. 全局快捷键配置 -->
      <div class="setting-card">
        <h3 class="card-title">
          <SvgIcon name="keyboard" :size="17" />
          <span>全局媒体快捷键</span>
        </h3>
        <p class="card-desc">隐藏至系统托盘后依然可在任意前台窗口响应全局热键</p>

        <div class="shortcut-grid">
          <div class="sc-field">
            <label>播放 / 暂停</label>
            <input v-model="shortcuts.play_pause" class="fluent-input" />
          </div>
          <div class="sc-field">
            <label>下一首</label>
            <input v-model="shortcuts.next" class="fluent-input" />
          </div>
          <div class="sc-field">
            <label>上一首</label>
            <input v-model="shortcuts.prev" class="fluent-input" />
          </div>
          <div class="sc-field">
            <label>音量增加</label>
            <input v-model="shortcuts.vol_up" class="fluent-input" />
          </div>
          <div class="sc-field">
            <label>音量减小</label>
            <input v-model="shortcuts.vol_down" class="fluent-input" />
          </div>
          <div class="sc-field">
            <label>唤醒 / 隐藏主窗口</label>
            <input v-model="shortcuts.toggle_window" class="fluent-input" />
          </div>
        </div>

        <div class="setting-row mt-row">
          <button class="fluent-btn" @click="saveShortcuts">
            保存热键
          </button>
          <span v-if="conflictTip" class="conflict-tip">{{ conflictTip }}</span>
        </div>
      </div>

      <!-- 5. 关于酷音与开源仓库 -->
      <div class="setting-card">
        <h3 class="card-title">
          <SvgIcon name="github" :size="17" />
          <span>关于酷音与开源仓库</span>
        </h3>
        <p class="card-desc">酷音 KuYin v1.0（首发正式版 · MIT 开源协议）· Windows 11 极简超低内存酷狗概念版音乐播放器</p>

        <div class="setting-row">
          <span class="row-label">仓库地址</span>
          <input
            :value="repoUrl"
            class="fluent-input dir-input"
            readonly
            :title="repoUrl"
          />
          <button class="fluent-btn fluent-btn-primary" @click="openRepoUrl">
            <SvgIcon name="external-link" :size="14" />
            访问仓库
          </button>
          <button class="fluent-btn" @click="copyRepoUrl">
            <SvgIcon name="copy" :size="14" />
            复制链接
          </button>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, reactive, computed, onMounted } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import SvgIcon from '../components/SvgIcon.vue';
import FluentSelect from '../components/FluentSelect.vue';
import { useAppStore, type ThemePreference } from '../stores/app';
import { usePlayerStore } from '../stores/player';

const themeOptions = [
  { value: 'light', label: '浅色模式（默认 · Mica Light）' },
  { value: 'dark', label: '深色模式（Mica Dark）' },
  { value: 'system', label: '跟随 Windows 系统深浅色' },
];

const closeBehaviorOptions = [
  { value: 'tray', label: '隐藏到系统托盘（推荐 · 销毁 WebView2 后台常驻）' },
  { value: 'exit', label: '直接退出酷音程序' },
];

const qualityOptions = [
  { value: '128', label: '标准音质 (128K · 省流)' },
  { value: '320', label: '高品质 (320K · 推荐)' },
  { value: 'flac', label: '无损音质 (FLAC · 需概念版 VIP，无资源自动降级)' },
];

interface DeviceItem {
  id: string;
  name: string;
  is_default: boolean;
}

interface ShortcutConfig {
  play_pause: string;
  next: string;
  prev: string;
  vol_up: string;
  vol_down: string;
  toggle_window: string;
}

const appStore = useAppStore();
const playerStore = usePlayerStore();

const devices = ref<DeviceItem[]>([]);
const selectedDeviceId = ref('');
const closeBehavior = ref('tray');
const storageDir = ref('');
const conflictTip = ref('');
const repoUrl = 'https://github.com/li5bo5/KuYin';

const deviceOptions = computed(() => [
  { value: '', label: '跟随 Windows 系统默认设备' },
  ...devices.value.map((dev) => ({
    value: dev.id,
    label: `${dev.name}${dev.is_default ? ' (默认)' : ''}`,
  })),
]);

const shortcuts = reactive<ShortcutConfig>({
  play_pause: 'Ctrl+Alt+Space',
  next: 'Ctrl+Alt+Right',
  prev: 'Ctrl+Alt+Left',
  vol_up: 'Ctrl+Alt+Up',
  vol_down: 'Ctrl+Alt+Down',
  toggle_window: 'Ctrl+Alt+M',
});

async function loadDevices() {
  try {
    devices.value = await invoke<DeviceItem[]>('list_audio_devices');
  } catch (err) {
    console.warn('获取音频设备失败:', err);
  }
}

async function loadSettings() {
  try {
    const res = await invoke<{
      theme: string;
      close_behavior: string;
      selected_device_id: string;
      storage_dir: string;
      shortcuts: ShortcutConfig;
    }>('get_app_settings');
    closeBehavior.value = res.close_behavior || 'tray';
    selectedDeviceId.value = res.selected_device_id || '';
    storageDir.value = res.storage_dir || '';
    if (res.shortcuts) {
      Object.assign(shortcuts, res.shortcuts);
    }
  } catch (err) {
    console.warn('读取设置项失败:', err);
  }
}

function handleThemeChange(val: string) {
  appStore.applyThemePreference(val as ThemePreference, true);
}

async function handlePickStorageDir() {
  try {
    const picked = await invoke<string | null>('pick_storage_folder');
    if (!picked) return;
    const saved = await invoke<string>('set_storage_dir', { newDir: picked });
    storageDir.value = saved;
    playerStore.showToast('缓存目录已更新并迁移数据，重启酷音后完全生效');
  } catch (err: any) {
    playerStore.showToast(`更改缓存目录失败：${String(err)}`);
  }
}

async function openStorageDir() {
  await invoke('open_storage_folder');
}

async function cleanTempCacheNow() {
  try {
    const msg = await invoke<string>('clean_temp_cache');
    playerStore.showToast(msg);
  } catch (err: any) {
    playerStore.showToast(`清理临时缓存失败：${String(err)}`);
  }
}

async function backupNow() {
  try {
    await invoke('backup_database_now');
    playerStore.showToast('配置与歌单数据库已成功备份 (.bak)');
  } catch (err: any) {
    playerStore.showToast(`备份失败：${String(err)}`);
  }
}

async function handleDeviceChange() {
  try {
    await invoke('set_audio_device', {
      deviceId: selectedDeviceId.value ? selectedDeviceId.value : null,
    });
    playerStore.showToast('音频输出设备已切换');
  } catch (err) {
    console.warn('切换音频设备失败:', err);
  }
}

function handleQualityChange(val: string) {
  playerStore.setQuality(val);
}

async function handleCloseBehaviorChange() {
  await invoke('set_close_behavior', { behavior: closeBehavior.value });
  playerStore.showToast(
    closeBehavior.value === 'tray'
      ? '已设为关闭按钮隐藏到系统托盘'
      : '已设为关闭按钮直接退出程序'
  );
}

async function saveShortcuts() {
  conflictTip.value = '';
  try {
    const res = await invoke<{ conflicts: string[] }>('update_shortcuts', {
      config: { ...shortcuts },
    });
    if (res.conflicts && res.conflicts.length > 0) {
      conflictTip.value = `冲突或无效热键：${res.conflicts.join(', ')}`;
    } else {
      playerStore.showToast('全局快捷键已更新生效');
    }
  } catch (err: any) {
    conflictTip.value = String(err);
  }
}

async function openLogs() {
  await invoke('open_log_folder');
}

async function openRepoUrl() {
  try {
    await invoke('open_external_url', { url: repoUrl });
  } catch (err: any) {
    playerStore.showToast(`打开仓库链接失败：${String(err)}`);
  }
}

async function copyRepoUrl() {
  try {
    await navigator.clipboard.writeText(repoUrl);
    playerStore.showToast('已复制开源仓库地址');
  } catch {
    playerStore.showToast(`仓库地址：${repoUrl}`);
  }
}

onMounted(() => {
  loadDevices();
  loadSettings();
});
</script>

<style scoped>
.view-container {
  width: 100%;
  height: 100%;
  padding: 20px 24px;
  overflow-y: auto;
}

.view-header {
  margin-bottom: 18px;
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

.settings-content {
  display: flex;
  flex-direction: column;
  gap: 14px;
  max-width: 700px;
  padding-bottom: 24px;
}

.setting-card {
  padding: 16px 20px;
  background: var(--bg-card);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-lg);
}

.card-title {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 15px;
  font-weight: 600;
}

.card-desc {
  font-size: 12px;
  color: var(--text-secondary);
  margin: 6px 0 14px 0;
}

.setting-row {
  display: flex;
  gap: 10px;
  align-items: center;
}

.btn-wrap-row {
  flex-wrap: wrap;
}

.mt-row {
  margin-top: 12px;
}

.row-label {
  width: 68px;
  font-size: 13px;
  color: var(--text-secondary);
  flex-shrink: 0;
}

.row-select {
  flex: 1;
  height: 32px;
  font-size: 13px;
}

.dir-input {
  flex: 1;
  height: 32px;
  font-size: 12px;
  color: var(--text-secondary);
}

.checkbox-label {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
  cursor: pointer;
}

.shortcut-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 10px 14px;
}

.sc-field {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.sc-field label {
  font-size: 11px;
  color: var(--text-tertiary);
}

.conflict-tip {
  font-size: 12px;
  color: #d97706;
}
</style>
