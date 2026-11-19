import { ArrowLeft, ArrowRight, Brain, Check, CheckCircle2, Download, Eye, FolderOpen, HardDrive, Loader2, LogIn, Sparkles } from 'lucide-react'
import { ReactNode, useEffect, useState } from 'react'
import { Dep, Status, call, isTauri, useApi } from '../lib/api'
import { Badge, Button, Card, Progress, Switch, cx } from '../components/ui'
import { Logo } from '../components/Logo'

const STEPS = ['Welcome', 'Location', 'Mode', 'Connect', 'Done']

function savedStep() {
  try {
    return Number(localStorage.getItem('setup_step') ?? 0) || 0
  } catch {
    return 0
  }
}

export default function Setup({ status, onDone }: { status: Status; onDone: () => void }) {
  const [step, setStepRaw] = useState(savedStep)
  const setStep = (n: number) => {
    setStepRaw(n)
    try {
      localStorage.setItem('setup_step', String(n))
    } catch {}
  }
  const next = () => setStep(Math.min(step + 1, STEPS.length - 1))
  const back = () => setStep(Math.max(step - 1, 0))

  return (
    <div className="relative flex h-full flex-col items-center overflow-y-auto px-6 py-10">
      <div className="pointer-events-none fixed -top-40 left-1/2 size-[640px] -translate-x-1/2 rounded-full bg-brand/7 blur-3xl" />
      <div className="relative w-full max-w-[640px]">
        <div className="mb-8 flex items-center justify-center gap-2">
          {STEPS.map((s, i) => (
            <div key={s} className="flex items-center gap-2">
              <div
                className={cx(
                  'grid size-6 place-items-center rounded-full text-[11px] font-bold transition-colors',
                  i < step ? 'bg-brand text-[#04140f]' : i === step ? 'bg-card-2 text-ink ring-2 ring-brand' : 'bg-card-2 text-mute',
                )}
              >
                {i < step ? <Check className="size-3.5" /> : i + 1}
              </div>
              <span className={cx('text-[12px] font-medium', i === step ? 'text-ink' : 'text-mute')}>{s}</span>
              {i < STEPS.length - 1 && <div className={cx('mx-1 h-px w-6', i < step ? 'bg-brand' : 'bg-line-2')} />}
            </div>
          ))}
        </div>

        <div key={step} className="fade-in">
          {step === 0 && <Welcome next={next} />}
          {step === 1 && <Location next={next} back={back} />}
          {step === 2 && <Mode status={status} next={next} back={back} onChange={onDone} />}
          {step === 3 && <Connect status={status} next={next} back={back} />}
          {step === 4 && (
            <Finish
              status={status}
              back={back}
              onDone={() => {
                try {
                  localStorage.removeItem('setup_step')
                } catch {}
                call('finish_setup').then(onDone)
              }}
            />
          )}
        </div>
      </div>
    </div>
  )
}

function Shell({ title, text, children, footer }: { title: string; text: string; children?: ReactNode; footer: ReactNode }) {
  return (
    <Card className="p-8 shadow-2xl shadow-black/30">
      <h1 className="text-[24px] font-bold tracking-tight">{title}</h1>
      <p className="mt-1.5 text-[14px] leading-relaxed text-ink-2">{text}</p>
      {children && <div className="mt-6">{children}</div>}
      <div className="mt-8 flex items-center justify-between">{footer}</div>
    </Card>
  )
}

const BackBtn = ({ back }: { back: () => void }) => (
  <Button kind="ghost" icon={<ArrowLeft className="size-4" />} onClick={back}>
    Back
  </Button>
)

function Welcome({ next }: { next: () => void }) {
  const items = [
    ['Studies the Roblox charts', 'Every top game, every day — players, likes, money, trends.'],
    ['Learns how games work', 'Claude watches YouTube videos of each game and writes it down.'],
    ['Builds your knowledge vault', 'A clean note for every game, readable here or in Obsidian.'],
  ]
  return (
    <Card className="p-8 text-center shadow-2xl shadow-black/30">
      <div className="flex justify-center">
        <Logo size={64} />
      </div>
      <h1 className="mt-5 text-[28px] font-bold tracking-tight">Welcome to ROX-AI</h1>
      <p className="mx-auto mt-2 max-w-md text-[14px] text-ink-2">Your AI Roblox game studio. Setup takes about two minutes, and you only do it once.</p>
      <div className="mt-7 space-y-2.5 text-left">
        {items.map(([t, d]) => (
          <div key={t} className="flex items-start gap-3 rounded-xl bg-card-2/60 px-4 py-3">
            <CheckCircle2 className="mt-0.5 size-[18px] shrink-0 text-brand" />
            <div>
              <div className="text-[13.5px] font-semibold">{t}</div>
              <div className="text-[12.5px] text-mute">{d}</div>
            </div>
          </div>
        ))}
      </div>
      <Button kind="primary" size="lg" className="mt-8 w-full" icon={<Sparkles className="size-4" />} onClick={next}>
        Get started
      </Button>
    </Card>
  )
}

