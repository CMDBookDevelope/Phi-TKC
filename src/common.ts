import { toast as sonnerToast } from 'vuetify-sonner';

import { SUPPORTED_LOCALES, i18n } from './i18n';

import moment from 'moment';

import 'moment/dist/locale/zh-cn';
import 'moment/dist/locale/zh-hk';

export function anyFilter() {
  return {
    name: i18n.global.t('any-filter'),
    extensions: ['*'],
  };
}

export function isString(s: unknown): s is string {
  return typeof s === 'string';
}

export const RULES = {
  // Vuetify's `clearable` sets the model to null, so accept nullish values here.
  non_empty: (value: string | null | undefined) => (value ?? '').trim().length > 0 || i18n.global.t('rules.non-empty'),
  positive: (value: string | number | null | undefined) => (isNumeric(value) && Number(value) > 0) || i18n.global.t('rules.positive'),
  positiveInt: (value: string | number | null | undefined) =>
    (isNumeric(value) && Math.abs(Number(value) - Math.round(Number(value))) < 1e-4 && Number(value) > 0) || i18n.global.t('rules.positive-int'),
  number: (value: string | number | null | undefined) => isNumeric(value) || i18n.global.t('rules.number'),
  resolution: (value: string | null | undefined) => {
    const match = (value ?? '').trim().match(/^(\d+)\s*[xX]\s*(\d+)$/);
    const width = Number(match?.[1]);
    const height = Number(match?.[2]);
    return (!!match && width >= 16 && height >= 16 && width % 2 === 0 && height % 2 === 0) || i18n.global.t('rules.resolution');
  },
  sampleCount: (value: string | number | null | undefined) => {
    const n = Number(value);
    return (Number.isInteger(n) && n > 0 && (n & (n - 1)) === 0) || i18n.global.t('rules.sample-count');
  },
};

export function isNumeric(num: unknown) {
  return (typeof num === 'number' || (typeof num === 'string' && num.trim() !== '')) && Number.isFinite(Number(num));
}

export function setTitle(title: string) {
  document.title = title.length ? title + ' - Phi-TKC' : 'Phi-TKC';
}

export function changeLocale(locale: string) {
  if (locale.startsWith('en')) locale = 'en';
  if (!SUPPORTED_LOCALES.includes(locale)) locale = 'en';
  i18n.global.locale.value = (locale === 'zh-TW' ? 'zh-CN' : locale) as typeof i18n.global.locale.value;
  localStorage.setItem('locale', locale);
  const momentLocale =
    {
      'zh-CN': 'zh-cn',
      'zh-TW': 'zh-hk',
      en: 'en-us',
    }[locale] ?? 'en-us';
  moment.locale(momentLocale);
}

export function toast(message: string, kind?: 'success' | 'info' | 'warning' | 'error') {
  sonnerToast(message, {
    duration: 2000,
    cardProps: {
      color: kind,
    },
  });
}

export function toastError(error: any) {
  console.error(error);
  const msg = error instanceof Error ? error.message : String(error);
  if (msg.length) toast(msg, 'error');
}
