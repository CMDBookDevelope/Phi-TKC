import { createI18n } from 'vue-i18n';

// `zh-TW` is accepted by the navigator-based detection in main.ts, but the app only
// ships `en` and `zh-CN` message sets; changeLocale() maps it to `zh-CN`.
export const SUPPORTED_LOCALES = ['en', 'zh-CN', 'zh-TW'];

export const i18n = createI18n({
  locale: 'en',
  fallbackLocale: 'en',
  messages: {
    en: {
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
        number: 'Must be a number',
        resolution: "Must be like '1920x1080'",
        'sample-count': 'Must be a power of 2',
      },
      'has-error': 'There are errors in the configuration',
      'any-filter': 'All files',
      // Router titles are resolved through `i18n.global.t`, so they must live in the
      // global message set — SFC <i18n> blocks are locally scoped and invisible here.
      // `title-default` is intentionally absent: `missing()` maps it to '' → plain "Phi-TKC".
      'title-render': 'Render',
      'title-rpe': 'RPE',
      'title-tasks': 'Tasks',
      'title-batch-render': 'Batch Render',
      'title-setting': 'Settings',
      'title-about': 'About',
    },
    'zh-CN': {
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
        number: '必须是数字',
        resolution: "必须类似 '1920x1080'",
        'sample-count': '必须是 2 的幂',
      },
      'has-error': '配置中有错误',
      'any-filter': '所有文件',
      'title-render': '渲染',
      'title-rpe': 'RPE',
      'title-tasks': '任务',
      'title-batch-render': '批量渲染',
      'title-setting': '设置',
      'title-about': '关于',
    },
  },
  legacy: false,
  missing(_locale, key) {
    if (key.startsWith('title-')) return '';
    return key;
  },
});
