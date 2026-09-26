import { useEffect } from 'react';
import { useAppStore } from '../store/useAppStore';
import { setWindowTheme } from '../utils/windows';

const QUERY = '(prefers-color-scheme: dark)';

/**
 * Resolve the effective theme (following the OS when set to `system`) and
 * reflect it on the document root as `data-theme`, `color-scheme`, and a
 * `dark` class so Tailwind + CSS variables react consistently.
 */
export function useTheme() {
  const theme = useAppStore((state) => state.theme);
  const overlayOpacity = useAppStore((state) => state.overlayOpacity);
  const backdropBlur = useAppStore((state) => state.backdropBlur);

  useEffect(() => {
    const media = window.matchMedia(QUERY);

    const apply = () => {
      const dark = theme === 'dark' || (theme === 'system' && media.matches);
      const root = document.documentElement;
      root.dataset.theme = dark ? 'dark' : 'light';
      root.style.colorScheme = dark ? 'dark' : 'light';
      root.classList.toggle('dark', dark);
      void setWindowTheme(dark);
    };

    apply();
    media.addEventListener('change', apply);
    return () => media.removeEventListener('change', apply);
  }, [theme]);

  // Overlay glass alpha (settings window uses an opaque panel and ignores it).
  useEffect(() => {
    document.documentElement.style.setProperty(
      '--surface-alpha',
      overlayOpacity.toFixed(3),
    );
  }, [overlayOpacity]);

  // Sync backdrop blur preference to the root element for CSS .surface
  useEffect(() => {
    const root = document.documentElement;
    root.dataset.backdropBlur = backdropBlur ? 'true' : 'false';
  }, [backdropBlur]);
}
