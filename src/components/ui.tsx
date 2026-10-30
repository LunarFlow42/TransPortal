import React, { forwardRef, useEffect, useRef, useState } from 'react';
import { Check, ChevronDown } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { cn } from '../utils/cn';

/* ------------------------------------------------------------------ *
 * Surface — the glass container used by the overlay and settings.
 * ------------------------------------------------------------------ */
interface SurfaceProps extends React.HTMLAttributes<HTMLDivElement> {
  /** `glass` is the outer translucent card; `inner` is a flat grouped row. */
  variant?: 'glass' | 'inner';
  /** Render the gradient hairline + top highlight (glass only). */
  hairline?: boolean;
}

export const Surface = forwardRef<HTMLDivElement, SurfaceProps>(
  ({ variant = 'glass', hairline = true, className, children, ...props }, ref) => (
    <div
      ref={ref}
      className={cn(
        variant === 'glass'
          ? 'surface rounded-[var(--r-card)]'
          : 'inner-card rounded-[var(--r-inner)]',
        hairline && variant === 'glass' && 'surface-hairline',
        className,
      )}
      {...props}
    >
      {children}
    </div>
  ),
);
Surface.displayName = 'Surface';

/* ------------------------------------------------------------------ *
 * Section — labeled settings group.
 * ------------------------------------------------------------------ */
export const Section: React.FC<{
  label: string;
  icon?: React.ReactNode;
  children: React.ReactNode;
  className?: string;
}> = ({ label, icon, children, className }) => (
  <section className={cn('flex flex-col gap-2.5', className)}>
    <h3 className="flex items-center gap-1.5 text-[11px] font-semibold uppercase tracking-[0.09em] text-[var(--muted)]">
      {icon}
      {label}
    </h3>
    {children}
  </section>
);

/* ------------------------------------------------------------------ *
 * Button
 * ------------------------------------------------------------------ */
type ButtonVariant = 'primary' | 'ghost' | 'subtle' | 'danger';
type ButtonSize = 'sm' | 'md';

interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: ButtonSize;
}

const BUTTON_VARIANTS: Record<ButtonVariant, string> = {
  primary:
    'text-white border-0 bg-gradient-to-br from-[var(--accent-a)] to-[var(--accent-b)] shadow-[0_6px_18px_rgba(34,211,238,0.25)] hover:brightness-110',
  ghost:
    'bg-[var(--surface-2)] text-[var(--text)] border border-[var(--stroke)] hover:bg-[var(--surface-3)]',
  subtle: 'bg-transparent text-[var(--muted)] border-0 hover:bg-[var(--hover)] hover:text-[var(--text)]',
  danger: 'bg-rose-500/90 text-white border-0 hover:bg-rose-500',
};

const BUTTON_SIZES: Record<ButtonSize, string> = {
  sm: 'gap-1 px-2 py-1 text-[11px]',
  md: 'gap-1.5 px-3 py-1.5 text-xs',
};

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  ({ variant = 'ghost', size = 'md', className, children, ...props }, ref) => (
    <button
      ref={ref}
      className={cn(
        'inline-flex items-center justify-center rounded-[var(--r-ctrl)] font-medium transition-colors',
        'focus-ring disabled:cursor-not-allowed disabled:opacity-40',
        BUTTON_SIZES[size],
        BUTTON_VARIANTS[variant],
        className,
      )}
      {...props}
    >
      {children}
    </button>
  ),
);
Button.displayName = 'Button';

/* ------------------------------------------------------------------ *
 * IconButton
 * ------------------------------------------------------------------ */
interface IconButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  active?: boolean;
}

export const IconButton = forwardRef<HTMLButtonElement, IconButtonProps>(
  ({ active = false, className, children, ...props }, ref) => (
    <button
      ref={ref}
      className={cn(
        'grid h-6.5 w-6.5 shrink-0 place-items-center rounded-[var(--r-ctrl)] transition-colors',
        'focus-ring',
        active
          ? 'bg-[var(--hover)] text-[var(--text)]'
          : 'text-[var(--muted)] hover:bg-[var(--hover)] hover:text-[var(--text)]',
        className,
      )}
      {...props}
    >
      {children}
    </button>
  ),
);
IconButton.displayName = 'IconButton';

/* ------------------------------------------------------------------ *
 * Switch — accessible toggle.
 * ------------------------------------------------------------------ */
interface SwitchProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
  label?: string;
  className?: string;
}

