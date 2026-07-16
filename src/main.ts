import './assets/main.css'
import { createApp } from 'vue'
import { createI18n } from 'vue-i18n'
import router from './router'
import { changeLocale } from './common'
// 引入VFluent3
import VueFluent from '@creatorsn/vfluent3'
// 必须引入全局样式
import '@creatorsn/vfluent3/style.css'
import App from "./App.vue"
import ColorSwatch from './components/ColorSwatch.vue'

export const SUPPORTED_LOCALES = ['en', 'zh-CN', 'zh-TW']
let locale = localStorage.getItem('locale')

const i18n = createI18n({
  locale: 'en',
  fallbackLocale: 'en',
  messages: {
    en: {
      // 新增导航菜单翻译
      render: 'Render',
      rpe: 'RPE',
      tasks: 'Tasks',
      'batch-render': 'Batch Render',
      setting: 'Settings',
      about: 'About',
      rules: {
        'non-empty': 'Must not be empty',
        positive: 'Must be a positive number',
        'positive-int': 'Must be a positive integer',
        resolution: "Must be like '1920x1080'",
        'sample-count': 'Must be a power of 2',
      },
      'has-error': 'There are errors in the configuration',
      'any-filter': 'All files',
    },
    'zh-CN': {
      // 中文导航翻译
      render: '渲染',
      rpe: 'RPE配置',
      tasks: '任务队列',
      'batch-render': '批量渲染',
      setting: '设置',
      about: '关于',
      rules: {
        'non-empty': '不能为空',
        positive: '必须是正数',
        'positive-int': '必须是正整数',
        resolution: "必须类似 '1920x1080'",
        'sample-count': '必须是 2 的幂',
      },
      'has-error': '配置中有错误',
      'any-filter': '所有文件',
    },
  },
  legacy: false,
  missing(_locale, key) {
    if (key.startsWith('title-')) return ''
    return key
  },
})

if (!locale) {
  locale = 'en'
  for (const alt of navigator.languages) {
    if (SUPPORTED_LOCALES.includes(alt)) {
      locale = alt
      break
    }
  }
}
changeLocale(locale)

const app = createApp(App)
app.component('ColorSwatch', ColorSwatch)
// 全局安装VFluent
app.use(VueFluent)
app.use(i18n)
app.use(router)
app.mount('#app')

export { i18n }

// 或通过监听 contextmenu 事件（JS 层面）
document.addEventListener('contextmenu', (e) => e.preventDefault());

// 禁用 F12 / Ctrl+Shift+I / Ctrl+Shift+J / Ctrl+U / Ctrl+R
document.addEventListener('keydown', (e) => {
  const ctrl = e.ctrlKey || e.metaKey;
  const shift = e.shiftKey;
  const key = e.key.toUpperCase();

  if (e.key === 'F12') e.preventDefault();
  if (ctrl && shift && (key === 'I' || key === 'J')) e.preventDefault();
  if (ctrl && (key === 'U' || key === 'R')) e.preventDefault();
});

