import { Brain, Eye, FolderOpen, Loader2, RefreshCw, RotateCcw, ScrollText, Trash2 } from 'lucide-react'
import { ReactNode, useEffect, useRef, useState } from 'react'
import { Status, call, useApi } from '../lib/api'
import { Button, Card, PageHeader, Switch, cx } from '../components/ui'
import RobloxConnection from '../components/RobloxConnection'

type S = Record<string, any>
const CHARTS = [
  ['top-trending', 'Top Trending'],
  ['top-playing-now', 'Top Playing Now'],
  ['up-and-coming', 'Up-and-Coming'],
  ['fun-with-friends', 'Fun with Friends'],
  ['top-revisited', 'Top Revisited'],
]
const MODELS = [
  ['', 'Default (recommended)'],
  ['claude-opus-5-5', 'Opus 5.5 — best quality'],
  ['claude-sonnet-5-5', 'Sonnet 5.5 — balanced'],
  ['claude-haiku-5-5', 'Haiku 5.5 — uses least'],
]

export default function SettingsPage({ status, onChange, onCheckUpdates }: { status: Status; onChange: () => void; onCheckUpdates: () => void }) {
  const { data, reload } = useApi<S>('settings_get')
  const [s, setS] = useState<S | null>(null)
  const [saved, setSaved] = useState(false)
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined)
  useEffect(() => {
    if (data && !s) setS(data)
  }, [data, s])

  const set = (patch: S, debounce = false) => {
    setS((o) => ({ ...o, ...patch }))
    const save = () =>
      call('settings_set', { patch }).then(() => {
        setSaved(true)
        setTimeout(() => setSaved(false), 1500)
        onChange()
        reload()
      })
    clearTimeout(timer.current)
    if (debounce) timer.current = setTimeout(save, 600)
    else save()
  }

  if (!s)
    return (
      <div className="flex items-center gap-2 text-mute">
        <Loader2 className="size-4 animate-spin" /> Loading settings…
      </div>
    )
  const active = s.mode === 'active'

  return (
    <>
      <PageHeader title="Settings" subtitle="Changes save on their own." actions={saved && <span className="fade-in text-[12.5px] font-medium text-brand">Saved</span>} />
      <div className="max-w-3xl space-y-5">
        <Section title="How ROX-AI works">
          <div className="grid grid-cols-2 gap-3 p-4">
            <ModeCard on={!active} onClick={() => set({ mode: 'passive' })} icon={<Eye className="size-5" />} title="Passive" text="Follows the charts and game data. Uses no Claude." />
            <ModeCard on={active} onClick={() => set({ mode: 'active' })} icon={<Brain className="size-5" />} title="Active" text="Also learns every game from YouTube videos with Claude." />
          </div>
          <Row title="Automatic" text="Run by itself in the background, even when this window is closed.">
            <Switch on={s.auto_run} onChange={(v) => set({ auto_run: v })} />
          </Row>
        </Section>

        <Section title="Learning" muted={!active}>
          <Row title="Games per day" text="How many games to learn each day.">
            <Num value={s.learn_per_run} min={1} max={25} onChange={(v) => set({ learn_per_run: v }, true)} />
          </Row>
          <Row title="Videos per game" text="More videos give better notes but take longer.">
            <Num value={s.videos_per_game} min={1} max={10} onChange={(v) => set({ videos_per_game: v }, true)} />
          </Row>
          <Row title="Relearn after" text="Learn a game again after this many days.">
            <Num value={s.reanalyze_after_days} min={3} max={180} suffix="days" onChange={(v) => set({ reanalyze_after_days: v }, true)} />
          </Row>
          <Row title="Claude model" text="Which Claude model learns the games.">
            <Select value={s.claude_model} options={MODELS} onChange={(v) => set({ claude_model: v })} />
          </Row>
        </Section>

        <Section title="Charts">
          <Row title="Chart to follow" text="The Roblox chart ROX-AI studies every day.">
            <Select value={s.sort_id} options={CHARTS} onChange={(v) => set({ sort_id: v })} />
          </Row>
          <Row title="Games per chart" text="How far down the chart to go.">
            <Num value={s.chart_limit} min={5} max={100} onChange={(v) => set({ chart_limit: v }, true)} />
          </Row>
        </Section>

        <Section title="Game ideas">
          <div className="p-4">
            <div className="text-[13.5px] font-medium">Things to avoid</div>
            <div className="mb-2.5 text-[12.5px] text-mute">Genres or themes your team should never make. One per line or separated by commas.</div>
            <textarea
              value={s.avoid ?? ''}
              onChange={(e) => set({ avoid: e.target.value }, true)}
              placeholder="e.g. horror, gambling-style games"
              rows={3}
              className="selectable w-full resize-none rounded-[10px] border border-line bg-bg px-3 py-2.5 text-[13.5px] outline-none placeholder:text-mute focus:border-brand/60"
            />
          </div>
        </Section>

        <div id="roblox">
          <div className="mb-2 px-1 text-[12px] font-semibold uppercase tracking-wider text-mute">Game building</div>
          <RobloxConnection />
        </div>

        <Section title="App">
          <Row title="Updates" text={status.update.state === 'available' ? `Version ${status.update.latest} is ready to install.` : `You're on version ${status.version}.`}>
            {status.update.state === 'available' ? (
              <Button kind="primary" size="sm" onClick={() => call('update_run')}>
                Install update
              </Button>
            ) : (
              <Button size="sm" icon={<RefreshCw className={cx('size-3.5', status.update.state === 'checking' && 'animate-spin')} />} onClick={onCheckUpdates}>
                Check now
              </Button>
            )}
          </Row>
          <Row title="Folders" text="Where ROX-AI keeps its files, and its activity logs.">
            <div className="flex gap-2">
              <Button size="sm" icon={<FolderOpen className="size-3.5" />} onClick={() => call('open', { what: 'folder' })}>
                App folder
              </Button>
              <Button size="sm" icon={<ScrollText className="size-3.5" />} onClick={() => call('open', { what: 'logs' })}>
                Logs
              </Button>
            </div>
          </Row>
          <Row title="Run setup again" text="Go through the first-time setup screens again.">
            <Button size="sm" icon={<RotateCcw className="size-3.5" />} onClick={() => call('restart_setup').then(onChange)}>
              Run setup
            </Button>
          </Row>
          <RemoveRow />
        </Section>
      </div>
    </>
  )
}

