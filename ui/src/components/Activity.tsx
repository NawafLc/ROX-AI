import { AlertTriangle, Brain, CheckCircle2, Eye, RefreshCw, Square } from 'lucide-react'
import { useState } from 'react'
import { Status, call } from '../lib/api'
import { Badge, Button, Card, Progress, cx } from './ui'

export function ModeSwitch({ mode, onChange }: { mode: string; onChange: () => void }) {
  const [m, setM] = useState(mode)
  const set = (v: string) => {
    setM(v)
    call('settings_set', { patch: { mode: v } }).then(onChange)
  }
  const opt = (v: string, label: string, Icon: typeof Eye, tip: string) => (
    <button
      title={tip}
      onClick={() => set(v)}
      className={cx('flex h-8 items-center gap-1.5 rounded-lg px-3 text-[12.5px] font-semibold transition-all', m === v ? 'bg-card-2 text-ink shadow-sm ring-1 ring-line-2' : 'text-mute hover:text-ink-2')}
    >
      <Icon className={cx('size-3.5', m === v && v === 'active' && 'text-brand')} />
      {label}
    </button>
  )
  return (
    <div className="flex rounded-[10px] border border-line bg-card p-1">
      {opt('passive', 'Passive', Eye, 'Only follows the charts and data')}
      {opt('active', 'Active', Brain, 'Also learns each game from YouTube videos')}
    </div>
  )
}

export function Hero({ status, onChange }: { status: Status; onChange: () => void }) {
  const a = status.activity
  const tone = a.kind === 'attention' ? 'amber' : a.kind === 'idle' ? 'brand' : 'sky'
  const colors = { amber: 'text-amber bg-amber/12', brand: 'text-brand bg-brand/12', sky: 'text-sky bg-sky/12' }
  const Icon = a.kind === 'attention' ? AlertTriangle : a.kind === 'learning' ? Brain : a.kind === 'scanning' ? RefreshCw : CheckCircle2
  const working = a.kind === 'learning' || a.kind === 'scanning'
  return (
    <Card className="relative overflow-hidden p-6">
      <div className={cx('pointer-events-none absolute -right-24 -top-24 size-72 rounded-full blur-3xl', working ? 'bg-sky/10' : a.kind === 'attention' ? 'bg-amber/10' : 'bg-brand/8')} />
      <div className="relative flex items-center gap-5">
        <div className={cx('grid size-14 shrink-0 place-items-center rounded-2xl', colors[tone])}>
          <Icon className={cx('size-7', a.kind === 'scanning' && 'animate-spin [animation-duration:2.5s]')} />
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2.5">
            <div className="text-[20px] font-bold tracking-tight">{a.title}</div>
            {working && <Badge tone="sky">Working</Badge>}
            {!status.auto_run && a.kind === 'idle' && <Badge tone="amber">Automatic mode off</Badge>}
          </div>
          <div className="mt-0.5 text-[13.5px] text-ink-2">{a.detail}</div>
          {working && (
            <div className="mt-3 max-w-md">
              <Progress value={null} />
            </div>
          )}
        </div>
        <div className="flex shrink-0 gap-2">
          {working && (
            <Button kind="secondary" icon={<Square className="size-3.5 fill-current" />} onClick={() => call('start', { what: 'stop' }).then(onChange)}>
              Stop
            </Button>
          )}
          {a.kind === 'attention' && (
            <Button kind="warn" onClick={() => call('unblock').then(onChange)}>
              I fixed it
            </Button>
          )}
          {a.kind === 'idle' && (
            <Button icon={<RefreshCw className="size-4" />} onClick={() => call('start', { what: 'scan' }).then(onChange)}>
              Update charts
            </Button>
          )}
        </div>
      </div>
    </Card>
  )
}