export const Switch: React.FC<SwitchProps> = ({
  checked,
  onChange,
  disabled = false,
  label,
  className,
}) => (
  <button
    type="button"
    role="switch"
    aria-checked={checked}
    aria-label={label}
    disabled={disabled}
    onClick={() => onChange(!checked)}
    className={cn(
      'relative h-[22px] w-[38px] shrink-0 rounded-full transition-colors focus-ring disabled:opacity-40',
      checked
        ? 'bg-gradient-to-br from-[var(--accent-a)] to-[var(--accent-b)] shadow-[0_4px_12px_rgba(34,211,238,0.28)]'
        : 'bg-[var(--surface-3)]',
      className,
    )}
  >
    <span
      className={cn(
        'absolute top-[2px] h-[18px] w-[18px] rounded-full bg-white shadow transition-all',
        checked ? 'left-[18px]' : 'left-[4px]',
      )}
    />
  </button>
);

/* ------------------------------------------------------------------ *
 * Input / Select
 * ------------------------------------------------------------------ */
export interface InputProps extends React.InputHTMLAttributes<HTMLInputElement> {
  leftIcon?: React.ReactNode;
  rightElement?: React.ReactNode;
  clearable?: boolean;
  onClear?: () => void;
}

export const Input = forwardRef<HTMLInputElement, InputProps>(
  ({ className, leftIcon, rightElement, clearable, onClear, value, onChange, ...props }, ref) => {
    if (!leftIcon && !rightElement && !clearable) {
      return (
        <input
          ref={ref}
          value={value}
          onChange={onChange}
          className={cn(
            'w-full flex-1 min-w-0 rounded-[var(--r-ctrl)] border border-[var(--stroke)] bg-[var(--surface-2)] px-2.5 py-1.5 text-[13px] text-[var(--text)] placeholder:text-[var(--faint)] focus-ring',
            className,
          )}
          {...props}
        />
      );
    }

    const hasValue = Boolean(value && String(value).length > 0);

    return (
      <div
        className={cn(
          'relative flex flex-1 min-w-0 items-center rounded-[var(--r-ctrl)] border border-[var(--stroke)] bg-[var(--surface-2)] transition-colors focus-within:border-[var(--accent-b)] focus-within:bg-[var(--surface-3)]',
          className,
        )}
      >
        {leftIcon && (
          <span className="pointer-events-none flex shrink-0 items-center pl-2.5 text-[var(--muted)]">
            {leftIcon}
          </span>
        )}
        <input
          ref={ref}
          value={value}
          onChange={onChange}
          className={cn(
            'w-full flex-1 min-w-0 bg-transparent py-1.5 text-[13px] text-[var(--text)] placeholder:text-[var(--faint)] outline-none',
            leftIcon ? 'pl-2' : 'pl-2.5',
            rightElement || (clearable && hasValue) ? 'pr-1.5' : 'pr-2.5',
          )}
          {...props}
        />
        {clearable && hasValue && (
          <button
            type="button"
            onClick={(e) => {
              e.stopPropagation();
              onClear?.();
            }}
            title="Clear"
            className="flex h-5 w-5 shrink-0 items-center justify-center rounded-full text-[var(--muted)] hover:bg-[var(--hover)] hover:text-[var(--text)] transition-colors"
          >
            <span className="text-xs leading-none">×</span>
          </button>
        )}
        {rightElement && (
          <span className="flex shrink-0 items-center pr-2 text-[var(--muted)]">
            {rightElement}
          </span>
        )}
      </div>
    );
  },
);
Input.displayName = 'Input';

export interface SelectOption {
  value: string;
  label: string;
  icon?: React.ReactNode;
}

export type SelectSize = 'xs' | 'sm' | 'md';

/**
 * Themed dropdown.
 *
 * Native `<select>` popups ignore `color-scheme` in WebView2 and render as an
 * unreadable white list in dark mode, so the options are drawn in-page.
 */
