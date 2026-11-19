import { Loader2 } from 'lucide-react'
import { ReactNode, useEffect, useState } from 'react'
import { call } from '../lib/api'

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

export function SourceBadge({ source, label }: { source: string; label?: string }) {
  if (source === 'videos') return <Badge tone="brand">{label ?? 'Videos'}</Badge>
  if (source === 'playtest') return <Badge tone="sky">{label ?? 'Playtest'}</Badge>
  if (source === 'data') return <Badge tone="neutral">{label ?? 'Data + AI'}</Badge>
  return <Badge tone="neutral" className="opacity-70">{label ?? 'Data only'}</Badge>
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

const iconCache = new Map<number, string | null>()
const palette = ['#00d6a8', '#6ea0ff', '#ffc452', '#f05260', '#b388ff', '#4dd0e1']
/** Game icon (from the vault) with a tidy lettered fallback. */
export function GameIcon({ uid, title, size = 40 }: { uid: number; title: string; size?: number }) {
  const [src, setSrc] = useState<string | null | undefined>(iconCache.get(uid))
  useEffect(() => {
    if (iconCache.has(uid)) return
    call<{ data: string | null }>('icon', { uid })
      .then((r) => {
        iconCache.set(uid, r.data)
        setSrc(r.data)
      })
      .catch(() => iconCache.set(uid, null))
  }, [uid])
  const r = Math.round(size * 0.26)
  if (src) return <img src={src} alt="" style={{ width: size, height: size, borderRadius: r }} className="shrink-0 object-cover ring-1 ring-line" />
  const c = palette[uid % palette.length]
  const letters = title.replace(/[^A-Za-z0-9 ]/g, '').split(' ').filter(Boolean).slice(0, 2).map((w) => w[0]).join('').toUpperCase() || '?'
  return (
    <div
      style={{ width: size, height: size, borderRadius: r, background: `linear-gradient(135deg, ${c}33, ${c}11)`, color: c, fontSize: size * 0.36 }}
      className="grid shrink-0 place-items-center font-bold ring-1 ring-line"
    >
      {letters}
    </div>
  )
}

export function MomentumBar({ value }: { value: number }) {
  const c = value >= 65 ? 'bg-brand' : value <= 40 ? 'bg-rose' : 'bg-sky'
  return (
    <div className="flex items-center gap-2">
      <div className="h-1.5 w-16 overflow-hidden rounded-full bg-line">
        <div className={cx('h-full rounded-full', c)} style={{ width: `${Math.min(100, Math.max(4, value))}%` }} />
      </div>
      <span className="w-6 text-[12px] tabular-nums text-ink-2">{value}</span>
    </div>
  )
}

export function Sparkline({ values, width = 220, height = 48, invert = false }: { values: number[]; width?: number; height?: number; invert?: boolean }) {
  if (values.length < 2) return <div className="text-[12px] text-mute">Needs a few days of data</div>
  const min = Math.min(...values)
  const max = Math.max(...values)
  const span = max - min || 1
  const pts = values.map((v, i) => {
    const x = (i / (values.length - 1)) * width
    const n = (v - min) / span
    const y = invert ? n * (height - 6) + 3 : height - 3 - n * (height - 6)
    return `${x.toFixed(1)},${y.toFixed(1)}`
  })
  return (
    <svg width={width} height={height} className="overflow-visible">
      <defs>
        <linearGradient id="spark" x1="0" x2="0" y1="0" y2="1">
          <stop offset="0" stopColor="#00d6a8" stopOpacity="0.35" />
          <stop offset="1" stopColor="#00d6a8" stopOpacity="0" />
        </linearGradient>
      </defs>
      <polygon points={`0,${height} ${pts.join(' ')} ${width},${height}`} fill="url(#spark)" />
      <polyline points={pts.join(' ')} fill="none" stroke="#00d6a8" strokeWidth="2" strokeLinejoin="round" strokeLinecap="round" />
    </svg>
  )
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
