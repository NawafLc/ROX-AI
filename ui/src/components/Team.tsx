import { Brain, Crown, Hammer, Palette, PencilRuler, Radar, ShieldCheck } from 'lucide-react'
import { Agent } from '../lib/api'
import { cx } from './ui'

const ICONS: Record<string, typeof Brain> = { lead: Crown, scout: Radar, researcher: Brain, designer: PencilRuler, builder: Hammer, artist: Palette, tester: ShieldCheck }
const STATE = {
  working: { dot: 'bg-brand pulse-dot text-brand', label: 'Working', text: 'text-brand' },
  idle: { dot: 'bg-sky', label: 'Ready', text: 'text-sky' },
  paused: { dot: 'bg-amber', label: 'Paused', text: 'text-amber' },
  soon: { dot: 'bg-line-2', label: 'Coming soon', text: 'text-mute' },
}

export function AgentAvatar({ id, state, size = 40 }: { id: string; state: Agent['state']; size?: number }) {
  const Icon = ICONS[id] ?? Brain
  return (
    <div
      style={{ width: size, height: size }}
      className={cx(
        'relative grid shrink-0 place-items-center rounded-xl',
        state === 'working' ? 'bg-brand/15 text-brand' : state === 'soon' ? 'bg-card-2 text-mute' : 'bg-sky/12 text-sky',
      )}
    >
      <Icon style={{ width: size * 0.45, height: size * 0.45 }} />
      <span className={cx('absolute -bottom-0.5 -right-0.5 size-3 rounded-full ring-[3px] ring-card', STATE[state].dot)} />
    </div>
  )
}

/** One team member as a compact row: avatar, name, what they're doing. */
export function AgentRow({ a }: { a: Agent }) {
  return (
    <div className={cx('flex items-center gap-3 py-2.5', a.state === 'soon' && 'opacity-60')}>
      <AgentAvatar id={a.id} state={a.state} size={36} />
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span className="text-[13.5px] font-semibold">{a.name}</span>
          <span className={cx('text-[11px] font-semibold', STATE[a.state].text)}>{STATE[a.state].label}</span>
        </div>
        <div className="truncate text-[12px] text-mute">{a.state === 'soon' ? a.role : a.text}</div>
      </div>
    </div>
  )
}

export function AgentCard({ a }: { a: Agent }) {
  return (
    <div className={cx('rounded-xl border border-line bg-card p-4', a.state === 'working' && 'border-brand/40 ring-1 ring-brand/20')}>
      <div className="flex items-start gap-3">
        <AgentAvatar id={a.id} state={a.state} size={44} />
        <div className="min-w-0 flex-1">
          <div className="text-[14.5px] font-semibold">{a.name}</div>
          <div className="text-[12px] text-mute">{a.role}</div>
        </div>
        <span className="rounded-md bg-card-2 px-1.5 py-0.5 text-[10.5px] font-semibold text-ink-2">{a.model}</span>
      </div>
      <div className="mt-3.5 flex items-center gap-2 rounded-lg bg-card-2/60 px-3 py-2 text-[12.5px]">
        <span className={cx('size-2 shrink-0 rounded-full', STATE[a.state].dot)} />
        <span className={cx('truncate', a.state === 'soon' ? 'text-mute' : 'text-ink-2')}>{a.text}</span>
      </div>
    </div>
  )
}

export const stateLabel = (s: Agent['state']) => STATE[s].label
