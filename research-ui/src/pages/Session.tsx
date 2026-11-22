import { AlertTriangle, CheckCircle2, Clapperboard, Eye, FolderOpen, Gamepad2, Play, RotateCcw, Send, SkipForward, Sparkles, Trash2 } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { Markdown } from '../components/Markdown'
import { Badge, Button, Card, cx, GameIcon, Progress, Skeleton } from '../components/ui'
import { call, clock, compact, fileUrl, go, join, openOverlay, Session, Status, useApi } from '../lib/api'

export function StatusBadge({ status }: { status: Session['status'] }) {
  const m: Record<Session['status'], [string, 'neutral' | 'brand' | 'sky' | 'amber' | 'rose']> = {
    planning: ['Planning', 'sky'],
    ready: ['Ready to film', 'neutral'],
    recording: ['Recording', 'rose'],
    recorded: ['Filmed', 'amber'],
    analyzing: ['Studying', 'sky'],
    done: ['Teardown ready', 'brand'],
    failed: ['Needs attention', 'rose'],
  }
  const [t, tone] = m[status] ?? [status, 'neutral']
  return <Badge tone={tone}>{t}</Badge>
}

export function SessionPage({ id, status }: { id: string; status?: Status }) {
  const { data, error, reload } = useApi<{ session: Session; dir: string }>('session', { id }, 2000)
  const [tab, setTab] = useState<'plan' | 'teardown'>('plan')
  const s = data?.session
  useEffect(() => {
    if (s?.status === 'done') setTab('teardown')
  }, [s?.status])

  if (error) return <div className="text-[13.5px] text-rose">{error}</div>
  if (!s || !data) return <Skeleton className="h-40" />

  const filmed = s.segments.filter((g) => g.file).length
  const handled = s.segments.filter((g) => g.file || g.skipped).length
  const allDone = s.plan.length > 0 && handled >= s.plan.length

  return (
    <div className="fade-in">
      {/* header */}
      <div className="mb-6 flex items-center gap-4">
        <GameIcon src={s.game.icon} title={s.game.name} size={64} />
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2.5">
            <h1 className="truncate text-[24px] font-bold tracking-tight text-ink">{s.game.name}</h1>
            <StatusBadge status={s.status} />
          </div>
          <div className="mt-0.5 text-[13px] text-mute">
            by {s.game.creator} · {compact(s.game.playing)} playing · {s.game.genre || 'Roblox'} · {filmed}/{s.plan.length} segments filmed
          </div>
        </div>
        <div className="flex items-center gap-2">
          <Button kind="ghost" size="sm" icon={<FolderOpen className="size-4" />} onClick={() => call('open', { what: 'session', id })}>
            Files
          </Button>
          <DeleteButton id={id} disabled={s.status === 'recording' || s.status === 'analyzing'} />
        </div>
      </div>

      {s.status === 'planning' ? (
        <Card className="p-6">
          <div className="flex items-center gap-2.5 text-[15px] font-semibold text-ink">
            <Sparkles className="size-5 text-sky" /> The AI is planning what to film
          </div>
          <div className="mt-1 text-[13px] text-mute">It's looking the game up to write a shot list for it. This takes a minute or two.</div>
          <div className="mt-4">
            <Progress value={null} label={s.step || 'Working…'} />
          </div>
          <div className="mt-5 space-y-2">
            {[0, 1, 2].map((k) => (
              <Skeleton key={k} className="h-14" />
            ))}
          </div>
        </Card>
      ) : (
        <>
          {s.status === 'analyzing' && (
            <Card className="mb-5 border-sky/30 p-5">
              <div className="flex items-center gap-2.5 text-[15px] font-semibold text-ink">
                <Sparkles className="size-5 text-sky" /> The AI is studying your videos
              </div>
              <div className="mt-1 text-[13px] text-mute">It watches every segment frame by frame with your notes, then writes the teardown. You can close the app; it carries on next time.</div>
              <div className="mt-4">
                <Progress value={null} label={s.step || 'Working…'} />
              </div>
            </Card>
          )}
          {s.status === 'failed' && (
            <Card className="mb-5 border-rose/30 p-5">
              <div className="flex items-center gap-2.5 text-[15px] font-semibold text-ink">
                <AlertTriangle className="size-5 text-rose" /> The study stopped
              </div>
              <div className="selectable mt-1 text-[13px] text-ink-2">{s.error || 'Something went wrong.'}</div>
              <div className="mt-3">
                <Button kind="primary" size="sm" icon={<RotateCcw className="size-3.5" />} onClick={() => call('push', { id, notes: s.final_notes }).then(reload)}>
                  Try again
                </Button>
              </div>
            </Card>
          )}

          {(s.status === 'ready' || s.status === 'recording' || s.status === 'recorded') && <FilmBar s={s} status={status} filmed={filmed} allDone={allDone} />}

          {s.teardown && (
            <div className="mb-4 flex gap-1 rounded-xl border border-line bg-panel p-1">
              {(['teardown', 'plan'] as const).map((t) => (
                <button key={t} onClick={() => setTab(t)} className={cx('h-8 flex-1 rounded-lg text-[13px] font-semibold', tab === t ? 'bg-card-2 text-ink' : 'text-mute hover:text-ink')}>
                  {t === 'teardown' ? 'Teardown' : 'Segments & notes'}
                </button>
              ))}
            </div>
          )}

          {tab === 'teardown' && s.teardown ? (
            <Teardown id={id} />
          ) : (
            <>
              <div className="space-y-3">
                {s.plan.map((shot, i) => (
                  <SegmentCard key={i} s={s} dir={data.dir} i={i} shotTitle={shot.title} />
                ))}
              </div>
              {(allDone || s.status === 'recorded' || s.final_notes) && s.status !== 'analyzing' && s.status !== 'done' && <PushCard s={s} filmed={filmed} reload={reload} />}
            </>
          )}
        </>
      )}
    </div>
  )
}

