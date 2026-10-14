import React, { useEffect, useRef, useState } from 'react';
import { dismissShell } from '../hooks/useWindowAnimation';
import { IconButton, Pill, Select } from './ui';
import {
  ArrowLeftRight,
  BookOpen,
  Check,
  ChevronLeft,
  Copy,
  Download,
  GripVertical,
  Maximize2,
  Minimize2,
  Pin,
  Settings,
  Volume2,
  X,
} from 'lucide-react';
import { InteractiveSentence } from './InteractiveSentence';
import { ReplaceSelectionButton } from './ReplaceSelectionButton';
import { useAppStore } from '../store/useAppStore';
import { saveSettings } from '../store/persistence';
import { startWindowDrag } from '../utils/drag';
import { openSettingsWindow } from '../utils/windows';
import { cn } from '../utils/cn';
import { playPronunciation, stopPronunciation } from '../utils/audio';
import { recallScrollTop, rememberScrollTop } from '../utils/scrollMemory';
import type { LookupNode, ProviderType } from '../types';

const CARD_PROVIDER_OPTIONS: { value: ProviderType; label: string }[] = [
  { value: 'bing_free', label: 'Bing' },
  { value: 'google_free', label: 'Google' },
  { value: 'youdao_free', label: '有道' },
  { value: 'tencent_free', label: '腾讯' },
  { value: 'openai', label: 'OpenAI' },
];

const AI_TEMPLATE_OPTIONS = [
  { value: 'direct', label: '直译' },
  { value: 'academic', label: '学术' },
  { value: 'code', label: '代码' },
  { value: 'casual', label: '口语' },
];

const QUICK_LANG_OPTIONS = [
  { value: 'zh-CN', label: '中文' },
  { value: 'en', label: '英语' },
  { value: 'ja', label: '日语' },
  { value: 'ko', label: '韩语' },
  { value: 'fr', label: '法语' },
  { value: 'de', label: '德语' },
  { value: 'ru', label: '俄语' },
  { value: 'es', label: '西语' },
];

/** Split an enriched phonetic string like `英 /uk/ 美 /us/` into labelled parts. */
function splitPhonetic(phonetic: string): { label: string; value: string }[] | null {
  const uk = phonetic.indexOf('英');
  const us = phonetic.indexOf('美');
  if (uk === -1 && us === -1) return null;

  const parts: { label: string; value: string }[] = [];
  if (uk !== -1) {
    const end = us > uk ? us : phonetic.length;
    const value = phonetic.slice(uk + 1, end).trim();
    if (value) parts.push({ label: '英', value });
  }
  if (us !== -1) {
    const value = phonetic.slice(us + 1).trim();
    if (value) parts.push({ label: '美', value });
  }

  return parts.length ? parts : null;
}

/** ECDICT exchange codes -> readable labels. */
const EXCHANGE_LABELS: Record<string, string> = {
  p: '过去式',
  d: '过去分词',
  i: '现在分词',
  '3': '三单',
  r: '比较级',
  t: '最高级',
  s: '复数',
};

function exchangeItems(exchange: string): { label: string; value: string }[] {
  return exchange
    .split('/')
    .map((item) => {
      const [code, value] = item.split(':');
      return { code, value };
    })
    .filter((item) => Boolean(item.value) && Boolean(EXCHANGE_LABELS[item.code]))
    .map((item) => ({ label: EXCHANGE_LABELS[item.code], value: item.value }));
}

/** Short label used in the breadcrumb trail. */
function crumbLabel(node: LookupNode): string {
  if (node.kind === 'word') return node.text;
  return node.text.length > 10 ? `${node.text.slice(0, 10)}…` : node.text;
}