function Section({ title, children, muted }: { title: string; children: ReactNode; muted?: boolean }) {
  return (
    <div>
      <div className="mb-2 flex items-center gap-2 px-1 text-[12px] font-semibold uppercase tracking-wider text-mute">
        {title}
        {muted && <span className="normal-case tracking-normal text-mute/80">· used in Active mode</span>}
      </div>
      <Card className={cx('divide-y divide-line', muted && 'opacity-60')}>{children}</Card>
    </div>
  )
}

function Row({ title, text, children }: { title: string; text: string; children: ReactNode }) {
  return (
    <div className="flex items-center gap-6 px-4 py-3.5">
      <div className="min-w-0 flex-1">
        <div className="text-[13.5px] font-medium">{title}</div>
        <div className="text-[12.5px] text-mute">{text}</div>
      </div>
      {children}
    </div>
  )
}

function ModeCard({ on, onClick, icon, title, text }: { on: boolean; onClick: () => void; icon: ReactNode; title: string; text: string }) {
  return (
    <button onClick={onClick} className={cx('flex flex-col rounded-xl border p-4 text-left transition-all', on ? 'border-brand/60 bg-brand/6 ring-1 ring-brand/30' : 'border-line bg-card-2/40 hover:border-line-2')}>
      <div className="flex items-center justify-between">
        <span className={on ? 'text-brand' : 'text-mute'}>{icon}</span>
        <span className={cx('grid size-4 place-items-center rounded-full border-2', on ? 'border-brand' : 'border-line-2')}>{on && <span className="size-1.5 rounded-full bg-brand" />}</span>
      </div>
      <div className="mt-2.5 text-[14px] font-semibold">{title}</div>
      <div className="mt-0.5 text-[12.5px] text-mute">{text}</div>
    </button>
  )
}

function Num({ value, min, max, suffix, onChange }: { value: number; min: number; max: number; suffix?: string; onChange: (v: number) => void }) {
  const v = Number(value) || min
  const step = (d: number) => onChange(Math.min(max, Math.max(min, v + d)))
  return (
    <div className="flex items-center gap-1 rounded-[10px] border border-line bg-bg p-1">
      <button onClick={() => step(-1)} className="grid size-7 place-items-center rounded-md text-ink-2 hover:bg-card-2">
        –
      </button>
      <span className="min-w-[52px] text-center text-[13px] font-semibold tabular-nums">
        {v}
        {suffix && <span className="ml-1 font-normal text-mute">{suffix}</span>}
      </span>
      <button onClick={() => step(1)} className="grid size-7 place-items-center rounded-md text-ink-2 hover:bg-card-2">
        +
      </button>
    </div>
  )
}

function Select({ value, options, onChange }: { value: string; options: string[][]; onChange: (v: string) => void }) {
  return (
    <select value={value ?? ''} onChange={(e) => onChange(e.target.value)} className="h-9 rounded-[10px] border border-line bg-bg px-3 text-[13px] text-ink outline-none focus:border-brand/60">
      {options.map(([v, l]) => (
        <option key={v} value={v}>
          {l}
        </option>
      ))}
    </select>
  )
}

function RemoveRow() {
  const [stage, setStage] = useState<'idle' | 'confirm' | 'done'>('idle')
  const [msg, setMsg] = useState('')
  return (
    <div className="px-4 py-3.5">
      <div className="flex items-center gap-6">
        <div className="min-w-0 flex-1">
          <div className="text-[13.5px] font-medium text-rose">Remove from this PC</div>
          <div className="text-[12.5px] text-mute">Stops the background schedule and Claude connections. Your folder, vault and data stay.</div>
        </div>
        {stage === 'idle' && (
          <Button size="sm" icon={<Trash2 className="size-3.5" />} onClick={() => setStage('confirm')}>
            Remove
          </Button>
        )}
        {stage === 'confirm' && (
          <div className="flex gap-2">
            <Button size="sm" kind="ghost" onClick={() => setStage('idle')}>
              Cancel
            </Button>
            <Button
              size="sm"
              kind="danger"
              onClick={() =>
                call<{ message: string }>('remove_from_pc').then((r) => {
                  setMsg(r.message)
                  setStage('done')
                })
              }
            >
              Yes, remove
            </Button>
          </div>
        )}
      </div>
      {stage === 'done' && <div className="mt-2 whitespace-pre-line text-[12.5px] text-ink-2">{msg}</div>}
    </div>
  )
}
