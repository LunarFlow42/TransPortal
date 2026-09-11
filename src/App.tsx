import React, { useEffect, useState } from 'react';
import { MorphingContainer } from './components/MorphingContainer';
import { useAppStore } from './store/useAppStore';
import { Search, Settings, Languages } from 'lucide-react';

export const App: React.FC = () => {
  const {
    mode,
    setMode,
    processText,
    provider,
    setProvider,
    openaiConfig,
    setOpenaiConfig,
  } = useAppStore();

  const [inputVal, setInputVal] = useState('portal');
  const [showSettings, setShowSettings] = useState(false);

  // Initialize with 'portal' on startup
  useEffect(() => {
    processText('portal');
  }, []);

  const handleSearch = (e?: React.FormEvent) => {
    if (e) e.preventDefault();
    if (inputVal.trim()) {
      processText(inputVal);
    }
  };

  const sampleQueries = [
    { text: 'portal', label: 'Single Word' },
    { text: 'cognitive load', label: 'Phrase' },
    {
      text: 'TransPortal acts as a cognitive gateway across different languages.',
      label: 'Full Sentence',
    },
    {
      text: 'The capsule smoothly morphs into an informative word card.',
      label: 'Sentence with Drill-down',
    },
  ];

  return (
    <div className="relative min-h-screen w-full flex flex-col items-center justify-start p-4 select-none bg-transparent">
      {/* Simulation & Test Controls Bar (Collapsible / Sleek) */}
      <div className="w-full max-w-[460px] mb-3 flex flex-col gap-2 z-20">
        {/* Search input to test selection capture */}
        <form
          onSubmit={handleSearch}
          className="flex items-center gap-2 px-3 py-1.5 rounded-xl bg-[#1a1a1e]/90 border border-white/10 shadow-lg backdrop-blur-md"
        >
          <Search className="w-4 h-4 text-slate-400 shrink-0" />
          <input
            type="text"
            value={inputVal}
            onChange={(e) => setInputVal(e.target.value)}
            placeholder="Simulate word or sentence selection..."
            className="w-full bg-transparent text-xs text-white placeholder-slate-500 focus:outline-none"
          />
          <button
            type="submit"
            title="Translate & Query"
            className="px-2 py-0.5 rounded-md bg-indigo-600 hover:bg-indigo-500 text-[11px] font-medium text-white transition-colors"
          >
            Query
          </button>
          <button
            type="button"
            onClick={() => setShowSettings(!showSettings)}
            title="Settings"
            className={`p-1 rounded hover:bg-white/10 transition-colors ${
              showSettings ? 'text-cyan-400' : 'text-slate-400 hover:text-white'
            }`}
          >
            <Settings className="w-3.5 h-3.5" />
          </button>
        </form>

        {/* Quick presets */}
        <div className="flex items-center gap-1.5 overflow-x-auto py-0.5 no-scrollbar">
          {sampleQueries.map((q, idx) => (
            <button
              key={idx}
              onClick={() => {
                setInputVal(q.text);
                processText(q.text);
              }}
              className="shrink-0 px-2 py-0.5 rounded-full text-[10px] font-medium bg-white/5 hover:bg-white/15 text-slate-300 border border-white/5 transition-all active:scale-95"
            >
              {q.label}
            </button>
          ))}
        </div>

        {/* Settings Panel (Modal or Drawer) */}
        {showSettings && (
          <div className="p-3 rounded-xl bg-[#1c1c22]/95 border border-white/10 shadow-2xl backdrop-blur-xl flex flex-col gap-2.5 text-xs text-slate-200">
            <div className="flex items-center justify-between pb-1.5 border-b border-white/10">
              <span className="font-semibold text-white flex items-center gap-1.5">
                <Languages className="w-3.5 h-3.5 text-indigo-400" />
                Provider Configuration
              </span>
              <span className="text-[10px] text-slate-400">Microkernel Plugin</span>
            </div>

            {/* Provider toggle */}
            <div className="flex items-center gap-2">
              <label className="text-slate-400 w-20">Provider:</label>
              <select
                value={provider}
                onChange={(e) => setProvider(e.target.value as any)}
                className="bg-[#2a2a32] text-white border border-white/15 rounded px-2 py-1 flex-1 focus:outline-none"
              >
                <option value="google_free">Google Free (Zero Config Web API)</option>
                <option value="openai">OpenAI Compatible (BYOK SSE Stream)</option>
              </select>
            </div>

            {provider === 'openai' && (
              <div className="flex flex-col gap-2 pt-1 border-t border-white/5">
                <div className="flex items-center gap-2">
                  <label className="text-slate-400 w-20">Base URL:</label>
                  <input
                    type="text"
                    value={openaiConfig.base_url || ''}
                    onChange={(e) => setOpenaiConfig({ base_url: e.target.value })}
                    placeholder="https://api.openai.com/v1"
                    className="bg-[#2a2a32] text-white border border-white/15 rounded px-2 py-1 flex-1 focus:outline-none"
                  />
                </div>
                <div className="flex items-center gap-2">
                  <label className="text-slate-400 w-20">API Key:</label>
                  <input
                    type="password"
                    value={openaiConfig.api_key}
                    onChange={(e) => setOpenaiConfig({ api_key: e.target.value })}
                    placeholder="sk-..."
                    className="bg-[#2a2a32] text-white border border-white/15 rounded px-2 py-1 flex-1 focus:outline-none"
                  />
                </div>
                <div className="flex items-center gap-2">
                  <label className="text-slate-400 w-20">Model:</label>
                  <input
                    type="text"
                    value={openaiConfig.model || ''}
                    onChange={(e) => setOpenaiConfig({ model: e.target.value })}
                    placeholder="gpt-4o-mini"
                    className="bg-[#2a2a32] text-white border border-white/15 rounded px-2 py-1 flex-1 focus:outline-none"
                  />
                </div>
              </div>
            )}

            {/* Mode override toggle */}
            <div className="flex items-center justify-between pt-1 border-t border-white/5">
              <span className="text-slate-400">Current Morph State:</span>
              <div className="flex items-center gap-1">
                <button
                  onClick={() => setMode('bubble')}
                  className={`px-2 py-0.5 rounded text-[10px] font-medium transition-colors ${
                    mode === 'bubble'
                      ? 'bg-cyan-500/20 text-cyan-300 border border-cyan-500/40'
                      : 'text-slate-400 hover:text-white'
                  }`}
                >
                  Bubble
                </button>
                <button
                  onClick={() => setMode('card')}
                  className={`px-2 py-0.5 rounded text-[10px] font-medium transition-colors ${
                    mode === 'card'
                      ? 'bg-indigo-500/20 text-indigo-300 border border-indigo-500/40'
                      : 'text-slate-400 hover:text-white'
                  }`}
                >
                  Card
                </button>
              </div>
            </div>
          </div>
        )}
      </div>

      {/* The Core Morphing Container (Ghost Bubble <-> Deep Word Card) */}
      <MorphingContainer />
    </div>
  );
};

export default App;
