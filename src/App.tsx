import React from 'react';
import { HashRouter, Navigate, Route, Routes } from 'react-router-dom';
import { ShellView } from './views/ShellView';
import { SettingsView } from './views/SettingsView';
import { RegionSelectView } from './views/RegionSelectView';
import { useTheme } from './hooks/useTheme';
import './App.css';

/**
 * Hash routing addresses each Tauri window:
 * `index.html#/shell` (overlay) and `index.html#/settings` (settings window).
 */
export const App: React.FC = () => {
  useTheme();

  return (
    <HashRouter>
      <Routes>
        <Route path="/shell" element={<ShellView />} />
        <Route path="/settings" element={<SettingsView />} />
        <Route path="/ocr-region" element={<RegionSelectView />} />
        <Route path="*" element={<Navigate to="/shell" replace />} />
      </Routes>
    </HashRouter>
  );
};

export default App;
