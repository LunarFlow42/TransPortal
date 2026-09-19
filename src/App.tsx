import React from 'react';
import { HashRouter, Navigate, Route, Routes } from 'react-router-dom';
import { ShellView } from './views/ShellView';
import { useTheme } from './hooks/useTheme';
import './App.css';

const SettingsView = React.lazy(() =>
  import('./views/SettingsView').then((m) => ({ default: m.SettingsView })),
);
const RegionSelectView = React.lazy(() =>
  import('./views/RegionSelectView').then((m) => ({ default: m.RegionSelectView })),
);

/**
 * Hash routing addresses each Tauri window:
 * `index.html#/shell` (overlay) and `index.html#/settings` (settings window).
 */
export const App: React.FC = () => {
  useTheme();

  React.useEffect(() => {
    const handleContextMenu = (event: MouseEvent) => {
      const target = event.target as HTMLElement | null;
      const isInput =
        target?.tagName === 'INPUT' ||
        target?.tagName === 'TEXTAREA' ||
        target?.isContentEditable;
      if (!isInput) {
        event.preventDefault();
      }
    };
    window.addEventListener('contextmenu', handleContextMenu);
    return () => window.removeEventListener('contextmenu', handleContextMenu);
  }, []);

  return (
    <HashRouter>
      <React.Suspense fallback={null}>
        <Routes>
          <Route path="/shell" element={<ShellView />} />
          <Route path="/settings" element={<SettingsView />} />
          <Route path="/ocr-region" element={<RegionSelectView />} />
          <Route path="*" element={<Navigate to="/shell" replace />} />
        </Routes>
      </React.Suspense>
    </HashRouter>
  );
};

export default App;
