<template>
  <div v-if="appStore.showLoginModal" class="modal-backdrop" @click.self="closeModal">
    <div class="login-card">
      <div class="modal-header">
        <h3 class="modal-title">酷狗账号登录</h3>
        <button class="close-icon" @click="closeModal">
          <SvgIcon name="close" :size="16" />
        </button>
      </div>

      <!-- 已登录状态展示 -->
      <div v-if="appStore.authProfile.is_logged_in" class="logged-box">
        <img
          v-if="safeAvatar"
          :src="safeAvatar"
          alt="avatar"
          loading="lazy"
          decoding="async"
          class="user-avatar"
        />
        <div v-else class="user-avatar-placeholder">
          <SvgIcon name="user" :size="28" />
        </div>
        <div class="user-name">{{ appStore.authProfile.nickname }}</div>
        <div class="user-uid">UID: {{ appStore.authProfile.userid }}</div>
        <button class="fluent-btn danger-btn" @click="handleLogout">退出登录</button>
      </div>

      <!-- 未登录状态：四种方式切换 -->
      <div v-else>
        <div class="tab-bar">
          <button :class="['tab-btn', { active: loginMode === 'wx' }]" @click="switchMode('wx')">
            微信扫码
          </button>
          <button :class="['tab-btn', { active: loginMode === 'qr' }]" @click="switchMode('qr')">
            酷狗扫码
          </button>
          <button :class="['tab-btn', { active: loginMode === 'sms' }]" @click="switchMode('sms')">
            短信登录
          </button>
          <button :class="['tab-btn', { active: loginMode === 'pwd' }]" @click="switchMode('pwd')">
            密码登录
          </button>
        </div>

        <!-- 1. 微信扫码登录 -->
        <div v-if="loginMode === 'wx'" class="qr-section">
          <div class="qr-frame" @click="loadWxQrCode" title="点击刷新微信二维码">
            <img v-if="wxQrImgBase64" :src="wxQrImgBase64" alt="微信登录二维码" class="qr-img" />
            <div v-else class="qr-loading">{{ wxStatusTip }}</div>
          </div>
          <p class="qr-tip">{{ wxStatusTip }}</p>
          <button class="fluent-btn" @click="loadWxQrCode">刷新二维码</button>
        </div>

        <!-- 2. 酷狗二维码扫码登录 -->
        <div v-else-if="loginMode === 'qr'" class="qr-section">
          <div class="qr-frame" @click="loadQrCode" title="点击刷新二维码">
            <img v-if="qrImgBase64" :src="qrImgBase64" alt="登录二维码" class="qr-img" />
            <div v-else class="qr-loading">{{ qrStatusTip }}</div>
          </div>
          <p class="qr-tip">{{ qrStatusTip }}</p>
          <button class="fluent-btn" @click="loadQrCode">刷新二维码</button>
        </div>

        <!-- 2. 手机短信验证码登录 -->
        <div v-else-if="loginMode === 'sms'" class="form-section">
          <input
            v-model="mobile"
            class="fluent-input"
            placeholder="请输入 11 位手机号码"
            maxlength="11"
          />
          <div class="sms-row">
            <input
              v-model="smsCode"
              class="fluent-input"
              placeholder="短信验证码"
              maxlength="8"
            />
            <button
              class="fluent-btn sms-btn"
              :disabled="smsCooldown > 0 || sendingSms"
              @click="handleSendSms"
            >
              {{ smsCooldown > 0 ? `${smsCooldown}s` : '获取验证码' }}
            </button>
          </div>
          <p v-if="errorMsg" class="err-tip">{{ errorMsg }}</p>
          <button
            class="fluent-btn fluent-btn-primary submit-btn"
            :disabled="submitting"
            @click="handleSmsLogin"
          >
            {{ submitting ? '登录中...' : '立即登录' }}
          </button>
        </div>

        <!-- 3. 账号密码登录 -->
        <div v-else class="form-section">
          <input
            v-model="username"
            class="fluent-input"
            placeholder="手机号 / 酷狗账号 / 邮箱"
          />
          <input
            v-model="password"
            type="password"
            class="fluent-input"
            placeholder="登录密码"
            @keydown.enter="handlePwdLogin"
          />
          <p v-if="errorMsg" class="err-tip">{{ errorMsg }}</p>
          <button
            class="fluent-btn fluent-btn-primary submit-btn"
            :disabled="submitting"
            @click="handlePwdLogin"
          >
            {{ submitting ? '登录中...' : '登录' }}
          </button>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch, onUnmounted } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { useAppStore } from '../stores/app';
