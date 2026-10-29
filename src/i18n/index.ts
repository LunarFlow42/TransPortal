import i18n from 'i18next';
import { initReactI18next } from 'react-i18next';
import zhCN from './locales/zh-CN.json';
import enUS from './locales/en-US.json';

const STORAGE_KEY = 'transportal_ui_language';

export const getInitialLanguage = (): 'zh-CN' | 'en-US' => {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (saved === 'zh-CN' || saved === 'en-US') return saved;
    const nav = typeof navigator !== 'undefined' ? navigator.language.toLowerCase() : '';
    if (nav.startsWith('zh')) return 'zh-CN';
    return 'en-US';
  } catch {
    return 'zh-CN';
  }
};

export const setAppLanguage = (lang: 'zh-CN' | 'en-US') => {
  try {
    localStorage.setItem(STORAGE_KEY, lang);
    void i18n.changeLanguage(lang);
  } catch (err) {
    console.error('Failed to persist app language:', err);
  }
};

export const resources = {
  'zh-CN': { translation: zhCN },
  'en-US': { translation: enUS },
} as const;

void i18n.use(initReactI18next).init({
  resources,
  lng: getInitialLanguage(),
  fallbackLng: 'zh-CN',
  interpolation: {
    escapeValue: false,
  },
});

export default i18n;
