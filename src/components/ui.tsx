import React, { forwardRef, useEffect, useRef, useState } from 'react';
import { Check, ChevronDown } from 'lucide-react';
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
export const Input = forwardRef<HTMLInputElement, React.InputHTMLAttributes<HTMLInputElement>>(
  ({ className, ...props }, ref) => (
    <input
      ref={ref}
      className={cn(
        'flex-1 rounded-[var(--r-ctrl)] border border-[var(--stroke)] bg-[var(--surface-2)] px-2.5 py-1.5 text-[13px] text-[var(--text)] placeholder:text-[var(--faint)] focus-ring',
        className,
      )}
      {...props}
    />
  ),
);
Input.displayName = 'Input';

export interface SelectOption {
  value: string;
  label: string;
}

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
  ariaLabel?: string;
}> = ({ value, onChange, options, disabled = false, className, ariaLabel }) => {
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent) => {
      if (!root.current?.contains(event.target as Node)) setOpen(false);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setOpen(false);
    };
    document.addEventListener('pointerdown', onPointerDown);
    document.addEventListener('keydown', onKeyDown);
    return () => {
      document.removeEventListener('pointerdown', onPointerDown);
      document.removeEventListener('keydown', onKeyDown);
    };
  }, [open]);

  const selected = options.find((option) => option.value === value);

  return (
    <div ref={root} className="relative flex-1">
      <button
        type="button"
        disabled={disabled}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label={ariaLabel}
        onClick={() => setOpen((previous) => !previous)}
        className={cn(
          'flex w-full items-center justify-between gap-2 rounded-[var(--r-ctrl)] border border-[var(--stroke)] bg-[var(--surface-2)] py-1.5 pl-2.5 pr-2 text-left text-[13px] text-[var(--text)] focus-ring disabled:opacity-40',
          className,
        )}
      >
        <span className="truncate">{selected?.label ?? value}</span>
        <ChevronDown
          className={cn(
            'h-3.5 w-3.5 shrink-0 text-[var(--muted)] transition-transform',
            open && 'rotate-180',
          )}
        />
      </button>

      {open && (
        <ul
          role="listbox"
          className="absolute z-50 mt-1 max-h-64 w-full min-w-[140px] overflow-y-auto rounded-[var(--r-inner)] border border-[var(--stroke)] bg-[var(--popover-bg)] p-1 shadow-[0_12px_32px_rgba(0,0,0,0.5)] backdrop-blur-xl"
        >
          {options.map((option) => {
            const active = option.value === value;
            return (
              <li key={option.value} role="option" aria-selected={active}>
                <button
                  type="button"
                  onClick={() => {
                    onChange(option.value);
                    setOpen(false);
                  }}
                  className={cn(
                    'flex w-full items-center justify-between gap-2 rounded-[var(--r-ctrl)] px-2 py-1.5 text-left text-[13px] transition-colors',
                    active
                      ? 'bg-[var(--pos-bg)] font-medium text-[var(--pos-fg)]'
                      : 'text-[var(--text)] hover:bg-[var(--hover)]',
                  )}
                >
                  <span className="truncate">{option.label}</span>
                  {active && <Check className="h-3.5 w-3.5 shrink-0" />}
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