import { usePlayerStore } from '../stores/player';
import { formatSafeCover } from '../utils/trackMapper';
import SvgIcon from './SvgIcon.vue';

const appStore = useAppStore();
const playerStore = usePlayerStore();

const loginMode = ref<'wx' | 'qr' | 'sms' | 'pwd'>('wx');
const wxUuid = ref('');
const wxQrImgBase64 = ref('');
const wxStatusTip = ref('请使用手机微信扫一扫登录');
const qrKey = ref('');
const qrImgBase64 = ref('');
const qrStatusTip = ref('请使用酷狗音乐 App 扫码登录');
const mobile = ref('');
const smsCode = ref('');
const username = ref('');
const password = ref('');
const errorMsg = ref('');
const submitting = ref(false);
const sendingSms = ref(false);
const smsCooldown = ref(0);

let qrTimer: number | null = null;
let smsTimer: number | null = null;
let wxSessionVersion = 0;

const safeAvatar = computed(() => formatSafeCover(appStore.authProfile.pic, 100));

function stopTimers() {
  wxSessionVersion++;
  if (qrTimer) {
    clearInterval(qrTimer);
    qrTimer = null;
  }
  if (smsTimer) {
    clearInterval(smsTimer);
    smsTimer = null;
  }
}

function closeModal() {
  stopTimers();
  appStore.showLoginModal = false;
}

function switchMode(mode: 'wx' | 'qr' | 'sms' | 'pwd') {
  loginMode.value = mode;
  errorMsg.value = '';
  wxSessionVersion++;
  if (qrTimer) {
    clearInterval(qrTimer);
    qrTimer = null;
  }
  if (mode === 'wx') {
    loadWxQrCode();
  } else if (mode === 'qr') {
    loadQrCode();
  }
}

async function loadWxQrCode() {
  const curVersion = ++wxSessionVersion;
  wxUuid.value = '';
  wxQrImgBase64.value = '';
  wxStatusTip.value = '正在获取微信二维码...';
  try {
    const res = await invoke<any>('get_wx_qr');
    if (curVersion !== wxSessionVersion) return;
    if (res?.uuid && res?.qrcode_img) {
      wxUuid.value = res.uuid;
      wxQrImgBase64.value = res.qrcode_img;
      wxStatusTip.value = '请使用手机微信扫一扫登录';
      void pollWxQrStatus(curVersion);
    } else {
      wxStatusTip.value = '微信二维码生成失败，请点击刷新重试';
    }
  } catch {
    if (curVersion === wxSessionVersion) {
      wxStatusTip.value = '网络请求失败，请点击刷新重试';
    }
  }
}

async function pollWxQrStatus(sessionVersion: number) {
  while (
    sessionVersion === wxSessionVersion &&
    wxUuid.value &&
    appStore.showLoginModal &&
    loginMode.value === 'wx'
  ) {
    try {
      const res = await invoke<any>('check_wx_qr_status', { uuid: wxUuid.value });
      if (sessionVersion !== wxSessionVersion) break;
      const code = Number(res?.wx_errcode ?? res?.status ?? 0);
      if (code === 405) {
        if (res?.login?.status === 1) {
          stopTimers();
          await appStore.refreshAuthProfile();
          playerStore.showToast(`欢迎回来，${appStore.authProfile.nickname}！`);
          appStore.showLoginModal = false;
        } else {
          wxStatusTip.value = res?.login?.error || res?.login?.msg || '微信授权登录失败，请刷新重试';
        }
        break;
      } else if (code === 404) {
        wxStatusTip.value = '已扫码，请在手机微信上点击确认登录';
      } else if (code === 402 || code === 403) {
        wxStatusTip.value = '二维码已过期或已取消，请点击刷新';
        break;
      }
    } catch {
      if (sessionVersion !== wxSessionVersion) break;
    }
    await new Promise((r) => window.setTimeout(r, 1200));
  }
}