function Location({ next, back }: { next: () => void; back: () => void }) {
  const { data } = useApi<{ here: string; temporary: boolean; suggested: string }>('location')
  const [dest, setDest] = useState('')
  const [desktop, setDesktop] = useState(true)
  const [startMenu, setStartMenu] = useState(true)
  const [moving, setMoving] = useState(false)
  const [err, setErr] = useState('')
  useEffect(() => {
    if (data && !dest) setDest(data.temporary ? data.suggested : data.here)
  }, [data, dest])

  const pick = async () => {
    if (!isTauri) return
    const { open } = await import('@tauri-apps/plugin-dialog')
    const r = await open({ directory: true, defaultPath: dest || undefined, title: 'Choose where to keep ROX-AI' })
    if (typeof r === 'string') setDest(r.toLowerCase().endsWith('rox-ai') ? r : r.replace(/[\\/]$/, '') + '\\ROX-AI')
  }
  const go = async () => {
    setErr('')
    if (!data) return
    if (dest === data.here) {
      if (desktop || startMenu) await call('shortcuts', { desktop, start_menu: startMenu }).catch(() => {})
      return next()
    }
    setMoving(true)
    try {
      localStorage.setItem('setup_step', '2')
    } catch {}
    try {
      await call('move_app', { dest, desktop, start_menu: startMenu })
    } catch (e) {
      setMoving(false)
      setErr(String((e as Error).message ?? e))
    }
  }

  return (
    <Shell
      title="Where should ROX-AI live?"
      text="ROX-AI is portable: the app, its data and your vault stay together in one folder. Nothing is installed into Windows."
      footer={
        <>
          <BackBtn back={back} />
          <Button kind="primary" loading={moving} icon={!moving && <ArrowRight className="size-4" />} onClick={go} disabled={!data}>
            {moving ? 'Moving…' : data && dest !== data.here ? 'Move here' : 'Continue'}
          </Button>
        </>
      }
    >
      {data?.temporary && (
        <div className="mb-4 rounded-xl border border-amber/30 bg-amber/8 px-4 py-3 text-[13px] text-ink-2">
          <span className="font-semibold text-amber">Heads up:</span> ROX-AI is running from a temporary place (like Downloads or a zip). Pick a permanent folder so nothing gets lost.
        </div>
      )}
      <div className="text-[12px] font-semibold uppercase tracking-wider text-mute">Folder</div>
      <div className="mt-2 flex gap-2">
        <div className="flex h-11 min-w-0 flex-1 items-center gap-2.5 rounded-[10px] border border-line bg-bg px-3">
          <HardDrive className="size-4 shrink-0 text-mute" />
          <input value={dest} onChange={(e) => setDest(e.target.value)} className="selectable min-w-0 flex-1 bg-transparent text-[13px] outline-none" spellCheck={false} />
        </div>
        {isTauri && (
          <Button icon={<FolderOpen className="size-4" />} onClick={pick} className="h-11">
            Browse
          </Button>
        )}
      </div>
      <div className="mt-5 space-y-3">
        <Toggle on={desktop} onChange={setDesktop} label="Desktop shortcut" />
        <Toggle on={startMenu} onChange={setStartMenu} label="Start menu shortcut" />
      </div>
      {err && <div className="mt-4 text-[12.5px] text-rose">{err}</div>}
    </Shell>
  )
}

function Toggle({ on, onChange, label }: { on: boolean; onChange: (v: boolean) => void; label: string }) {
  return (
    <label className="flex cursor-pointer items-center justify-between rounded-xl bg-card-2/50 px-4 py-3">
      <span className="text-[13.5px]">{label}</span>
      <Switch on={on} onChange={onChange} />
    </label>
  )
}

function Mode({ status, next, back, onChange }: { status: Status; next: () => void; back: () => void; onChange: () => void }) {
  const [m, setM] = useState(status.mode)
  const set = (v: 'passive' | 'active') => {
    setM(v)
    call('settings_set', { patch: { mode: v } }).then(onChange)
  }
  const opt = (v: 'passive' | 'active', icon: ReactNode, title: string, text: string, points: string[]) => (
    <button
      onClick={() => set(v)}
      className={cx('flex flex-col rounded-2xl border p-5 text-left transition-all', m === v ? 'border-brand/60 bg-brand/6 ring-1 ring-brand/30' : 'border-line bg-card-2/40 hover:border-line-2')}
    >
      <div className="flex items-center justify-between">
        <span className={cx('grid size-10 place-items-center rounded-xl', m === v ? 'bg-brand/15 text-brand' : 'bg-card-2 text-mute')}>{icon}</span>
        {v === 'active' && <Badge tone="brand">Recommended</Badge>}
      </div>
      <div className="mt-4 text-[16px] font-semibold">{title}</div>
      <div className="mt-1 text-[12.5px] text-mute">{text}</div>
      <ul className="mt-3 space-y-1.5">
        {points.map((p) => (
          <li key={p} className="flex items-center gap-2 text-[12.5px] text-ink-2">
            <Check className="size-3.5 text-brand" /> {p}
          </li>
        ))}
      </ul>
    </button>
  )
  return (
    <Shell
      title="How much should it do?"
      text="You can change this any time from the Studio page."
      footer={
        <>
          <BackBtn back={back} />
          <Button kind="primary" icon={<ArrowRight className="size-4" />} onClick={next}>
            Continue
          </Button>
        </>
      }
    >
      <div className="grid grid-cols-2 gap-3">
        {opt('passive', <Eye className="size-5" />, 'Passive', 'Just the numbers.', ['Daily chart tracking', 'Trends and genre mix', 'Uses no Claude'])}
        {opt('active', <Brain className="size-5" />, 'Active', 'Numbers plus real understanding.', ['Everything in Passive', 'Learns games from YouTube', 'Detailed game notes'])}
      </div>
    </Shell>
  )
}