const Breadcrumb: React.FC<{ stack: LookupNode[] }> = ({ stack }) => {
  const popTo = useAppStore((state) => state.popTo);
  if (stack.length < 3) return null;

  const head = stack[0];
  const tail = stack.slice(-2);

  const crumb = (node: LookupNode, index: number, isCurrent: boolean) => (
    <button
      key={node.id}
      type="button"
      disabled={isCurrent}
      onClick={() => popTo(index)}
      title={node.text}
      className={cn(
        'max-w-[72px] truncate rounded-[6px] px-1 py-0.5 transition-colors',
        isCurrent
          ? 'font-medium text-[var(--text)]'
          : 'text-[var(--muted)] hover:bg-[var(--hover)] hover:text-[var(--text)]',
      )}
    >
      {crumbLabel(node)}
    </button>
  );

  return (
    <div className="flex min-w-0 items-center gap-0.5 text-[10px]">
      {crumb(head, 0, false)}
      {stack.length > 3 && <span className="shrink-0 opacity-60">…</span>}
      {tail.map((node, i) => {
        const index = stack.length - tail.length + i;
        return (
          <React.Fragment key={node.id}>
            <span className="shrink-0 opacity-50">›</span>
            {crumb(node, index, index === stack.length - 1)}
          </React.Fragment>
        );
      })}
    </div>
  );
};

interface LookupCardProps {
  node: LookupNode;
  stack: LookupNode[];
  stackIndex: number;
  isTop: boolean;
  fillHeight?: boolean;
}

/**
 * A single frame of the drill-down stack.
 *
 * Renders either a compact sentence translation or a dense word card, plus the
 * navigation header (back arrow, breadcrumb, depth badge) and card actions.
 */