async function loadQrCode() {
  if (qrTimer) {
    clearInterval(qrTimer);
    qrTimer = null;
  }
  qrImgBase64.value = '';
  qrStatusTip.value = '正在获取二维码...';
  try {
    const res = await invoke<any>('get_qr_key');
    const data = res?.data;
    if (data?.qrcode) {
      qrKey.value = data.qrcode;
      qrImgBase64.value = data.qrcode_img || '';
      qrStatusTip.value = qrImgBase64.value
        ? '请使用酷狗音乐 / 酷狗概念版 App 扫码'
        : `二维码标识已就绪 (${String(data.qrcode).slice(0, 8)}...)，若未显示图片请切换短信登录`;
      startQrCheck();
    } else {
      qrStatusTip.value = '二维码生成失败，请点击重试或使用短信登录';
    }
  } catch (err) {
    qrStatusTip.value = '网络请求失败，请重试';
  }
}

function startQrCheck() {
  qrTimer = window.setInterval(async () => {
    if (!qrKey.value || !appStore.showLoginModal || loginMode.value !== 'qr') {
      return;
    }
    try {
      const res = await invoke<any>('check_qr_status', { qrcode: qrKey.value });
      const st = res?.data?.status;
      if (st === 2) {
        qrStatusTip.value = `已扫码 (${res?.data?.nickname || ''})，请在手机端确认登录`;
      } else if (st === 4) {
        stopTimers();
        await appStore.refreshAuthProfile();
        playerStore.showToast(`欢迎回来，${appStore.authProfile.nickname}！`);
        appStore.showLoginModal = false;
      } else if (st === 0) {
        if (qrTimer) clearInterval(qrTimer);
        qrTimer = null;
        qrStatusTip.value = '二维码已过期，请点击刷新';
      }
    } catch {
      // 忽略单次轮询网络抖动
    }
  }, 2000);
}

async function handleSendSms() {
  if (!/^1\d{10}$/.test(mobile.value.trim())) {
    errorMsg.value = '请输入正确的 11 位手机号码';
    return;
  }
  errorMsg.value = '';
  sendingSms.value = true;
  try {
    const res = await invoke<any>('send_sms_code', { mobile: mobile.value.trim() });
    if (res?.status === 1) {
      playerStore.showToast('短信验证码已发送');
      smsCooldown.value = 60;
      smsTimer = window.setInterval(() => {
        smsCooldown.value--;
        if (smsCooldown.value <= 0 && smsTimer) {
          clearInterval(smsTimer);
          smsTimer = null;
        }
      }, 1000);
    } else {
      errorMsg.value = res?.error || res?.data || '发送失败，请稍后重试或使用扫码登录';
    }
  } catch (err: any) {
    errorMsg.value = String(err);
  } finally {
    sendingSms.value = false;
  }
}

async function handleSmsLogin() {
  if (!mobile.value.trim() || !smsCode.value.trim()) {
    errorMsg.value = '请填写手机号与验证码';
    return;
  }
  errorMsg.value = '';
  submitting.value = true;
  try {
    const res = await invoke<any>('login_cellphone', {
      mobile: mobile.value.trim(),
      code: smsCode.value.trim(),
    });
    if (res?.status === 1) {
      await appStore.refreshAuthProfile();
      playerStore.showToast('手机验证码登录成功！');
      closeModal();
    } else {
      errorMsg.value = res?.error || res?.msg || '验证码错误或需要安全校验，请尝试扫码登录';
    }
  } catch (err: any) {
    errorMsg.value = String(err);
  } finally {
    submitting.value = false;
  }
}

