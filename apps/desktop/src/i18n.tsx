import React, { createContext, useCallback, useContext, useEffect, useMemo, useState } from 'react';

export type UiLanguage = 'en' | 'ru';
export type UiLanguagePreference = 'system' | UiLanguage;

const STORAGE_KEY = 'vontaqfs.ui.language';

type I18nContextValue = {
  language: UiLanguage;
  preference: UiLanguagePreference;
  setPreference: (value: UiLanguagePreference) => void;
  tr: (english: string, russian: string) => string;
};

const I18nContext = createContext<I18nContextValue | null>(null);

function systemLanguage(): UiLanguage {
  const language = typeof navigator === 'undefined' ? 'en' : navigator.language.toLowerCase();
  return language.startsWith('ru') ? 'ru' : 'en';
}

function readPreference(): UiLanguagePreference {
  if (typeof window === 'undefined') return 'system';
  const value = window.localStorage.getItem(STORAGE_KEY);
  return value === 'en' || value === 'ru' || value === 'system' ? value : 'system';
}

export function I18nProvider({ children }: { children: React.ReactNode }) {
  const [preference, setPreferenceState] = useState<UiLanguagePreference>(() => readPreference());
  const [detectedLanguage, setDetectedLanguage] = useState<UiLanguage>(() => systemLanguage());
  const language = preference === 'system' ? detectedLanguage : preference;

  const setPreference = useCallback((value: UiLanguagePreference) => {
    window.localStorage.setItem(STORAGE_KEY, value);
    setPreferenceState(value);
  }, []);

  useEffect(() => {
    document.documentElement.lang = language;
    document.title = language === 'ru' ? 'VontaqFS — локальное хранилище' : 'VontaqFS — Local storage';
  }, [language]);

  useEffect(() => {
    const onLanguageChange = () => setDetectedLanguage(systemLanguage());
    const onStorage = (event: StorageEvent) => {
      if (event.key === STORAGE_KEY) setPreferenceState(readPreference());
    };
    window.addEventListener('languagechange', onLanguageChange);
    window.addEventListener('storage', onStorage);
    return () => {
      window.removeEventListener('languagechange', onLanguageChange);
      window.removeEventListener('storage', onStorage);
    };
  }, []);

  const tr = useCallback((english: string, russian: string) => language === 'ru' ? russian : english, [language]);
  const value = useMemo(() => ({ language, preference, setPreference, tr }), [language, preference, setPreference, tr]);

  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useI18n(): I18nContextValue {
  const value = useContext(I18nContext);
  if (!value) throw new Error('useI18n must be used inside I18nProvider');
  return value;
}

const RUSSIAN_TOKEN_LABELS: Record<string, string> = {
  ready: 'готово',
  running: 'работает',
  healthy: 'исправно',
  completed: 'завершено',
  'up-to-date': 'актуально',
  starting: 'запуск',
  suspended: 'приостановлено',
  draining: 'завершение работы',
  checking: 'проверка',
  available: 'доступно',
  downloading: 'загрузка',
  downloaded: 'загружено',
  error: 'ошибка',
  failed: 'ошибка',
  'destructive-recovery-required': 'требуется восстановление',
  stopped: 'остановлено',
  neutral: 'не подключено',
  queued: 'в очереди',
  cancelling: 'отмена',
  cancelled: 'отменено',
  'not-checked': 'не проверялось',
  'not-configured': 'не настроено',
  persistent: 'постоянное',
  cache: 'кэш',
  temporary: 'временное',
  'user-data': 'данные пользователя',
  generated: 'сгенерированные данные',
  index: 'индекс',
  backup: 'резервная копия',
  snapshot: 'снимок',
  custom: 'пользовательская категория',
  read: 'чтение',
  write: 'запись',
  'read-write': 'чтение и запись',
  replace: 'заменять',
  skip: 'пропускать',
  rename: 'переименовывать',
  ask: 'спрашивать',
  'update-changed': 'обновлять изменённое',
  file: 'файл',
  files: 'файлы',
  directory: 'папка',
  archive: 'архив',
  'figma-plugin': 'Figma-плагин',
  'figma-widget': 'Figma-виджет',
  'other-supported-client': 'поддерживаемый клиент',
};

export function localizedToken(value: string, language: UiLanguage): string {
  if (language === 'ru' && RUSSIAN_TOKEN_LABELS[value]) return RUSSIAN_TOKEN_LABELS[value];
  return value.replaceAll('-', ' ');
}