export const LookupCard: React.FC<LookupCardProps> = ({
  node,
  stack,
  stackIndex,
  isTop,
  fillHeight = false,
}) => {
  const popLookup = useAppStore((state) => state.popLookup);
  const clearLookup = useAppStore((state) => state.clearLookup);
  const setDensity = useAppStore((state) => state.setDensity);
  const togglePinned = useAppStore((state) => state.togglePinned);
  const pinned = useAppStore((state) => state.pinned);
  const exportTsv = useAppStore((state) => state.exportTsv);
  const autoPronounce = useAppStore((state) => state.autoPronounce);
  const ocr = useAppStore((state) => state.ocr);
  const fontSize = useAppStore((state) => state.fontSize);
  const targetLanguage = useAppStore((state) => state.targetLanguage);
  const setTargetLanguage = useAppStore((state) => state.setTargetLanguage);
  const aiPromptTemplate = useAppStore((state) => state.aiPromptTemplate);
  const setAiPromptTemplate = useAppStore((state) => state.setAiPromptTemplate);
  const backTranslation = useAppStore((state) => state.backTranslation);
  const runBackTranslation = useAppStore((state) => state.runBackTranslation);
  const startStreamTranslation = useAppStore((state) => state.startStreamTranslation);
  const provider = useAppStore((state) => state.provider);
  const setProvider = useAppStore((state) => state.setProvider);

  const [copied, setCopied] = useState(false);
  const [isPlayingAudio, setIsPlayingAudio] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);
  const isTopRef = useRef(isTop);
  isTopRef.current = isTop;
  const hasAutoPronouncedRef = useRef(false);
  const copyTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Restore the previous scroll offset when this frame becomes the top card.
  useEffect(() => {
    if (isTop && scrollRef.current) {
      scrollRef.current.scrollTop = recallScrollTop(node.id);
    }
  }, [isTop, node.id]);

  useEffect(() => {
    if (!isTop) {
      stopPronunciation();
      setIsPlayingAudio(false);
    }
  }, [isTop]);

  // Stop audio if this frame is popped while it was still the top card.
  useEffect(
    () => () => {
      if (isTopRef.current) stopPronunciation();
      if (copyTimerRef.current) clearTimeout(copyTimerRef.current);
    },
    [],
  );

  // Auto-pronounce on arrival if configured.
  useEffect(() => {
    if (!isTop || !autoPronounce || hasAutoPronouncedRef.current) return;
    if (node.kind === 'word' && node.status === 'ready' && node.wordData) {
      hasAutoPronouncedRef.current = true;
      playPronunciation(node.wordData.word, node.wordData.audio_url, () => {});
    }
  }, [isTop, autoPronounce, node.kind, node.status, node.text, node.wordData]);

  const total = stack.length;
  const depth = stackIndex + 1;
  const wordData = node.wordData;
  const phones = wordData?.phonetic ? splitPhonetic(wordData.phonetic) : null;
  const exchanges = wordData?.exchange ? exchangeItems(wordData.exchange) : [];
  const compact = node.density === 'compact';
  const displayWord = wordData?.word || node.text;

  const handleBack = () => {
    if (total > 1) {
      popLookup();
    } else {
      clearLookup();
      stopPronunciation();
      if (!ocr) {
        void dismissShell({ force: true, immediate: true });
      }
    }
  };

  const handleClose = () => {
    clearLookup();
    stopPronunciation();
    void dismissShell({ force: true, immediate: true });
  };

  const handleCopy = async () => {
    const text = wordData ? `${displayWord}\n${node.translation}` : node.translation;
    await navigator.clipboard.writeText(text.trim());
    setCopied(true);
    if (copyTimerRef.current) clearTimeout(copyTimerRef.current);
    copyTimerRef.current = setTimeout(() => {
      setCopied(false);
      copyTimerRef.current = null;
    }, 2000);
  };

  const handlePronounce = () => {
    setIsPlayingAudio(true);
    playPronunciation(displayWord, wordData?.audio_url, () => setIsPlayingAudio(false));
  };

  const pinButton = (
    <IconButton
      title={pinned ? '取消固定 (Ctrl+Shift+T)' : '固定浮层 (Ctrl+Shift+T)'}
      active={pinned}
      onClick={togglePinned}
      className={pinned ? 'text-amber-400' : undefined}
    >
      <Pin className="h-3.5 w-3.5" />
    </IconButton>
  );

  const densityButton = (
    <IconButton
      title={compact ? '展开卡片详情' : '折叠卡片'}
      onClick={() => setDensity(compact ? 'dense' : 'compact')}
    >
      {compact ? (
        <Maximize2 className="h-3.5 w-3.5 text-[var(--accent-b)]" />
      ) : (
        <Minimize2 className="h-3.5 w-3.5 text-[var(--accent-b)]" />
      )}
    </IconButton>
  );

  const settingsButton = (
    <IconButton title="设置中心 (Ctrl+A)" onClick={() => void openSettingsWindow()}>
      <Settings className="h-3.5 w-3.5" />
    </IconButton>
  );

  const closeButton = (
    <IconButton
      title="关闭浮层 (Esc)"
      onClick={handleClose}
      className="hover:bg-rose-500/90 hover:text-white"
    >
      <X className="h-3.5 w-3.5" />
    </IconButton>
  );

  const meaning = (
    <div className="flex flex-col gap-2">
      {wordData?.definitions && wordData.definitions.length > 0 ? (
        wordData.definitions.map((def, idx) => (
          <div key={idx} className="flex items-start gap-2 text-sm leading-snug">
            <Pill tone="pos" className="shrink-0 px-2 py-0.5 text-[11px]">
              {def.pos}
            </Pill>
            <span>{def.meanings.join('；')}</span>
          </div>
        ))
      ) : node.status === 'loading' ? (
        <div className="flex flex-col gap-1.5">
          <div className="h-3 w-3/4 animate-pulse rounded bg-[var(--surface-3)]" />
          <div className="h-3 w-1/2 animate-pulse rounded bg-[var(--surface-3)]" />
        </div>
      ) : node.status === 'error' ? (
        <p className="text-sm text-rose-300">查询失败，请检查网络或 provider 设置。</p>
      ) : node.status === 'notfound' || !node.translation ? (
        <p className="text-sm text-[var(--muted)]">未收录该词。</p>
      ) : (
        <p className="select-text text-sm leading-relaxed">
          <InteractiveSentence sentence={node.translation} />
          {node.status === 'streaming' && (
            <span className="ml-0.5 inline-block h-3 w-1 animate-pulse bg-[var(--accent-b)] align-middle" />
          )}
        </p>
      )}
    </div>
  );

  return (
    <div
      style={{ fontSize: `${fontSize}px` }}
      className={cn(
        'surface surface-hairline relative w-full overflow-hidden rounded-[var(--r-card)] text-[var(--text)] flex flex-col',
        fillHeight && 'h-full',
      )}
    >
      {/* ------------------------------------------------ header */}
      <div
        onMouseDown={isTop ? startWindowDrag : undefined}
        className={cn(
          'flex shrink-0 items-center justify-between gap-2 border-b border-[var(--stroke)] px-3 py-1.5 bg-[var(--surface-2)]/65 shadow-[inset_0_1px_0_var(--highlight)] backdrop-blur-sm select-none',
          isTop ? 'cursor-grab active:cursor-grabbing' : 'cursor-default',
        )}
      >
        <div className="flex min-w-0 items-center gap-1.5">
          {isTop && (
            <div
              className="flex shrink-0 items-center text-[var(--muted)] opacity-40 hover:opacity-90 transition-opacity"
              title="按住拖拽窗口"
            >
              <GripVertical className="h-3.5 w-3.5" />
            </div>
          )}
          {(total > 1 || Boolean(ocr)) && (
            <IconButton title={ocr && total === 1 ? "返回截图对照" : "返回上一层 (Alt+←)"} onClick={handleBack}>
              <ChevronLeft className="h-4 w-4" />
            </IconButton>
          )}
          <Breadcrumb stack={stack} />
          {total > 1 && (
            <span className="shrink-0 rounded-full bg-[var(--surface-3)] px-1.5 py-0.5 font-mono text-[10px] text-[var(--muted)]">
              {depth}/{total}
            </span>
          )}
          {node.kind === 'sentence' && (
            <div className="flex items-center gap-1 ml-1">
              <button
                type="button"
                onClick={() => {
                  const nextLang = targetLanguage.startsWith('zh') ? 'en' : 'zh-CN';
                  setTargetLanguage(nextLang);
                  void startStreamTranslation(node.text, node.id);
                }}
                title="点击在自动/中文与目标语言间快速互译"
                className="flex items-center gap-0.5 rounded bg-[var(--surface-3)] px-1.5 py-0 font-mono text-[10px] text-[var(--muted)] hover:bg-[var(--hover)] hover:text-[var(--text)] transition-colors border border-[var(--stroke)] h-5 cursor-pointer"
              >
                <span>自动 ⇄ {QUICK_LANG_OPTIONS.find((l) => l.value === targetLanguage)?.label ?? targetLanguage}</span>
              </button>
              <Select
                size="xs"
                value={targetLanguage}
                onChange={(nextLang) => {
                  setTargetLanguage(nextLang);
                  void startStreamTranslation(node.text, node.id);
                }}
                options={QUICK_LANG_OPTIONS}
                title="选择目标语言"
              />
              <Select
                size="xs"
                value={provider}
                onChange={(next) => {
                  const nextProvider = next as ProviderType;
                  setProvider(nextProvider);
                  void saveSettings({ provider: nextProvider });
                  void startStreamTranslation(node.text, node.id);
                }}
                options={CARD_PROVIDER_OPTIONS}
                title="切换翻译引擎"
              />
              {provider === 'openai' && (
                <Select
                  size="xs"
                  value={aiPromptTemplate}
                  onChange={(val) => {
                    const t = val as 'direct' | 'academic' | 'code' | 'casual';
                    setAiPromptTemplate(t);
                    void startStreamTranslation(node.text, node.id);
                  }}
                  options={AI_TEMPLATE_OPTIONS}
                  title="AI 提示词模板"
                />
              )}
            </div>
          )}
        </div>

        <div className="flex shrink-0 items-center gap-0.5">
          {node.kind === 'word' ? (
            <IconButton title="导出单词本 (TSV)" onClick={() => exportTsv()}>
              <Download className="h-3.5 w-3.5" />
            </IconButton>
          ) : (
            <>
              <IconButton
                title="回译验证 (⇌)"
                onClick={() => void runBackTranslation()}
                active={Boolean(backTranslation)}
                className={backTranslation ? 'text-[var(--accent-b)]' : undefined}
              >
                <ArrowLeftRight className="h-3.5 w-3.5" />
              </IconButton>
              <IconButton title="复制译文" onClick={() => void handleCopy()}>
                {copied ? (
                  <Check className="h-3.5 w-3.5 text-emerald-400" />
                ) : (
                  <Copy className="h-3.5 w-3.5" />
                )}
              </IconButton>
            </>
          )}
          {densityButton}
          {pinButton}
          {settingsButton}
          {closeButton}
        </div>
      </div>

      <div className={cn('flex flex-col flex-1 min-h-0', compact ? 'p-3' : 'p-3.5')}>
        {stackIndex === 0 && <ReplaceSelectionButton node={node} />}

        {/* ------------------------------------------------ body */}
      {node.kind === 'sentence' ? (
        <div
          ref={scrollRef}
          onScroll={() => scrollRef.current && rememberScrollTop(node.id, scrollRef.current.scrollTop)}
          className={cn(
            'flex select-text flex-col gap-1.5 overflow-y-auto pr-0.5',
            fillHeight ? 'mt-2.5 flex-1 min-h-0' : (compact ? 'mt-2.5 max-h-[520px]' : 'mt-3 max-h-[560px]'),
          )}
        >
          <InteractiveSentence
            sentence={node.text}
            muted
            className="text-xs italic"
            currentWord={node.highlightWord}
          />
          <div
            className={cn(
              'font-medium leading-relaxed',
              compact ? 'text-sm' : 'text-base',
            )}
          >
            {node.status === 'loading' ? (
              <span className="animate-pulse text-[var(--faint)]">Analyzing…</span>
            ) : node.status === 'notfound' ? (
              <span className="text-[var(--muted)]">未获得翻译。</span>
            ) : (
              <>
                <InteractiveSentence
                  sentence={node.translation}
                  currentWord={node.highlightWord}
                />
                {node.status === 'streaming' && (
                  <span className="ml-0.5 inline-block h-3.5 w-1.5 animate-pulse bg-[var(--accent-b)] align-middle" />
                )}
              </>
            )}
          </div>

          {backTranslation && (
            <div className="mt-2 rounded-[var(--r-inner)] border border-[var(--stroke)] bg-[var(--surface-2)]/80 p-2 text-xs">
              <div className="flex items-center justify-between text-[10px] text-[var(--faint)] mb-1">
                <span className="flex items-center gap-1 font-mono">
                  <ArrowLeftRight className="h-2.5 w-2.5" />
                  <span>回译验证</span>
                </span>
                {backTranslation.loading && <span className="animate-pulse">翻译中...</span>}
              </div>
              {backTranslation.error ? (
                <span className="text-rose-400">{backTranslation.error}</span>
              ) : (
                <span className="text-[var(--text)] select-text">
                  {backTranslation.text || (backTranslation.loading ? '正在反向翻译...' : '')}
                </span>
              )}
            </div>
          )}
        </div>
      ) : (
        <div
          ref={scrollRef}
          onScroll={() => scrollRef.current && rememberScrollTop(node.id, scrollRef.current.scrollTop)}
          className={cn(
            'flex select-text flex-col gap-3 overflow-y-auto pr-1',
            fillHeight ? 'mt-2.5 flex-1 min-h-0' : (compact ? 'mt-2.5 max-h-[520px]' : 'mt-3 max-h-[560px]'),
          )}
        >
          {/* Headword */}
          <div className="flex flex-wrap items-baseline gap-2.5">
            <h1
              onClick={handlePronounce}
              title="点击发音"
              className="cursor-pointer text-2xl font-bold capitalize tracking-tight transition-colors hover:text-[var(--accent-b)]"
            >
              {displayWord}
            </h1>
            {phones
              ? phones.map((phone) => (
                  <span
                    key={phone.label}
                    onClick={handlePronounce}
                    title="点击发音"
                    className="cursor-pointer whitespace-nowrap font-mono text-xs text-[var(--accent-b)] transition-opacity hover:opacity-80"
                  >
                    <span className="mr-0.5 text-[var(--faint)]">{phone.label}</span>
                    {phone.value}
                  </span>
                ))
              : wordData?.phonetic && (
                  <span
                    onClick={handlePronounce}
                    title="点击发音"
                    className="cursor-pointer font-mono text-sm text-[var(--accent-b)] transition-opacity hover:opacity-80"
                  >
                    {wordData.phonetic}
                  </span>
                )}
            <IconButton
              title="Pronounce"
              onClick={handlePronounce}
              className={isPlayingAudio ? 'animate-pulse text-[var(--accent-b)]' : undefined}
            >
              <Volume2 className="h-4 w-4" />
            </IconButton>
            {wordData?.tags?.map((tag) => <Pill key={tag}>{tag}</Pill>)}
          </div>

          {/* Origin context (source sentence) */}
          {node.originText && (
            <div className="flex items-start gap-2 rounded-[var(--r-inner)] bg-[var(--surface-2)] px-2.5 py-1.5 text-[11px]">
              <span className="shrink-0 text-[var(--faint)]">出处</span>
              <InteractiveSentence
                sentence={node.originText}
                currentWord={node.originWord}
                className="text-[11px] italic text-[var(--muted)]"
              />
            </div>
          )}

          {!compact && exchanges.length > 0 && (
            <div className="flex flex-wrap gap-1.5">
              {exchanges.map((item, idx) => (
                <span
                  key={idx}
                  onClick={() => void useAppStore.getState().pushLookup(item.value, { text: displayWord, word: item.value })}
                  title={`点击查词并朗读 "${item.value}"`}
                  className="cursor-pointer rounded-full border border-[var(--stroke)] bg-[var(--surface-2)] px-2 py-0.5 text-[11px] transition-colors hover:border-[var(--accent-b)] hover:text-[var(--text)]"
                >
                  <span className="mr-1 text-[var(--muted)]">{item.label}</span>
                  {item.value}
                </span>
              ))}
            </div>
          )}

          {/* Meaning / translation fallback */}
          {meaning}

          {!compact && wordData?.english_definitions && wordData.english_definitions.length > 0 && (
            <div className="flex flex-col gap-1.5">
              <span className="text-[10px] uppercase tracking-[0.09em] text-[var(--muted)]">
                English Definitions
              </span>
              {wordData.english_definitions.map((def, idx) => (
                <div key={idx} className="flex items-start gap-2 text-xs leading-snug">
                  <Pill tone="pos" className="shrink-0">
                    {def.pos}
                  </Pill>
                  <span className="text-[var(--muted)]">{def.meanings.join(' ')}</span>
                </div>
              ))}
            </div>
          )}

          {!compact && wordData?.examples && wordData.examples.length > 0 && (
            <div className="flex flex-col gap-2 border-t border-[var(--stroke)] pt-2">
              <div className="flex items-center justify-between text-[11px]">
                <div className="flex items-center gap-1 text-[var(--accent-b)]">
                  <BookOpen className="h-3 w-3" />
                  <span>Context &amp; Drill-down</span>
                </div>
                <span className="text-[10px] italic text-[var(--faint)]">
                  Click words to drill down
                </span>
              </div>
              <div className="flex flex-col gap-2.5">
                {wordData.examples.map((ex, idx) => (
                  <div
                    key={idx}
                    className="rounded-[var(--r-inner)] border border-[var(--stroke)] bg-[var(--surface-2)] p-2 transition-colors hover:bg-[var(--surface-3)]"
                  >
                    <InteractiveSentence
                      sentence={ex.en}
                      currentWord={node.highlightWord}
                    />
                    {ex.zh && <p className="mt-1 text-xs text-[var(--muted)]">{ex.zh}</p>}
                  </div>
                ))}
              </div>
            </div>
          )}
        </div>
      )}
      </div>
    </div>
  );
};