function Connect({ status, next, back }: { status: Status; next: () => void; back: () => void }) {
  const { data: deps, reload } = useApi<Dep[]>('deps', {}, 2500)
  useEffect(() => {
    reload()
  }, [status.mode, reload])
  const list = deps ?? []
  const missing = list.filter((d) => !d.ok && d.action)
  const requiredMissing = list.filter((d) => d.required && !d.ok)
  const job = status.job
  const busyIds = new Set([...(job ? [job.id] : []), ...status.queued])

  return (
    <Shell
      title="Connect your apps"
      text={status.mode === 'active' ? 'ROX-AI uses these free tools. Anything missing is downloaded for you — no installers.' : 'Passive mode needs almost nothing. Optional tools are listed below.'}
      footer={
        <>
          <BackBtn back={back} />
          <div className="flex items-center gap-2">
            {requiredMissing.length > 0 && !job && (
              <Button kind="ghost" onClick={next}>
                Skip for now
              </Button>
            )}
            {missing.length > 0 && (
              <Button icon={<Download className="size-4" />} disabled={!!job} onClick={() => call('dep_act_all', { ids: missing.map((d) => d.id) })}>
                Set up all
              </Button>
            )}
            <Button kind="primary" icon={<ArrowRight className="size-4" />} onClick={next} disabled={requiredMissing.length > 0}>
              Continue
            </Button>
          </div>
        </>
      }
    >
      {!deps ? (
        <div className="flex items-center gap-2 text-[13px] text-mute">
          <Loader2 className="size-4 animate-spin" /> Checking this PC…
        </div>
      ) : (
        <div className="space-y-2">
          {list.map((d) => {
            const running = job?.id === d.id
            return (
              <div key={d.id} className="rounded-xl border border-line bg-card-2/40 px-4 py-3">
                <div className="flex items-center gap-3">
                  <span className={cx('grid size-8 shrink-0 place-items-center rounded-full', d.ok ? 'bg-brand/15 text-brand' : 'bg-card-2 text-mute')}>
                    {d.ok ? <Check className="size-4" /> : d.action?.kind === 'signin' ? <LogIn className="size-4" /> : <Download className="size-4" />}
                  </span>
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2 text-[13.5px] font-semibold">
                      {d.name}
                      {!d.required && <span className="text-[11px] font-medium text-mute">Optional</span>}
                    </div>
                    <div className="text-[12px] text-mute">{d.why.replace(/^(Optional|Recommended) — /, '')}</div>
                  </div>
                  {d.ok ? (
                    <span className="text-[12.5px] font-medium text-brand">Ready</span>
                  ) : d.action ? (
                    <Button size="sm" kind={d.required ? 'primary' : 'secondary'} loading={busyIds.has(d.id)} onClick={() => call('dep_act', { id: d.id, action: d.action!.kind })}>
                      {busyIds.has(d.id) && !running ? 'Waiting' : d.action.label}
                    </Button>
                  ) : null}
                </div>
                {running && job && (
                  <div className="mt-3">
                    <Progress value={job.progress} label={job.text} />
                  </div>
                )}
              </div>
            )
          })}
        </div>
      )}
    </Shell>
  )
}

function Finish({ status, back, onDone }: { status: Status; back: () => void; onDone: () => void }) {
  const [auto, setAuto] = useState(status.auto_run)
  return (
    <Shell
      title="You're all set"
      text="ROX-AI works on its own from here. Open it whenever you want to see what it found."
      footer={
        <>
          <BackBtn back={back} />
          <Button kind="primary" size="lg" icon={<Sparkles className="size-4" />} onClick={onDone}>
            Open my studio
          </Button>
        </>
      }
    >
      <Toggle
        on={auto}
        onChange={(v) => {
          setAuto(v)
          call('settings_set', { patch: { auto_run: v } })
        }}
        label="Run automatically in the background"
      />
      <div className="mt-3 text-[12.5px] text-mute">
        {auto ? (status.mode === 'active' ? 'Charts update every day and a few new games are learned daily, even when this window is closed.' : 'Charts update every day, even when this window is closed.') : "Nothing runs unless you press a button."}
      </div>
    </Shell>
  )
}
