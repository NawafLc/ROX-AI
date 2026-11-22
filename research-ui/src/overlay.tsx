// The guide that floats over Roblox while filming. Windows hides it from screen capture, so it never shows in a video.
import '@fontsource-variable/inter'
import './styles.css'
import { CheckCircle2, ChevronLeft, ChevronsDownUp, ChevronsUpDown, Circle, Eye, Square, SkipForward, X } from 'lucide-react'
import { StrictMode, useEffect, useRef, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { Button, cx } from './components/ui'
import { call, clock, closeOverlay, Session, Status, useApi } from './lib/api'

const id = new URLSearchParams(location.search).get('id') ?? ''
const FULL = { w: 380, h: 640 }
const SMALL = { w: 380, h: 58 }

async function resize(w: number, h: number) {
  const { getCurrentWindow, LogicalSize } = await import('@tauri-apps/api/window')
  await getCurrentWindow().setSize(new LogicalSize(w, h))
}

function Overlay() {
  const { data, reload } = useApi<{ session: Session }>('session', { id }, 1500)
  const { data: st } = useApi<Status>('status', {}, 1000)
  const [notes, setNotes] = useState('')
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')
  const [small, setSmall] = useState(false)
  const shownIndex = useRef(-1)
  const saveTimer = useRef<number>(0)

  const s = data?.session
  const i = s?.current ?? 0
  const shot = s?.plan[i]
  const rec = st?.recording && st.recording.id === id ? st.recording : null
  const done = !!s && i >= s.plan.length

  // load this segment's saved notes when the segment changes
  useEffect(() => {
    if (!s || shownIndex.current === i) return
    shownIndex.current = i
    setNotes(s.segments[i]?.notes ?? '')
  }, [s, i])

  const save = (text: string) => {
    setNotes(text)
    clearTimeout(saveTimer.current)
    saveTimer.current = window.setTimeout(() => call('notes', { id, index: i, notes: text }).catch(() => {}), 600)
  }

  const run = async (fn: () => Promise<unknown>) => {
    setBusy(true)
    setErr('')
    try {
      await fn()
      await reload()
    } catch (e) {
      setErr(String((e as Error).message ?? e))
    } finally {
      setBusy(false)
    }
  }

  const toggleSize = () => {
    const next = !small
    setSmall(next)
    const z = next ? SMALL : FULL
    resize(z.w, z.h)
  }

  const start = () => run(() => call('rec_start', { id, index: i }))
  const finish = () =>
    run(async () => {
      clearTimeout(saveTimer.current)
      shownIndex.current = -1
      await call('rec_stop', { id, notes })
    })
  const skip = () =>
    run(async () => {
      clearTimeout(saveTimer.current)
      await call('notes', { id, index: i, notes })
      shownIndex.current = -1
      await call('skip', { id, index: i })
    })
  const back = () =>
    run(async () => {
      shownIndex.current = -1
      await call('goto', { id, index: Math.max(0, i - 1) })
    })

  const target = (shot?.minutes ?? 0) * 60
  const over = rec && target > 0 && rec.seconds > target

  // ---------- compact bar ----------
  if (small)
    return (
      <div data-tauri-drag-region className="flex h-full items-center gap-2.5 border border-line-2 bg-panel px-3">
        {rec ? <span className="pulse-dot size-2.5 shrink-0 rounded-full bg-rose text-rose" /> : <span className="size-2.5 shrink-0 rounded-full bg-mute/50" />}
        <div data-tauri-drag-region className="min-w-0 flex-1">
          <div data-tauri-drag-region className="truncate text-[12.5px] font-semibold text-ink">
            {done ? 'All segments filmed' : `${i + 1}/${s?.plan.length ?? 0} · ${shot?.title ?? ''}`}
          </div>
          {rec && (
            <div className={cx('text-[11.5px] tabular-nums', over ? 'text-amber' : 'text-mute')}>
              {clock(rec.seconds)} / {clock(target)}
            </div>
          )}
        </div>
        {!done &&
          (rec ? (
            <Button size="sm" kind="danger" loading={busy} onClick={finish} icon={<Square className="size-3.5" />}>
              Finish
            </Button>
          ) : (
            <Button size="sm" kind="primary" loading={busy} disabled={!st?.roblox} onClick={start} icon={<Circle className="size-3.5 fill-current" />}>
              Record
            </Button>
          ))}
        <button onClick={toggleSize} className="grid size-7 place-items-center rounded-md text-mute hover:bg-card-2 hover:text-ink" aria-label="Expand">
          <ChevronsUpDown className="size-4" />
        </button>
      </div>
    )

  // ---------- full guide ----------
  return (
    <div className="flex h-full flex-col border border-line-2 bg-panel">
      <div data-tauri-drag-region className="flex h-10 shrink-0 items-center gap-2 border-b border-line px-3">
        {rec ? <span className="pulse-dot size-2 rounded-full bg-rose text-rose" /> : <span className="size-2 rounded-full bg-brand" />}
        <span data-tauri-drag-region className="flex-1 truncate text-[12px] font-semibold text-ink-2">
          {s ? s.game.name : 'ROX Research'}
        </span>
        <button onClick={toggleSize} className="grid size-7 place-items-center rounded-md text-mute hover:bg-card-2 hover:text-ink" aria-label="Make smaller" title="Make smaller">
          <ChevronsDownUp className="size-4" />
        </button>
        <button
          onClick={() => !rec && closeOverlay(true, `#/s/${id}`)}
          disabled={!!rec}
          className="grid size-7 place-items-center rounded-md text-mute hover:bg-card-2 hover:text-ink disabled:opacity-30"
          aria-label="Close guide"
          title={rec ? 'Finish the segment first' : 'Back to ROX Research'}
        >
          <X className="size-4" />
        </button>
      </div>

      {!s ? (
        <div className="p-4 text-[13px] text-mute">Loading…</div>
      ) : done ? (
        <div className="flex flex-1 flex-col items-center justify-center gap-3 px-6 text-center">
          <CheckCircle2 className="size-10 text-brand" />
          <div className="text-[16px] font-bold text-ink">All segments filmed</div>
          <div className="text-[13px] text-mute">Write your final notes and push the session so the AI can study it.</div>
          <Button kind="primary" onClick={() => closeOverlay(true, `#/s/${id}`)}>
            Write final notes
          </Button>
          <button onClick={back} className="text-[12.5px] text-mute hover:text-ink">
            Go back to the last segment
          </button>
        </div>
      ) : (
        <>
          <div className="flex-1 overflow-y-auto px-4 pb-3 pt-3">
            {/* progress through the shot list */}
            <div className="mb-3 flex gap-1">
              {s.plan.map((_, k) => {
                const g = s.segments[k]
                return <div key={k} className={cx('h-1 flex-1 rounded-full', k === i ? 'bg-sky' : g?.file ? 'bg-brand' : g?.skipped ? 'bg-amber/60' : 'bg-line-2')} />
              })}
            </div>
            <div className="text-[11.5px] font-semibold uppercase tracking-wider text-mute">
              Segment {i + 1} of {s.plan.length} · about {shot!.minutes} min
            </div>
            <div className="mt-1 text-[17px] font-bold leading-snug tracking-tight text-ink">{shot!.title}</div>
            {shot!.goal && <div className="mt-1 text-[12.5px] leading-relaxed text-ink-2">{shot!.goal}</div>}

            <ol className="mt-3 space-y-1.5">
              {shot!.steps.map((t, k) => (
                <li key={k} className="flex gap-2 text-[13px] leading-snug text-ink">
                  <span className="mt-px grid size-[18px] shrink-0 place-items-center rounded-full bg-card-2 text-[10.5px] font-bold text-sky">{k + 1}</span>
                  <span>{t}</span>
                </li>
              ))}
            </ol>

            {shot!.look_for.length > 0 && (
              <div className="mt-3 rounded-lg border border-line bg-card p-2.5">
                <div className="mb-1 flex items-center gap-1.5 text-[11.5px] font-semibold text-mute">
                  <Eye className="size-3.5" /> Make sure the video shows
                </div>
                <ul className="space-y-0.5 text-[12.5px] text-ink-2">
                  {shot!.look_for.map((t, k) => (
                    <li key={k}>• {t}</li>
                  ))}
                </ul>
              </div>
            )}
          </div>

          <div className="shrink-0 border-t border-line p-3">
            <div className="mb-2.5">
              <div className="mb-1 text-[11.5px] font-semibold text-mute">Your notes on this segment</div>
              <textarea
                value={notes}
                onChange={(e) => save(e.target.value)}
                placeholder="What do you notice? What feels good or bad? Anything the video can't show…"
                className="selectable h-[84px] w-full resize-none rounded-lg border border-line bg-card px-3 py-2 text-[13px] text-ink outline-none placeholder:text-mute/70 focus:border-sky/60"
              />
            </div>
            {err && <div className="mb-2 text-[12px] text-rose">{err}</div>}
            {!rec && !st?.roblox && <div className="mb-2 text-[12px] text-amber">Open the game in Roblox to record. Only the Roblox window and its sound are recorded.</div>}
            {rec ? (
              <div className="flex items-center gap-3">
                <div className="flex-1">
                  <div className={cx('text-[22px] font-bold tabular-nums leading-none', over ? 'text-amber' : 'text-ink')}>{clock(rec.seconds)}</div>
                  <div className="mt-1 text-[11.5px] text-mute">{over ? 'Past the suggested time — finish when ready' : `Suggested ${clock(target)}`}</div>
                </div>
                <Button kind="danger" size="lg" loading={busy} onClick={finish} icon={<Square className="size-4" />}>
                  Finish segment
                </Button>
              </div>
            ) : (
              <div className="flex items-center gap-2">
                <button
                  onClick={back}
                  disabled={i === 0 || busy}
                  className="grid size-10 place-items-center rounded-[10px] text-mute hover:bg-card-2 hover:text-ink disabled:opacity-30"
                  title="Previous segment"
                >
                  <ChevronLeft className="size-5" />
                </button>
                <Button kind="primary" size="lg" className="flex-1" loading={busy} disabled={!st?.roblox || !st?.ffmpeg} onClick={start} icon={<Circle className="size-4 fill-current" />}>
                  {s.segments[i]?.file ? 'Record again' : 'Start recording'}
                </Button>
                <Button kind="ghost" size="lg" disabled={busy} onClick={skip} icon={<SkipForward className="size-4" />} title="Skip this segment">
                  Skip
                </Button>
              </div>
            )}
          </div>
        </>
      )}
    </div>
  )
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <Overlay />
  </StrictMode>,
)
