import { AlertTriangle, Coffee, Crown, Gamepad2, Loader2, Pause, Play, Send, Settings2, Users } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { Agent, ChatMsg, Team as TeamT, call, useApi } from '../lib/api'
import { Badge, Button, Card, PageHeader, Progress, Skeleton, cx } from '../components/ui'
import { AgentAvatar, stateLabel } from '../components/Team'
import { go } from '../App'

const MODELS = [
  ['opus', 'Opus', 'Best quality'],
  ['sonnet', 'Sonnet', 'Balanced'],
  ['haiku', 'Haiku', 'Uses least'],
]

/** The AI team: start/pause building, the game in progress, chat with the Lead, and every member. */
export default function Team() {
  const { data: t, reload } = useApi<TeamT>('team', {}, 2500)
  const [busy, setBusy] = useState(false)
  if (!t)
    return (
      <>
        <PageHeader title="Team" />
        <div className="grid grid-cols-3 gap-4">{[...Array(6)].map((_, i) => <Skeleton key={i} className="h-32" />)}</div>
      </>
    )

  const toggle = () => {
    setBusy(true)
    call(t.enabled ? 'team_pause' : 'team_start').finally(() => {
      setBusy(false)
      reload()
    })
  }
  const active = t.agents.filter((a) => a.active)
  const later = t.agents.filter((a) => !a.active)
  const working = active.filter((a) => a.state === 'working').length

  return (
    <>
      <PageHeader
        title="Team"
        subtitle={
          t.enabled
            ? working
              ? `${working} ${working === 1 ? 'member is' : 'members are'} working right now.`
              : t.paused_until
                ? 'Taking a short break: Claude usage limit. It continues by itself.'
                : 'The team is on. It builds your games in the background.'
            : 'You talk to the Lead. The team builds games on its own once you press Start.'
        }
        actions={
          <Button kind={t.enabled ? 'secondary' : 'primary'} loading={busy} disabled={!t.claude_ready} icon={t.enabled ? <Pause className="size-4" /> : <Play className="size-4" />} onClick={toggle}>
            {t.enabled ? 'Pause team' : 'Start building'}
          </Button>
        }
      />

      {!t.claude_ready && (
        <Notice icon={<AlertTriangle className="size-4" />} text="Claude Code isn't set up yet. Open Settings → Run setup, then come back." action={['Open settings', () => go('settings')]} />
      )}
      {t.claude_ready && t.blocked && (
        <Notice
          icon={<AlertTriangle className="size-4" />}
          text={`${t.blocked}. Sign in once and the team carries on by itself.`}
          action={['Sign in', () => call('dep_act', { id: 'claude_login', action: 'signin' }).then(() => call('unblock'))]}
        />
      )}
      {t.claude_ready && !t.blocked && !t.roblox_connected && (
        <Notice
          icon={<AlertTriangle className="size-4" />}
          text="Roblox isn't connected, so the team can only test on this PC. Connect your test place to test every game on Roblox's servers."
          action={['Connect Roblox', () => go('settings')]}
        />
      )}

      <div className="grid grid-cols-5 gap-5">
        <div className="col-span-3 space-y-5">
          <Building t={t} />
          <Chat />
        </div>
        <div className="col-span-2 space-y-5">
          <Setup t={t} onChange={reload} />
        </div>
      </div>

      <div className="mb-3 mt-7 flex items-center gap-2 text-[15px] font-semibold">
        <Users className="size-4 text-mute" /> Members
      </div>
      <div className="grid grid-cols-3 gap-4">
        {active.map((a) => (
          <Member key={a.id} a={a} onChange={reload} />
        ))}
      </div>
      {later.length > 0 && (
        <>
          <div className="mb-2 mt-6 text-[12px] font-semibold uppercase tracking-wider text-mute">Joining in later updates</div>
          <div className="flex flex-wrap gap-2">
            {later.map((a) => (
              <span key={a.id} className="flex items-center gap-2 rounded-full border border-line bg-card px-3 py-1.5 text-[12.5px] text-mute">
                <AgentAvatar id={a.id} state="soon" size={22} />
                {a.name}
              </span>
            ))}
          </div>
        </>
      )}
    </>
  )
}

