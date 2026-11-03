import React from "react";
import ReactDOM from "react-dom/client";
import "./i18n";
import App from "./App";
import { isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { SettingsWindowBoundary } from './components/SettingsWindowBoundary';

// Protect the whole settings root, including App's theme/bootstrap effects.
// A boundary inside App's route cannot catch errors thrown by App itself.
const settingsWindow = isTauri()
  ? getCurrentWindow().label === 'settings'
  : window.location.hash === '#/settings';
const app = settingsWindow
  ? <SettingsWindowBoundary><App /></SettingsWindowBoundary>
  : <App />;

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {app}
  </React.StrictMode>,
);