function FilmBar({ s, status, filmed, allDone }: { s: Session; status?: Status; filmed: number; allDone: boolean }) {
  const [err, setErr] = useState('')
  const rec = status?.recording?.id === s.id
  if (allDone) return null
  return (
    <Card className="mb-5 flex items-center gap-4 border-brand/25 bg-[linear-gradient(120deg,rgba(0,214,168,.08),transparent_60%)] p-5">
      <div className="grid size-11 place-items-center rounded-xl bg-brand/15 text-brand">
        <Clapperboard className="size-5" />
      </div>
      <div className="min-w-0 flex-1">
        <div className="text-[15px] font-semibold text-ink">{rec ? 'Recording now' : filmed ? `Carry on filming — segment ${s.current + 1} of ${s.plan.length}` : `Ready to film ${s.plan.length} segments`}</div>
        <div className="mt-0.5 text-[12.5px] text-mute">
          {status?.roblox ? 'Roblox is open. Start the guide and it will float over the game.' : 'Open the game in Roblox (on your alt account), then start the guide.'}
        </div>
        {err && <div className="mt-1 text-[12.5px] text-rose">{err}</div>}
      </div>
      {!status?.roblox && (
        <Button icon={<Gamepad2 className="size-4" />} onClick={() => call('launch', { place_id: s.game.place_id }).catch((e) => setErr(String(e)))}>
          Open in Roblox
        </Button>
      )}
      <Button kind="primary" size="lg" icon={<Play className="size-4 fill-current" />} onClick={() => openOverlay(s.id).catch((e) => setErr(String(e)))}>
        {rec ? 'Show guide' : 'Start the guide'}
      </Button>
    </Card>
  )
}

function SegmentCard({ s, dir, i, shotTitle }: { s: Session; dir: string; i: number; shotTitle: string }) {
  const shot = s.plan[i]
  const g = s.segments[i]
  const [notes, setNotes] = useState(g?.notes ?? '')
  const [open, setOpen] = useState(false)
  const editing = useRef(false)
  const timer = useRef<number>(0)
  useEffect(() => {
    if (!editing.current) setNotes(g?.notes ?? '')
  }, [g?.notes])
  const save = (t: string) => {
    setNotes(t)
    editing.current = true
    clearTimeout(timer.current)
    timer.current = window.setTimeout(() => {
      call('notes', { id: s.id, index: i, notes: t }).finally(() => (editing.current = false))
    }, 600)
  }
  const isNext = i === s.current && s.status !== 'done' && s.status !== 'analyzing'
  const locked = s.status === 'analyzing'
  return (
    <Card className={cx('overflow-hidden', isNext && !g?.file && 'border-sky/40')}>
      <div className="flex gap-4 p-4">
        <div className="w-[300px] shrink-0">
          {g?.file && s.videos_deleted ? (
            <div className="grid aspect-video w-full place-items-center rounded-lg border border-line bg-panel text-center">
              <div>
                <CheckCircle2 className="mx-auto size-5 text-brand" />
                <div className="mt-1.5 px-4 text-[12px] text-mute">Studied. The video was deleted; its screenshots are in the analysis.</div>
              </div>
            </div>
          ) : g?.file ? (
            <video src={fileUrl(join(dir, g.file))} controls preload="metadata" className="aspect-video w-full rounded-lg bg-black ring-1 ring-line" />
          ) : (
            <div className="grid aspect-video w-full place-items-center rounded-lg border border-dashed border-line-2 bg-panel text-center">
              <div>
                {g?.skipped ? <SkipForward className="mx-auto size-5 text-amber" /> : <Clapperboard className="mx-auto size-5 text-mute" />}
                <div className="mt-1.5 text-[12px] text-mute">{g?.skipped ? 'Skipped' : isNext ? 'Up next' : 'Not filmed yet'}</div>
              </div>
            </div>
          )}
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <span className="grid size-6 shrink-0 place-items-center rounded-md bg-card-2 text-[12px] font-bold text-ink-2">{i + 1}</span>
            <div className="truncate text-[14.5px] font-semibold text-ink">{shotTitle}</div>
            {g?.file && (
              <span className="ml-auto flex shrink-0 items-center gap-1 text-[12px] text-brand">
                <CheckCircle2 className="size-3.5" /> {clock(g.seconds)}
              </span>
            )}
            {!g?.file && <span className="ml-auto shrink-0 text-[12px] text-mute">~{shot.minutes} min</span>}
          </div>
          {shot.goal && <div className="mt-1 text-[12.5px] leading-relaxed text-ink-2">{shot.goal}</div>}
          <button onClick={() => setOpen(!open)} className="mt-1 text-[12px] font-medium text-sky hover:underline">
            {open ? 'Hide what to film' : 'What to film'}
          </button>
          {open && (
            <div className="mt-2 grid grid-cols-2 gap-3 text-[12.5px]">
              <ol className="list-decimal space-y-0.5 pl-4 text-ink-2">
                {shot.steps.map((t, k) => (
                  <li key={k}>{t}</li>
                ))}
              </ol>
              <div>
                <div className="mb-0.5 flex items-center gap-1 font-semibold text-mute">
                  <Eye className="size-3.5" /> Must show
                </div>
                <ul className="space-y-0.5 text-ink-2">
                  {shot.look_for.map((t, k) => (
                    <li key={k}>• {t}</li>
                  ))}
                </ul>
              </div>
            </div>
          )}
          <textarea
            value={notes}
            disabled={locked}
            onChange={(e) => save(e.target.value)}
            placeholder="Your notes on this segment"
            className="selectable mt-2.5 h-[72px] w-full resize-none rounded-lg border border-line bg-panel px-3 py-2 text-[13px] text-ink outline-none placeholder:text-mute/70 focus:border-sky/60 disabled:opacity-60"
          />
        </div>
      </div>
    </Card>
  )
}