async function handlePwdLogin() {
  if (!username.value.trim() || !password.value.trim()) {
    errorMsg.value = '请输入账号与密码';
    return;
  }
  errorMsg.value = '';
  submitting.value = true;
  try {
    const res = await invoke<any>('login_password', {
      username: username.value.trim(),
      password: password.value,
    });
    if (res?.status === 1) {
      await appStore.refreshAuthProfile();
      playerStore.showToast('登录成功！');
      closeModal();
    } else {
      errorMsg.value = res?.error || res?.msg || '账号密码错误或触发风控，推荐使用扫码登录';
    }
  } catch (err: any) {
    errorMsg.value = String(err);
  } finally {
    submitting.value = false;
  }
}

async function handleLogout() {
  await invoke('logout');
  await appStore.refreshAuthProfile();
  playerStore.showToast('已安全退出登录');
}

watch(
  () => appStore.showLoginModal,
  (shown) => {
    if (shown && !appStore.authProfile.is_logged_in) {
      if (loginMode.value === 'wx') {
        loadWxQrCode();
      } else if (loginMode.value === 'qr') {
        loadQrCode();
      }
    } else if (!shown) {
      stopTimers();
    }
  }
);

onUnmounted(() => {
  stopTimers();
});
</script>

<style scoped>
.modal-backdrop {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.48);
  backdrop-filter: blur(8px);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 300;
}

.login-card {
  width: 360px;
  padding: 20px 24px 24px;
  background: var(--bg-player);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-lg);
  box-shadow: 0 20px 48px rgba(0, 0, 0, 0.45);
}

.modal-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 16px;
}

.modal-title {
  font-size: 16px;
  font-weight: 700;
}

.close-icon {
  width: 28px;
  height: 28px;
  border-radius: var(--radius-full);
  color: var(--text-secondary);
}

.close-icon:hover {
  background: var(--bg-card-hover);
  color: var(--text-primary);
}

.logged-box {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 10px;
  padding: 16px 0 8px;
}

.user-avatar,
.user-avatar-placeholder {
  width: 68px;
  height: 68px;
  border-radius: var(--radius-full);
  object-fit: cover;
  background: var(--bg-card);
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 28px;
}

.user-name {
  font-size: 16px;
  font-weight: 600;
}

.user-uid {
  font-size: 12px;
  color: var(--text-tertiary);
  margin-bottom: 8px;
}

.danger-btn {
  color: #ff6b6b;
}

.tab-bar {
  display: flex;
  gap: 6px;
  margin-bottom: 16px;
  background: var(--bg-card);
  padding: 4px;
  border-radius: var(--radius-md);
}

.tab-btn {
  flex: 1;
  padding: 6px 0;
  font-size: 12px;
  border-radius: var(--radius-sm);
  color: var(--text-secondary);
}

.tab-btn.active {
  background: var(--accent-color);
  color: var(--accent-text);
  font-weight: 600;
}

.qr-section {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 12px;
}

.qr-frame {
  width: 176px;
  height: 176px;
  border-radius: var(--radius-md);
  background: #ffffff;
  padding: 10px;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
}

.qr-img {
  width: 100%;
  height: 100%;
  object-fit: contain;
}

.qr-loading {
  color: #333333;
  font-size: 12px;
  text-align: center;
  padding: 8px;
}

.qr-tip {
  font-size: 12px;
  color: var(--text-secondary);
  text-align: center;
}

.form-section {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.sms-row {
  display: flex;
  gap: 8px;
}

.sms-btn {
  flex-shrink: 0;
  white-space: nowrap;
}

.err-tip {
  font-size: 12px;
  color: #ff7b7b;
}

.submit-btn {
  width: 100%;
  margin-top: 4px;
  padding: 8px 0;
}
</style>
