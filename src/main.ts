import { createApp } from 'vue';
import { createPinia } from 'pinia';
import App from './App.vue';
import './styles/fluent-win11.css';

// 禁用 WebView2 浏览器默认右键菜单（返回、刷新、另存为、打印、更多工具），保留歌曲行自定义右键菜单
window.addEventListener('contextmenu', (e) => {
  e.preventDefault();
});

const app = createApp(App);
app.use(createPinia());
app.mount('#app');
