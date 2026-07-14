import { i18n } from './main'
export type ToastIntent = 'success' | 'info' | 'warning'
export const SUPPORTED_LOCALES = ['en', 'zh-CN', 'zh-TW']
export function anyFilter() {
  return {
    name: i18n.global.t('any-filter'),
    extensions: ['*'],
  }
}
export function isString(s: unknown): s is string {
  return typeof s === 'string'
}
export function isNumeric(num: any) {
  return (typeof num === 'number' || (typeof num === 'string' && num.trim() !== '')) && !isNaN(num as number)
}
// 表单校验规则
export const RULES = {
  non_empty: (v: string) => v.trim().length > 0 || i18n.global.t('rules.non-empty'),
  positive: (v: string) => (isNumeric(v) && Number(v) >= 0) || i18n.global.t('rules.positive'),
  positiveInt: (v: string) => {
    if (!isNumeric(v)) return i18n.global.t('rules.positive-int')
    const n = Number(v)
    return Math.abs(n - Math.round(n)) < 1e-4 && n > 0 || i18n.global.t('rules.positive-int')
  },
}
export function setTitle(title: string) {
  document.title = title ? `${title} - Phi-TKC` : 'Phi-TKC'
}
// 语言切换
export function changeLocale(locale: string) {
  if (locale.startsWith('en')) locale = 'en'
  if (!SUPPORTED_LOCALES.includes(locale)) locale = 'en'
  if (locale === 'zh-TW') i18n.global.locale.value = 'zh-CN'
  else i18n.global.locale.value = locale
  localStorage.setItem('locale', locale)
}
// 全局提示调用App挂载的$toast
export function toast(message: string, intent: ToastIntent = 'info') {
  (window as any).$toast(message, intent)
}
export function toastError(error: any) {
  console.error(error)
  const msg = error instanceof Error ? error.message : String(error)
  if (msg) toast(msg, 'warning')
}

