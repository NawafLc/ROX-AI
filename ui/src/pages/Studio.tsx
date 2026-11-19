import { AlertTriangle, ArrowRight, BookOpen, Brain, CheckCircle2, Clock, Eye, Gamepad2, Play, RefreshCw, Sparkles, Square, TrendingUp } from 'lucide-react'
import { useState } from 'react'
import { GameRow, Status, ago, call, fmt, useApi } from '../lib/api'
import { Badge, Button, Card, GameIcon, PageHeader, Progress, Skeleton, SourceBadge, Stat, cx } from '../components/ui'
import { go } from '../App'

type QueueItem = { universe_id: number; title: string; rank: number; reason: string }

export default function Studio({ status, onChange }: { status: Status; onChange: () => void }) {
  const { data: queue } = useApi<QueueItem[]>('learn_queue', {}, 15000)
  const { data: games } = useApi<GameRow[]>('games', {}, 15000)
  const recent = (games ?? []).filter((g) => g.source).sort((a, b) => (b.last_analysis || '').localeCompare(a.last_analysis || '')).slice(0, 5)
  const active = status.mode === 'active'
  const hour = new Date().getHours()
  const hello = hour < 12 ? 'Good morning' : hour < 18 ? 'Good afternoon' : 'Good evening'

  return (
    <>
      <PageHeader title={hello} subtitle="Here's what your studio is up to." actions={<ModeSwitch mode={status.mode} onChange={onChange} />} />

      <Hero status={status} onChange={onChange} />

      <div className="mt-5 grid grid-cols-4 gap-4">
        <Stat label="Games tracked" value={fmt(status.stats.games)} icon={<Gamepad2 className="size-4" />} sub="From the Roblox charts" />
        <Stat label="Games learned" value={fmt(status.stats.learned)} icon={<Brain className="size-4" />} sub={status.stats.games ? `${Math.round((status.stats.learned / status.stats.games) * 100)}% of tracked` : 'None yet'} />
        <Stat label="On today's chart" value={fmt(status.stats.on_chart)} icon={<TrendingUp className="size-4" />} sub={status.stats.chart_day ? `Chart of ${status.stats.chart_day}` : 'No chart yet'} />
        <Stat label="Last chart update" value={<span className="text-[22px]">{ago(status.stats.last_scan)}</span>} icon={<Clock className="size-4" />} sub="Updates once a day" />
      </div>

      <div className="mt-5 grid grid-cols-5 gap-5">
        <Card className="col-span-3 p-5">
          <div className="mb-3 flex items-center justify-between">
            <div>
              <div className="text-[15px] font-semibold">Up next to learn</div>
              <div className="text-[12.5px] text-mute">{active ? 'Learned from YouTube videos, a few each day.' : 'Turn on Active mode to learn these from videos.'}</div>
            </div>
            {active && (
              <Button size="sm" icon={<Play className="size-3.5" />} disabled={status.busy} onClick={() => call('start', { what: 'learn' }).then(onChange)}>
                Learn now
              </Button>
            )}
          </div>
          {!queue ? (
            <div className="space-y-2">{[0, 1, 2, 3].map((i) => <Skeleton key={i} className="h-12" />)}</div>
          ) : queue.length === 0 ? (
            <div className="flex items-center gap-3 rounded-xl bg-card-2 px-4 py-5 text-[13px] text-ink-2">
              <CheckCircle2 className="size-5 text-brand" /> Every game on today's chart is learned and up to date.
            </div>
          ) : (
            <div className="divide-y divide-line">
              {queue.slice(0, 6).map((q) => (
                <button key={q.universe_id} onClick={() => go('game', String(q.universe_id))} className="flex w-full items-center gap-3 py-2.5 text-left hover:opacity-90">
                  <span className="w-6 text-right text-[12px] tabular-nums text-mute">#{q.rank}</span>
                  <GameIcon uid={q.universe_id} title={q.title} size={34} />
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-[13.5px] font-medium">{q.title}</div>
                    <div className="text-[12px] text-mute">{q.reason}</div>
                  </div>
                  <ArrowRight className="size-4 text-mute" />
                </button>
              ))}
            </div>
          )}
        </Card>

        <Card className="col-span-2 p-5">
          <div className="mb-3 flex items-center justify-between">
            <div className="text-[15px] font-semibold">Recently learned</div>
            <button onClick={() => go('research')} className="text-[12.5px] font-medium text-brand hover:underline">
              See all
            </button>
          </div>
          {!games ? (
            <div className="space-y-2">{[0, 1, 2].map((i) => <Skeleton key={i} className="h-12" />)}</div>
          ) : recent.length === 0 ? (
            <div className="rounded-xl bg-card-2 px-4 py-5 text-[13px] text-mute">Nothing learned yet. {active ? 'The first games will be learned soon.' : 'Switch to Active mode to start.'}</div>
          ) : (
            <div className="space-y-1">
              {recent.map((g) => (
                <button key={g.uid} onClick={() => go('game', String(g.uid))} className="flex w-full items-center gap-3 rounded-lg px-1 py-2 text-left hover:bg-card-2">
                  <GameIcon uid={g.uid} title={g.title} size={34} />
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-[13.5px] font-medium">{g.title}</div>
                    <div className="truncate text-[12px] text-mute">{g.one_liner || g.genre}</div>
                  </div>
                  <SourceBadge source={g.source} />
                </button>
              ))}
            </div>
          )}
        </Card>
      </div>

      <Card className="mt-5 flex items-center gap-4 p-5">
        <div className="grid size-11 place-items-center rounded-xl bg-sky/12 text-sky">
          <Sparkles className="size-5" />
        </div>
        <div className="flex-1">
          <div className="text-[14px] font-semibold">Your AI game team is coming next</div>
          <div className="text-[12.5px] text-mute">Everything learned here becomes the playbook your team uses to build games. Finished games will appear in My Games, ready to play and publish.</div>
        </div>
        <Button kind="ghost" size="sm" icon={<BookOpen className="size-4" />} onClick={() => go('vault')}>
          Open vault
        </Button>
      </Card>
    </>
  )
}

function ModeSwitch({ mode, onChange }: { mode: string; onChange: () => void }) {
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

function Hero({ status, onChange }: { status: Status; onChange: () => void }) {
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
