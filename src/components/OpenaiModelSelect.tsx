import React, { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { AlertCircle, Check, ChevronDown, Loader2, RotateCw, Search } from 'lucide-react';
import { Button, Input } from './ui';
import { useAppStore } from '../store/useAppStore';
import { OpenAiConfig } from '../types';
import { cn } from '../utils/cn';

interface OpenaiModelSelectProps {
  value: string;
  onChange: (model: string) => void;
  config: OpenAiConfig;
  disabled?: boolean;
}

export const OpenaiModelSelect: React.FC<OpenaiModelSelectProps> = ({
  value,
  onChange,
  config,
  disabled = false,
}) => {
  const { openaiModels, setOpenaiModels } = useAppStore();
  const [fetching, setFetching] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [successMsg, setSuccessMsg] = useState<string | null>(null);
  const [isOpen, setIsOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState('');
  const containerRef = useRef<HTMLDivElement>(null);
  const searchInputRef = useRef<HTMLInputElement>(null);

  // Close dropdown on outside click
  useEffect(() => {
    if (!isOpen) return;
    const handlePointerDown = (event: PointerEvent) => {
      if (!containerRef.current?.contains(event.target as Node)) {
        setIsOpen(false);
      }
    };
    document.addEventListener('pointerdown', handlePointerDown);
    return () => {
      document.removeEventListener('pointerdown', handlePointerDown);
    };
  }, [isOpen]);

  // Focus search input when dropdown opens
  useEffect(() => {
    if (isOpen && openaiModels.length > 5) {
      setTimeout(() => searchInputRef.current?.focus(), 50);
    }
  }, [isOpen, openaiModels.length]);

  const handleFetchModels = async () => {
    if (fetching || disabled) return;
    setFetching(true);
    setError(null);
    setSuccessMsg(null);

    try {
      const models = await invoke<string[]>('fetch_openai_models', {
        config: {
          apiKey: config.api_key,
          baseUrl: config.base_url,
          model: config.model,
          temperature: config.temperature,
          systemPrompt: config.system_prompt,
          promptTemplate: config.prompt_template,
        },
      });

      setOpenaiModels(models);
      setSuccessMsg(`已获取 ${models.length} 个可用模型`);
      if (models.length > 0) {
        setIsOpen(true);
        if (!value.trim()) {
          onChange(models[0]);
        }
      }
    } catch (err) {
      setError(String(err));
    } finally {
      setFetching(false);
    }
  };

  const filteredModels = openaiModels.filter((m) =>
    m.toLowerCase().includes(searchQuery.trim().toLowerCase()),
  );

  return (
    <div className="flex flex-col gap-1.5" ref={containerRef}>
      <div className="flex items-center gap-3">
        <label className="w-24 shrink-0 text-[13px] text-[var(--muted)]">Model</label>

        {/* Input with dropdown toggle */}
        <div className="relative flex-1 min-w-0">
          <Input
            type="text"
            value={value}
            onChange={(e) => {
              onChange(e.target.value);
              setError(null);
            }}
            placeholder="gpt-4o-mini"
            disabled={disabled}
            rightElement={
              openaiModels.length > 0 ? (
                <button
                  type="button"
                  tabIndex={-1}
                  onClick={() => setIsOpen(!isOpen)}
                  title="展开模型列表"
                  className="flex h-5 w-5 items-center justify-center rounded text-[var(--muted)] hover:text-[var(--text)] transition-colors"
                >
                  <ChevronDown
                    className={cn(
                      'h-3.5 w-3.5 transition-transform duration-150',
                      isOpen && 'rotate-180 text-[var(--accent-b)]',
                    )}
                  />
                </button>
              ) : undefined
            }
          />

          {/* Floating Dropdown */}
          {isOpen && openaiModels.length > 0 && (
            <div className="absolute left-0 right-0 top-full z-50 mt-1 max-h-64 rounded-[var(--r-ctrl)] border border-[var(--stroke)] bg-[var(--surface-1)] p-1.5 shadow-2xl backdrop-blur-md flex flex-col gap-1">
              {/* Quick filter search box */}
              {openaiModels.length > 5 && (
                <div className="relative flex items-center px-1 pb-1 border-b border-[var(--stroke)]">
                  <Search className="h-3 w-3 text-[var(--faint)] absolute left-2.5 pointer-events-none" />
                  <input
                    ref={searchInputRef}
                    type="text"
                    value={searchQuery}
                    onChange={(e) => setSearchQuery(e.target.value)}
                    placeholder="搜索模型名称..."
                    className="w-full bg-[var(--surface-2)] text-[12px] text-[var(--text)] pl-6 pr-2 py-1 rounded-[var(--r-ctrl)] border border-[var(--stroke)] outline-none focus:border-[var(--accent-b)] placeholder:text-[var(--faint)]"
                  />
                </div>
              )}

              {/* Scrollable list */}
              <div className="overflow-y-auto max-h-48 flex flex-col gap-0.5">
                {filteredModels.length === 0 ? (
                  <div className="py-3 text-center text-[11px] text-[var(--faint)]">
                    无匹配模型
                  </div>
                ) : (
                  filteredModels.map((model) => {
                    const isSelected = model === value;
                    return (
                      <button
                        key={model}
                        type="button"
                        onClick={() => {
                          onChange(model);
                          setIsOpen(false);
                          setSearchQuery('');
                        }}
                        className={cn(
                          'flex items-center justify-between px-2.5 py-1.5 rounded-[var(--r-ctrl)] text-left text-[12px] font-mono transition-colors',
                          isSelected
                            ? 'bg-[var(--hover)] text-[var(--accent-b)] font-semibold'
                            : 'text-[var(--text)] hover:bg-[var(--hover)] hover:text-[var(--text)]',
                        )}
                      >
                        <span className="truncate">{model}</span>
                        {isSelected && (
                          <Check className="h-3.5 w-3.5 shrink-0 text-[var(--accent-b)] ml-2" />
                        )}
                      </button>
                    );
                  })
                )}
              </div>
            </div>
          )}
        </div>

        {/* Fetch Models Button */}
        <Button
          type="button"
          size="sm"
          variant="ghost"
          onClick={handleFetchModels}
          disabled={fetching || disabled}
          className="shrink-0 gap-1.5 text-[12px] h-[34px] px-3"
          title="根据上方配置的 Base URL 与 API Key 自动拉取支持的模型列表"
        >
          {fetching ? (
            <>
              <Loader2 className="h-3.5 w-3.5 animate-spin text-[var(--accent-b)]" />
              <span>获取中...</span>
            </>
          ) : (
            <>
              <RotateCw className="h-3.5 w-3.5 text-[var(--muted)]" />
              <span>获取模型</span>
            </>
          )}
        </Button>
      </div>

      {/* Status Feedback */}
      {error && (
        <div className="flex items-center gap-1.5 pl-27 text-[11px] text-rose-400">
          <AlertCircle className="h-3 w-3 shrink-0" />
          <span className="truncate">{error}</span>
        </div>
      )}
      {!error && successMsg && (
        <div className="flex items-center gap-1.5 pl-27 text-[11px] text-emerald-400">
          <Check className="h-3 w-3 shrink-0" />
          <span>{successMsg}</span>
        </div>
      )}
    </div>
  );
};