function Notice({ icon, text, action }: { icon: React.ReactNode; text: string; action: [string, () => void] }) {
  return (
    <div className="fade-in mb-5 flex items-center gap-3 rounded-xl border border-amber/30 bg-amber/8 px-4 py-3 text-[13px] text-ink-2">
      <span className="text-amber">{icon}</span>
      <span className="flex-1">{text}</span>
      <Button size="sm" onClick={action[1]}>
        {action[0]}
      </Button>
    </div>
  )
}

function Building({ t }: { t: TeamT }) {
  const g = t.current
  if (!g)
    return (
      <Card className="relative overflow-hidden p-5">
        <div className="pointer-events-none absolute -right-16 -top-20 size-56 rounded-full bg-brand/10 blur-3xl" />
        <div className="relative flex items-center gap-4">
          <div className="grid size-12 place-items-center rounded-xl bg-card-2 text-mute">
            <Gamepad2 className="size-6" />
          </div>
          <div className="flex-1">
            <div className="text-[14.5px] font-semibold">{t.ready >= t.games_to_build && t.ready > 0 ? 'All your games are done' : 'No game in progress'}</div>
            <div className="text-[12.5px] text-mute">
              {t.enabled
                ? t.running
                  ? 'The Lead is picking the next game from the market.'
                  : t.ready >= t.games_to_build
                    ? 'Raise "Games to build" to make more.'
                    : 'Starting up…'
                : 'Press Start building: the Lead picks a game from the market and the team builds it.'}
            </div>
          </div>
        </div>
      </Card>
    )
  return (
    <Card className="p-5">
      <div className="flex items-start gap-4">
        <div className="grid size-12 shrink-0 place-items-center rounded-xl bg-gradient-to-br from-brand/25 to-sky/10 text-brand ring-1 ring-brand/30">
          <Gamepad2 className="size-6" />
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <span className="truncate text-[16px] font-semibold">{g.title}</span>
            <Badge tone="sky">{g.genre || 'New game'}</Badge>
            <Badge>{g.size}</Badge>
          </div>
          <div className="mt-1 text-[12.5px] leading-relaxed text-ink-2">{g.pitch}</div>
        </div>
      </div>
      <div className="mt-4">
        <Progress value={g.progress} label={g.stage_text} />
      </div>
      {g.doing.length > 0 && (
        <div className="mt-4 space-y-1.5">
          {g.doing.map((d) => (
            <div key={d.id} className="flex items-center gap-2.5 rounded-lg bg-brand/6 px-3 py-2 text-[12.5px]">
              <AgentAvatar id={d.owner} state="working" size={24} />
              <span className="font-medium text-ink">{t.agents.find((a) => a.id === d.owner)?.name ?? d.owner}</span>
              <span className="truncate text-ink-2">{d.title}</span>
            </div>
          ))}
        </div>
      )}
      {g.activity.length > 0 && (
        <div className="mt-4 border-t border-line pt-3">
          <div className="mb-1.5 text-[11.5px] font-semibold uppercase tracking-wider text-mute">Latest</div>
          <div className="space-y-1">
            {g.activity.slice(0, 6).map((e, i) => (
              <div key={i} className="flex gap-2 text-[12.5px]">
                <span className="w-24 shrink-0 truncate font-medium text-ink-2">{t.agents.find((a) => a.id === e.by)?.name ?? (e.by === 'studio' ? 'Studio' : e.by === 'owner' ? 'You' : e.by)}</span>
                <span className="truncate text-mute">{e.text}</span>
              </div>
            ))}
          </div>
        </div>
      )}
    </Card>
  )
}