export const Select: React.FC<{
  value: string;
  onChange: (value: string) => void;
  options: SelectOption[];
  disabled?: boolean;
  className?: string;
  size?: SelectSize;
  ariaLabel?: string;
  title?: string;
}> = ({
  value,
  onChange,
  options,
  disabled = false,
  className,
  size = 'md',
  ariaLabel,
  title,
}) => {
  const [open, setOpen] = useState(false);
  const [dropUp, setDropUp] = useState(false);
  const [highlightedIndex, setHighlightedIndex] = useState(-1);
  const root = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent) => {
      if (!root.current?.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener('pointerdown', onPointerDown);
    return () => {
      document.removeEventListener('pointerdown', onPointerDown);
    };
  }, [open]);

  useEffect(() => {
    if (open) {
      if (root.current) {
        const rect = root.current.getBoundingClientRect();
        const spaceBelow = window.innerHeight - rect.bottom;
        const spaceAbove = rect.top;
        setDropUp(spaceBelow < 170 && spaceAbove > spaceBelow);
      }
      const currentIndex = options.findIndex((option) => option.value === value);
      setHighlightedIndex(currentIndex >= 0 ? currentIndex : 0);
    }
  }, [open, options, value]);

  const handleKeyDown = (event: React.KeyboardEvent) => {
    if (disabled) return;
    if (!open) {
      if (['ArrowDown', 'ArrowUp', 'Enter', ' '].includes(event.key)) {
        event.preventDefault();
        setOpen(true);
      }
      return;
    }

    if (event.key === 'ArrowDown') {
      event.preventDefault();
      setHighlightedIndex((prev) => (prev + 1) % options.length);
    } else if (event.key === 'ArrowUp') {
      event.preventDefault();
      setHighlightedIndex((prev) => (prev - 1 + options.length) % options.length);
    } else if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      if (highlightedIndex >= 0 && highlightedIndex < options.length) {
        onChange(options[highlightedIndex].value);
        setOpen(false);
      }
    } else if (event.key === 'Escape') {
      event.preventDefault();
      setOpen(false);
      triggerRef.current?.focus();
    }
  };

  const selected = options.find((option) => option.value === value);

  const SIZE_STYLES = {
    xs: {
      button: 'h-5 py-0 px-1.5 text-[10px] gap-1 font-mono rounded',
      chevron: 'h-2.5 w-2.5',
      listbox: 'p-0.5 min-w-[100px]',
      item: 'px-1.5 py-0.5 text-[10px] gap-1 font-mono rounded',
      check: 'h-2.5 w-2.5',
    },
    sm: {
      button: 'h-7 py-1 px-2 text-[12px] gap-1.5 rounded-[var(--r-ctrl)]',
      chevron: 'h-3 w-3',
      listbox: 'p-1 min-w-[120px]',
      item: 'px-2 py-1 text-[12px] gap-1.5 rounded-[var(--r-ctrl)]',
      check: 'h-3 w-3',
    },
    md: {
      button: 'py-1.5 pl-2.5 pr-2 text-[13px] gap-2 rounded-[var(--r-ctrl)]',
      chevron: 'h-3.5 w-3.5',
      listbox: 'p-1 min-w-[140px]',
      item: 'px-2 py-1.5 text-[13px] gap-2 rounded-[var(--r-ctrl)]',
      check: 'h-3.5 w-3.5',
    },
  }[size];

  return (
    <div
      ref={root}
      onKeyDown={handleKeyDown}
      className={cn('relative inline-flex', size === 'md' && 'flex-1')}
    >
      <button
        ref={triggerRef}
        type="button"
        title={title}
        disabled={disabled}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label={ariaLabel}
        onClick={() => setOpen((previous) => !previous)}
        className={cn(
          'flex w-full items-center justify-between border border-[var(--stroke)] bg-[var(--surface-2)] text-left text-[var(--text)] transition-colors hover:bg-[var(--surface-3)] focus-ring disabled:opacity-40 select-none cursor-pointer',
          SIZE_STYLES.button,
          className,
        )}
      >
        <span className="truncate flex items-center gap-1">
          {selected?.icon}
          {selected?.label ?? value}
        </span>
        <ChevronDown
          className={cn(
            'shrink-0 text-[var(--muted)] transition-transform',
            SIZE_STYLES.chevron,
            open && 'rotate-180',
          )}
        />
      </button>

      {open && (
        <ul
          role="listbox"
          className={cn(
            'absolute z-50 max-h-60 w-max min-w-full overflow-y-auto rounded-[var(--r-inner)] border border-[var(--stroke)] bg-[var(--popover-bg)] shadow-[0_12px_32px_rgba(0,0,0,0.5)] backdrop-blur-xl',
            dropUp ? 'bottom-full mb-1' : 'top-full mt-1',
            SIZE_STYLES.listbox,
          )}
        >
          {options.map((option, index) => {
            const active = option.value === value;
            const highlighted = index === highlightedIndex;
            return (
              <li key={option.value} role="option" aria-selected={active}>
                <button
                  type="button"
                  onMouseEnter={() => setHighlightedIndex(index)}
                  onClick={() => {
                    onChange(option.value);
                    setOpen(false);
                    triggerRef.current?.focus();
                  }}
                  className={cn(
                    'flex w-full items-center justify-between text-left transition-colors cursor-pointer',
                    SIZE_STYLES.item,
                    active
                      ? 'bg-[var(--pos-bg)] font-medium text-[var(--pos-fg)]'
                      : highlighted
                        ? 'bg-[var(--hover)] text-[var(--text)]'
                        : 'text-[var(--text)] hover:bg-[var(--hover)]',
                  )}
                >
                  <span className="truncate flex items-center gap-1">
                    {option.icon}
                    {option.label}
                  </span>
                  {active && <Check className={cn('shrink-0', SIZE_STYLES.check)} />}
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
};

/* ------------------------------------------------------------------ *
 * SavedBadge — Transient feedback when settings are persisted
 * ------------------------------------------------------------------ */
export const SavedBadge: React.FC<{
  saved: boolean;
  className?: string;
}> = ({ saved, className }) => {
  const { t } = useTranslation();
  return (
  <div
    className={cn(
      'inline-flex items-center gap-1 rounded-full border border-emerald-500/30 bg-emerald-500/15 px-2 py-0.5 text-[10px] font-medium text-emerald-400 transition-all duration-300 select-none shadow-[0_2px_8px_rgba(16,185,129,0.15)]',
      saved
        ? 'opacity-100 scale-100 pointer-events-auto'
        : 'opacity-0 scale-95 pointer-events-none',
      className,
    )}
  >
    <Check className="h-2.5 w-2.5 shrink-0 text-emerald-400" />
    <span>{t('common.saved')}</span>
  </div>
  );
};

/* ------------------------------------------------------------------ *
 * Pill
 * ------------------------------------------------------------------ */
type PillTone = 'neutral' | 'pos' | 'online' | 'local';

const PILL_TONES: Record<PillTone, string> = {
  neutral: 'bg-[var(--surface-3)] text-[var(--muted)]',
  pos: 'bg-[var(--pos-bg)] text-[var(--pos-fg)] border border-[var(--pos-stroke)]',
  online: 'bg-sky-500/20 text-sky-400',
  local: 'bg-amber-500/20 text-amber-500',
};

export const Pill: React.FC<{
  tone?: PillTone;
  className?: string;
  children: React.ReactNode;
}> = ({ tone = 'neutral', className, children }) => (
  <span
    className={cn(
      'inline-flex items-center rounded-full px-2 py-[1px] text-[10px] font-semibold leading-relaxed',
      PILL_TONES[tone],
      className,
    )}
  >
    {children}
  </span>
);


/* ------------------------------------------------------------------ *
 * Tooltip & HoverTipText — elegant hover explanations without '?' icons
 * ------------------------------------------------------------------ */
export const Tooltip: React.FC<{
  content: React.ReactNode;
  children: React.ReactNode;
  className?: string;
  side?: 'top' | 'bottom';
  align?: 'left' | 'center';
}> = ({ content, children, className, side = 'top', align = 'left' }) => {
  const [visible, setVisible] = useState(false);
  return (
    <div
      className={cn('relative inline-flex items-center', className)}
      onMouseEnter={() => setVisible(true)}
      onMouseLeave={() => setVisible(false)}
      onFocus={() => setVisible(true)}
      onBlur={() => setVisible(false)}
    >
      {children}
      {visible && (
        <div
          role="tooltip"
          className={cn(
            'pointer-events-none absolute z-50 w-max max-w-xs rounded-[var(--r-inner)] border border-[var(--stroke)] bg-[var(--surface-1)] px-2.5 py-1.5 text-[11px] font-normal leading-relaxed text-[var(--text)] shadow-[0_8px_24px_rgba(0,0,0,0.5)] backdrop-blur-xl transition-all duration-150 animate-in fade-in select-none',
            side === 'top' ? 'bottom-full mb-1.5' : 'top-full mt-1.5',
            align === 'left' ? 'left-0' : 'left-1/2 -translate-x-1/2',
          )}
        >
          {content}
        </div>
      )}
    </div>
  );
};

export const HoverTipText: React.FC<{
  text: React.ReactNode;
  tip: React.ReactNode;
  className?: string;
}> = ({ text, tip, className }) => (
  <Tooltip content={tip} align="left">
    <span
      className={cn(
        'cursor-help transition-colors border-b border-dashed border-[var(--stroke-strong)]/60 hover:border-[var(--accent-b)] hover:text-[var(--accent-b)] select-none',
        className,
      )}
    >
      {text}
    </span>
  </Tooltip>
);

export const InfoHint: React.FC<{ text: React.ReactNode; className?: string }> = ({
  text,
  className,
}) => {
  const { i18n } = useTranslation();
  return (
    <HoverTipText
      text={i18n.language.startsWith('zh') ? '详情' : 'Info'}
      tip={text}
      className={className}
    />
  );
};
