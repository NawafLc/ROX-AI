import { Loader2 } from 'lucide-react'
import { ReactNode, useEffect } from 'react'

export function cx(...c: (string | false | null | undefined)[]) {
  return c.filter(Boolean).join(' ')
}

type BtnKind = 'primary' | 'secondary' | 'ghost' | 'danger' | 'warn'
export function Button({
  kind = 'secondary',
  size = 'md',
  icon,
  children,
  loading,
  className,
  ...p
}: { kind?: BtnKind; size?: 'sm' | 'md' | 'lg'; icon?: ReactNode; loading?: boolean } & React.ButtonHTMLAttributes<HTMLButtonElement>) {
  const kinds: Record<BtnKind, string> = {
    primary: 'bg-brand text-[#04140f] hover:bg-[#19e3b6] shadow-[0_0_0_1px_rgba(0,214,168,.4),0_8px_24px_-8px_rgba(0,214,168,.5)]',
    secondary: 'bg-card-2 text-ink hover:bg-line border border-line-2',
    ghost: 'text-ink-2 hover:text-ink hover:bg-card-2',
    danger: 'bg-rose/90 text-white hover:bg-rose',
    warn: 'bg-amber text-[#1d1300] hover:brightness-110',
  }
  const sizes = { sm: 'h-8 px-3 text-[13px] gap-1.5 rounded-lg', md: 'h-10 px-4 text-sm gap-2 rounded-[10px]', lg: 'h-12 px-5 text-[15px] gap-2.5 rounded-xl' }
  return (
    <button
      {...p}
      disabled={p.disabled || loading}
      className={cx(
        'inline-flex select-none items-center justify-center font-semibold transition-all duration-150 active:scale-[.98] disabled:cursor-not-allowed disabled:opacity-45 disabled:active:scale-100',
        kinds[kind],
        sizes[size],
        className,
      )}
    >
      {loading ? <Loader2 className="size-4 animate-spin" /> : icon}
      {children}
    </button>
  )
}

export function Card({ children, className, ...p }: { children: ReactNode } & React.HTMLAttributes<HTMLDivElement>) {
  return (
    <div {...p} className={cx('rounded-xl border border-line bg-card', className)}>
      {children}
    </div>
  )
}

export function Badge({ children, tone = 'neutral', className }: { children: ReactNode; tone?: 'neutral' | 'brand' | 'sky' | 'amber' | 'rose'; className?: string }) {
  const tones = {
    neutral: 'bg-card-2 text-ink-2 border-line-2',
    brand: 'bg-brand/12 text-brand border-brand/25',
    sky: 'bg-sky/12 text-sky border-sky/25',
    amber: 'bg-amber/12 text-amber border-amber/25',
    rose: 'bg-rose/12 text-rose border-rose/25',
  }
  return <span className={cx('inline-flex items-center gap-1 whitespace-nowrap rounded-full border px-2 py-0.5 text-[11.5px] font-semibold', tones[tone], className)}>{children}</span>
}

export function Stat({ label, value, sub, icon }: { label: string; value: ReactNode; sub?: ReactNode; icon?: ReactNode }) {
  return (
    <Card className="p-4">
      <div className="flex items-center justify-between text-[12px] font-medium text-mute">
        {label}
        <span className="text-mute/70">{icon}</span>
      </div>
      <div className="mt-2 text-[26px] font-bold leading-none tracking-tight text-ink">{value}</div>
      {sub && <div className="mt-1.5 text-[12px] text-mute">{sub}</div>}
    </Card>
  )
}

export function Progress({ value, label }: { value: number | null; label?: string }) {
  return (
    <div>
      <div className="h-1.5 w-full overflow-hidden rounded-full bg-line">
        {value === null ? (
          <div className="indeterminate h-full w-2/5 rounded-full bg-brand" />
        ) : (
          <div className="h-full rounded-full bg-brand transition-all duration-300" style={{ width: `${Math.max(3, value * 100)}%` }} />
        )}
      </div>
      {label && <div className="mt-1.5 text-[12px] text-mute">{label}</div>}
    </div>
  )
}

export function Switch({ on, onChange, disabled }: { on: boolean; onChange: (v: boolean) => void; disabled?: boolean }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      disabled={disabled}
      onClick={() => onChange(!on)}
      className={cx('relative h-6 w-11 shrink-0 rounded-full transition-colors duration-200 disabled:opacity-50', on ? 'bg-brand' : 'bg-line-2')}
    >
      <span className={cx('absolute top-0.5 size-5 rounded-full bg-white shadow transition-all duration-200', on ? 'left-[22px]' : 'left-0.5')} />
    </button>
  )
}

export function Spinner({ className }: { className?: string }) {
  return <Loader2 className={cx('size-4 animate-spin text-mute', className)} />
}

export function Empty({ icon, title, text, action }: { icon: ReactNode; title: string; text: string; action?: ReactNode }) {
  return (
    <div className="flex flex-col items-center justify-center rounded-xl border border-dashed border-line-2 px-6 py-14 text-center">
      <div className="mb-3 grid size-12 place-items-center rounded-2xl bg-card-2 text-mute">{icon}</div>
      <div className="text-[15px] font-semibold text-ink">{title}</div>
      <div className="mt-1 max-w-sm text-[13px] text-mute">{text}</div>
      {action && <div className="mt-4">{action}</div>}
    </div>
  )
}

export function Skeleton({ className }: { className?: string }) {
  return <div className={cx('skeleton', className)} />
}

export function PageHeader({ title, subtitle, actions }: { title: string; subtitle?: ReactNode; actions?: ReactNode }) {
  return (
    <div className="mb-6 flex items-end justify-between gap-4">
      <div>
        <h1 className="text-[26px] font-bold tracking-tight text-ink">{title}</h1>
        {subtitle && <div className="mt-1 text-[13.5px] text-mute">{subtitle}</div>}
      </div>
      {actions && <div className="flex items-center gap-2">{actions}</div>}
    </div>
  )
}

export function Toast({ message, ok, onClose }: { message: string; ok: boolean; onClose: () => void }) {
  useEffect(() => {
    const t = setTimeout(onClose, 6000)
    return () => clearTimeout(t)
  }, [message, onClose])
  return (
    <div className="fade-in fixed bottom-5 right-5 z-50 max-w-sm rounded-xl border border-line-2 bg-card-2 px-4 py-3 text-[13px] shadow-2xl shadow-black/40">
      <div className="flex items-start gap-2.5">
        <span className={cx('mt-1 size-2 shrink-0 rounded-full', ok ? 'bg-brand' : 'bg-rose')} />
        <span className="text-ink">{message}</span>
      </div>
    </div>
  )
}

const palette = ['#00d6a8', '#6ea0ff', '#ffc452', '#f05260', '#b388ff', '#4dd0e1']
/** Game icon with a tidy lettered fallback. */
export function GameIcon({ src, title, size = 40 }: { src?: string; title: string; size?: number }) {
  const r = Math.round(size * 0.26)
  if (src) return <img src={src} alt="" style={{ width: size, height: size, borderRadius: r }} className="shrink-0 object-cover ring-1 ring-line" />
  const c = palette[title.length % palette.length]
  const letters = title.replace(/[^A-Za-z0-9 ]/g, '').split(' ').filter(Boolean).slice(0, 2).map((w) => w[0]).join('').toUpperCase() || '?'
  return (
    <div style={{ width: size, height: size, borderRadius: r, background: `linear-gradient(135deg, ${c}33, ${c}11)`, color: c, fontSize: size * 0.36 }} className="grid shrink-0 place-items-center font-bold ring-1 ring-line">
      {letters}
    </div>
  )
}