function PushCard({ s, filmed, reload }: { s: Session; filmed: number; reload: () => void }) {
  const [notes, setNotes] = useState(s.final_notes)
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')
  const timer = useRef<number>(0)
  const save = (t: string) => {
    setNotes(t)
    clearTimeout(timer.current)
    timer.current = window.setTimeout(() => call('final_notes', { id: s.id, notes: t }).catch(() => {}), 600)
  }
  const push = async () => {
    setBusy(true)
    setErr('')
    try {
      clearTimeout(timer.current)
      await call('push', { id: s.id, notes })
      reload()
    } catch (e) {
      setErr(String((e as Error).message ?? e))
    } finally {
      setBusy(false)
    }
  }
  return (
    <Card className="mt-5 p-5">
      <div className="text-[15px] font-semibold text-ink">Final notes</div>
      <div className="mt-0.5 text-[12.5px] text-mute">Your overall take: what makes this game work, what hooked you, what felt weak, how it makes money, what you'd copy.</div>
      <textarea
        value={notes}
        onChange={(e) => save(e.target.value)}
        placeholder="Overall, this game…"
        className="selectable mt-3 h-36 w-full resize-y rounded-lg border border-line bg-panel px-3 py-2.5 text-[13.5px] text-ink outline-none placeholder:text-mute/70 focus:border-brand/60"
      />
      {err && <div className="mt-2 text-[12.5px] text-rose">{err}</div>}
      <div className="mt-3 flex items-center justify-between gap-3">
        <div className="text-[12.5px] text-mute">{filmed === 0 ? 'Film at least one segment to push.' : `${filmed} segment${filmed === 1 ? '' : 's'} and your notes go to the AI. Videos stay on this PC.`}</div>
        <Button kind="primary" size="lg" loading={busy} disabled={filmed === 0} onClick={push} icon={<Send className="size-4" />}>
          Push
        </Button>
      </div>
    </Card>
  )
}

export function Teardown({ id }: { id: string }) {
  const { data } = useApi<{ markdown: string; path: string }>('teardown', { id })
  if (!data) return <Skeleton className="h-60" />
  const base = data.path.replace(/[\\/][^\\/]*$/, '')
  return (
    <Card className="p-8">
      <Markdown body={data.markdown.replace(/^---[\s\S]*?\n---\n/, '')} base={base} />
    </Card>
  )
}

function DeleteButton({ id, disabled }: { id: string; disabled: boolean }) {
  const [sure, setSure] = useState(false)
  useEffect(() => {
    if (!sure) return
    const t = setTimeout(() => setSure(false), 4000)
    return () => clearTimeout(t)
  }, [sure])
  return (
    <Button
      kind={sure ? 'danger' : 'ghost'}
      size="sm"
      disabled={disabled}
      icon={<Trash2 className="size-4" />}
      onClick={() => (sure ? call('session_delete', { id }).then(() => go('#/sessions')) : setSure(true))}
    >
      {sure ? 'Delete videos and notes?' : ''}
    </Button>
  )
}
