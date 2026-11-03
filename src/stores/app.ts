import { defineStore } from 'pinia';
import { invoke } from '@tauri-apps/api/core';
import type { Track } from './player';

export type NavTab =
  | 'discover'
  | 'daily'
  | 'rankings'
  | 'library'
  | 'search'
  | 'history'
  | 'settings';

export type ThemePreference = 'light' | 'dark' | 'system';

export interface AuthProfile {
  is_logged_in: boolean;
  userid: string;
  nickname: string;
  pic: string;
}

export interface UserPlaylistItem {
  listid: number;
  name: string;
  pic: string;
  count: number;
  listCreateUserid?: number;
  listCreateGid?: string;
  isDefaultLike?: boolean;
}

export interface ActivePlaylistMeta {
  id: string;
  name: string;
  cover?: string;
  intro?: string;
  kind?: 'playlist' | 'artist' | 'album';
  isUserList?: boolean;
  listid?: number;
  createUserId?: number;
  totalCount?: number;
}

let mediaQueryListenerBound = false;

export const useAppStore = defineStore('app', {
  state: () => ({
    currentTab: 'discover' as NavTab,
    previousTab: 'discover' as NavTab,
    theme: 'light' as ThemePreference,
    resolvedTheme: 'light' as 'light' | 'dark',
    searchKeyword: '',
    showLoginModal: false,
    vipActive: false,
    authProfile: {
      is_logged_in: false,
      userid: '',
      nickname: '未登录',
      pic: '',
    } as AuthProfile,
    userPlaylists: [] as UserPlaylistItem[],
    activePlaylist: null as ActivePlaylistMeta | null,
  }),

  actions: {
    computeResolvedTheme(pref: ThemePreference): 'light' | 'dark' {
      if (pref === 'dark') return 'dark';
      if (pref === 'system' && typeof window !== 'undefined' && window.matchMedia) {
        return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
      }
      return 'light';
    },

    async applyThemePreference(pref: ThemePreference, persist = true) {
      this.theme = pref;
      const resolved = this.computeResolvedTheme(pref);
      this.resolvedTheme = resolved;
      if (typeof document !== 'undefined') {
        document.documentElement.setAttribute('data-theme', resolved);
      }
      if (persist) {
        try {
          await invoke('set_theme', {
            theme: pref,
            resolvedDark: resolved === 'dark',
          });
        } catch (err) {
          console.warn('同步主题失败:', err);
        }
      }
    },

    async initApp() {
      try {
        const settings = await invoke<{ theme: string }>('get_app_settings');
        const pref: ThemePreference =
          settings.theme === 'dark' || settings.theme === 'system' ? settings.theme : 'light';
        await this.applyThemePreference(pref, true);
      } catch (err) {
        await this.applyThemePreference('light', false);
        console.warn('初始化应用设置失败:', err);
      }

      if (!mediaQueryListenerBound && typeof window !== 'undefined' && window.matchMedia) {
        mediaQueryListenerBound = true;
        const mql = window.matchMedia('(prefers-color-scheme: dark)');
        mql.addEventListener('change', () => {
          if (this.theme === 'system') {
            this.applyThemePreference('system', true);
          }
        });
      }

      await this.refreshAuthProfile();
    },

    async refreshAuthProfile() {
      try {
        this.authProfile = await invoke<AuthProfile>('get_auth_profile');
        if (this.authProfile.is_logged_in) {
          this.loadUserPlaylists();
          this.checkVipStatus();
        } else {
          this.userPlaylists = [];
          this.vipActive = false;
        }
      } catch (err) {
        console.warn('读取账号信息失败:', err);
      }
    },

    async checkVipStatus() {
      if (!this.authProfile.is_logged_in) {
        this.vipActive = false;
        return;
      }
      try {
        const res = await invoke<any>('get_vip_detail');
        const data = res?.data;
        const isVip =
          Number(data?.is_vip ?? data?.vip_type ?? 0) > 0 ||
          (Array.isArray(data?.busi_vip) &&
            data.busi_vip.some((b: any) => Number(b?.is_vip) === 1));
        this.vipActive = isVip;
      } catch {
        this.vipActive = false;
      }
    },

    async loadUserPlaylists() {
      if (!this.authProfile.is_logged_in) {
        this.userPlaylists = [];
        return;
      }
      try {
        const res = await invoke<any>('get_user_playlists', { page: 1, pagesize: 50 });
        const rawArr: any[] = res?.data?.info || res?.data?.lists || [];
        this.userPlaylists = rawArr
          .filter((item) => item && item.listid)
          .map((item) => ({
            listid: Number(item.listid),
            name: String(item.name || '我的歌单'),
            pic: String(item.pic || item.cover || ''),
            count: Number(item.count || 0),
            listCreateUserid: item.list_create_userid ? Number(item.list_create_userid) : undefined,
            listCreateGid: item.list_create_gid ? String(item.list_create_gid) : undefined,
            isDefaultLike:
              String(item.name || '').includes('我喜欢') || Number(item.sort) === 1,
          }));
      } catch (err) {
        console.warn('加载用户云歌单列表失败:', err);
      }
    },

    async favoriteTrackToPlaylist(listid: number, track: Track): Promise<string> {
      if (!this.authProfile.is_logged_in) {
        this.showLoginModal = true;
        throw new Error('请先登录账号后再收藏歌曲');
      }
      await invoke('favorite_song', { listid, track });
      await this.loadUserPlaylists();
      const targetName = this.userPlaylists.find((p) => p.listid === listid)?.name || '歌单';
      return `已收藏《${track.name}》至「${targetName}」`;
    },

    async favoriteTrackToDefault(track: Track): Promise<string> {
      if (!this.authProfile.is_logged_in) {
        this.showLoginModal = true;
        throw new Error('请先登录账号后再收藏歌曲');
      }
      if (this.userPlaylists.length === 0) {
        await this.loadUserPlaylists();
      }
      const target =
        this.userPlaylists.find((p) => p.isDefaultLike) || this.userPlaylists[0];
      if (!target) {
        throw new Error('暂无可用云歌单，请先在歌单页新建一个歌单');
      }
      return this.favoriteTrackToPlaylist(target.listid, track);
    },

    setTab(tab: NavTab) {
      this.activePlaylist = null;
      if (tab !== 'search') {
        this.previousTab = tab;
      } else if (this.currentTab !== 'search') {
        this.previousTab = this.currentTab;
      }
      this.currentTab = tab;
    },

    openPlaylistDetail(meta: ActivePlaylistMeta) {
      this.activePlaylist = {
        kind: 'playlist',
        ...meta,
      };
    },

    async toggleTheme() {
      const next: ThemePreference = this.resolvedTheme === 'dark' ? 'light' : 'dark';
      await this.applyThemePreference(next, true);
    },

    triggerSearch(keyword: string) {
      this.searchKeyword = keyword.trim();
      if (this.searchKeyword) {
        this.activePlaylist = null;
        if (this.currentTab !== 'search') {
          this.previousTab = this.currentTab;
        }
        this.currentTab = 'search';
      }
    },

    exitSearchToPrevious() {
      this.searchKeyword = '';
      if (this.currentTab === 'search') {
        this.currentTab = this.previousTab !== 'search' ? this.previousTab : 'discover';
      }
    },
  },
});
