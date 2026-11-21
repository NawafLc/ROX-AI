import { Clock, Coffee } from 'lucide-react'
import { Eta } from '../lib/api'
import { cx } from './ui'

/** "6 h 25 min", "40 min" */
export function humanMinutes(m: number) {
  const n = Math.max(1, Math.round(m))
  if (n < 60) return `${n} min`
  const h = Math.floor(n / 60)
  const r = Math.round((n % 60) / 5) * 5
  return r === 0 || r === 60 ? `${r === 60 ? h + 1 : h} h` : `${h} h ${r} min`
}

/** "around 7:10 PM", "tomorrow around 2:10 AM", "Mon around 9:00 AM" */
export function finishText(iso: string) {
  const d = new Date(iso)
  if (isNaN(d.getTime())) return ''
  const time = d.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' })
  const day = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime()
  const diff = Math.round((day(d) - day(new Date())) / 86400000)
  if (diff <= 0) return `today around ${time}`
  if (diff === 1) return `tomorrow around ${time}`
  return `${d.toLocaleDateString([], { weekday: 'short' })} around ${time}`
}

/**
 * The game's progress: a bar with the percentage, what stage it is in, and how long until it's done.
 * `progress` is 0–100. `paused` = the team isn't working right now (the time left then doesn't count down).
 */
export function BuildProgress({ progress, label, eta, paused, size = 'md' }: { progress: number; label?: string; eta?: Eta; paused?: string | null; size?: 'sm' | 'md' }) {
  const pct = Math.max(0, Math.min(100, Math.round(progress)))
  paused = paused ?? eta?.paused ?? null
  return (
    <div>
      <div className="mb-1.5 flex items-baseline justify-between gap-3">
        <span className={cx('truncate text-ink-2', size === 'sm' ? 'text-[12px]' : 'text-[13px]')}>{label}</span>
        <span className={cx('shrink-0 font-semibold tabular-nums text-ink', size === 'sm' ? 'text-[12px]' : 'text-[14px]')}>{pct}%</span>
      </div>
      <div className={cx('w-full overflow-hidden rounded-full bg-line', size === 'sm' ? 'h-1.5' : 'h-2.5')}>
        <div
          className={cx('relative h-full overflow-hidden rounded-full transition-all duration-500', paused ? 'bg-mute' : 'bg-gradient-to-r from-brand to-sky')}
          style={{ width: `${Math.max(3, pct)}%` }}
        >
          {!paused && <div className="shimmer absolute inset-0" />}
        </div>
      </div>
      {eta && (
        <div className={cx('mt-2 flex flex-wrap items-center gap-x-3 gap-y-1 text-mute', size === 'sm' ? 'text-[11.5px]' : 'text-[12.5px]')}>
          <span className="inline-flex items-center gap-1.5">
            <Clock className="size-3.5" />
            {paused ? (
              <span>
                About <b className="font-semibold text-ink-2">{humanMinutes(eta.minutes)}</b> of work left · {paused}
              </span>
            ) : (
              <span>
                About <b className="font-semibold text-ink-2">{humanMinutes(eta.minutes + eta.break_minutes)}</b> left · done {finishText(eta.finish_at)}
              </span>
            )}
          </span>
          {!paused && eta.break_minutes > 0 && (
            <span className="inline-flex items-center gap-1.5 text-amber">
              <Coffee className="size-3.5" />
              includes a {humanMinutes(eta.break_minutes)} Claude break
            </span>
          )}
        </div>
      )}
    </div>
  )
}