function Chat() {
  const { data, reload } = useApi<{ messages: ChatMsg[]; typing: boolean }>('chat', {}, 2000)
  const [text, setText] = useState('')
  const end = useRef<HTMLDivElement>(null)
  const msgs = data?.messages ?? []
  useEffect(() => {
    end.current?.scrollIntoView({ block: 'nearest' })
  }, [msgs.length, data?.typing])
  const send = () => {
    const v = text.trim()
    if (!v) return
    setText('')
    call('chat_send', { text: v }).then(reload)
  }
  return (
    <Card className="flex flex-col">
      <div className="flex items-center gap-3 border-b border-line px-5 py-3.5">
        <div className="grid size-9 place-items-center rounded-lg bg-brand/12 text-brand">
          <Crown className="size-[18px]" />
        </div>
        <div>
          <div className="text-[14px] font-semibold">Chat with the Lead</div>
          <div className="text-[12px] text-mute">Ideas, direction, feedback. The Lead acts on it and tells you what it did.</div>
        </div>
      </div>
      <div className="max-h-[360px] min-h-[140px] space-y-3 overflow-y-auto px-5 py-4">
        {msgs.length === 0 && !data?.typing && (
          <div className="py-6 text-center text-[12.5px] text-mute">Say hi, or try “Make me a cozy fishing game” or “Build 3 games, one at a time”.</div>
        )}
        {msgs.map((m, i) => (
          <div key={i} className={cx('flex', m.from === 'owner' ? 'justify-end' : 'justify-start')}>
            <div
              className={cx(
                'selectable max-w-[85%] whitespace-pre-wrap rounded-2xl px-3.5 py-2 text-[13px] leading-relaxed',
                m.from === 'owner' ? 'rounded-br-md bg-brand/15 text-ink' : 'rounded-bl-md bg-card-2 text-ink-2',
              )}
            >
              {m.text}
            </div>
          </div>
        ))}
        {data?.typing && (
          <div className="flex items-center gap-2 text-[12.5px] text-mute">
            <Loader2 className="size-3.5 animate-spin" /> The Lead is writing…
          </div>
        )}
        <div ref={end} />
      </div>
      <div className="flex items-center gap-2 border-t border-line p-3">
        <input
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && send()}
          placeholder="Message the Lead…"
          className="selectable h-10 flex-1 rounded-[10px] border border-line bg-bg px-3.5 text-[13.5px] outline-none placeholder:text-mute focus:border-brand/60"
        />
        <Button kind="primary" disabled={!text.trim()} icon={<Send className="size-4" />} onClick={send}>
          Send
        </Button>
      </div>
    </Card>
  )
}

function Setup({ t, onChange }: { t: TeamT; onChange: () => void }) {
  const save = (patch: Record<string, unknown>) => call('settings_set', { patch }).then(onChange)
  return (
    <Card className="divide-y divide-line">
      <div className="flex items-center gap-2 px-4 py-3 text-[14px] font-semibold">
        <Settings2 className="size-4 text-mute" /> How the team works
      </div>
      <div className="flex items-center gap-4 px-4 py-3.5">
        <div className="flex-1">
          <div className="text-[13.5px] font-medium">Games to build</div>
          <div className="text-[12.5px] text-mute">Built one at a time. {t.ready} done so far.</div>
        </div>
        <Stepper value={t.games_to_build} min={1} max={50} onChange={(v) => save({ games_to_build: v })} />
      </div>
      <div className="px-4 py-3.5">
        <div className="text-[13.5px] font-medium">Biggest game size</div>
        <div className="mb-2.5 text-[12.5px] text-mute">The Lead picks the size per idea, up to this.</div>
        <Segmented
          value={t.max_game_size}
          options={[
            ['small', 'Small'],
            ['medium', 'Medium'],
            ['large', 'Large · off for now'],
          ]}
          disabled={['large']}
          onChange={(v) => save({ max_game_size: v })}
        />
      </div>
      <div className="px-4 py-3.5">
        <div className="text-[13.5px] font-medium">Pace</div>
        <div className="mb-2.5 text-[12.5px] text-mute">Several at once is faster but uses your Claude plan quicker.</div>
        <Segmented
          value={t.parallel ? 'all' : 'one'}
          options={[
            ['one', 'One at a time'],
            ['all', 'All at once'],
          ]}
          onChange={(v) => save({ team_parallel: v === 'all' })}
        />
      </div>
      {t.paused_until && (
        <div className="flex items-center gap-2 px-4 py-3 text-[12.5px] text-amber">
          <Coffee className="size-4" /> On a break until {new Date(t.paused_until).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}
        </div>
      )}
    </Card>
  )
}

function Member({ a, onChange }: { a: Agent; onChange: () => void }) {
  const setModel = (m: string) =>
    call<{ agent_models: Record<string, string> }>('settings_get').then((s) => {
      const map = { ...(s.agent_models ?? {}) }
      if (m === a.default_model) delete map[a.id]
      else map[a.id] = m
      return call('settings_set', { patch: { agent_models: map } }).then(onChange)
    })
  return (
    <div className={cx('rounded-xl border border-line bg-card p-4 transition-all', a.state === 'working' && 'border-brand/40 ring-1 ring-brand/20')}>
      <div className="flex items-start gap-3">
        <AgentAvatar id={a.id} state={a.state} size={44} />
        <div className="min-w-0 flex-1">
          <div className="text-[14.5px] font-semibold">{a.name}</div>
          <div className="text-[12px] text-mute">{a.role}</div>
        </div>
        <select
          value={a.model_id ?? ''}
          onChange={(e) => setModel(e.target.value)}
          title={`Which Claude model this member uses (default: ${a.default_model})`}
          className="h-7 rounded-md border border-line bg-card-2 px-1.5 text-[11.5px] font-semibold text-ink-2 outline-none hover:border-line-2 focus:border-brand/60"
        >
          {MODELS.map(([v, l, d]) => (
            <option key={v} value={v} title={d}>
              {l}
            </option>
          ))}
        </select>
      </div>
      <div className="mt-3.5 flex items-center gap-2 rounded-lg bg-card-2/60 px-3 py-2 text-[12.5px]">
        <span className={cx('size-2 shrink-0 rounded-full', a.state === 'working' ? 'pulse-dot bg-brand' : a.state === 'paused' ? 'bg-amber' : 'bg-sky')} />
        <span className="shrink-0 text-[11px] font-semibold text-mute">{stateLabel(a.state)}</span>
        <span className="truncate text-ink-2">{a.text}</span>
      </div>
    </div>
  )
}

function Stepper({ value, min, max, onChange }: { value: number; min: number; max: number; onChange: (v: number) => void }) {
  const [v, setV] = useState(value)
  useEffect(() => {
    setV(value)
  }, [value])
  const step = (d: number) => {
    const n = Math.min(max, Math.max(min, v + d))
    setV(n)
    onChange(n)
  }
  return (
    <div className="flex items-center gap-1 rounded-[10px] border border-line bg-bg p-1">
      <button onClick={() => step(-1)} className="grid size-7 place-items-center rounded-md text-ink-2 hover:bg-card-2">
        –
      </button>
      <span className="min-w-[36px] text-center text-[13px] font-semibold tabular-nums">{v}</span>
      <button onClick={() => step(1)} className="grid size-7 place-items-center rounded-md text-ink-2 hover:bg-card-2">
        +
      </button>
    </div>
  )
}

function Segmented({ value, options, onChange, disabled = [] }: { value: string; options: string[][]; onChange: (v: string) => void; disabled?: string[] }) {
  return (
    <div className="flex rounded-[10px] border border-line bg-bg p-1">
      {options.map(([v, l]) => (
        <button
          key={v}
          disabled={disabled.includes(v)}
          onClick={() => onChange(v)}
          className={cx(
            'h-8 flex-1 rounded-md px-2 text-[12.5px] font-medium transition-all disabled:cursor-not-allowed disabled:opacity-40',
            value === v ? 'bg-card-2 text-ink shadow-sm ring-1 ring-line-2' : 'text-mute hover:text-ink-2',
          )}
        >
          {l}
        </button>
      ))}
    </div>
  )
}
