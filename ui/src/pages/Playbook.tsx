import { ArrowLeft, BookMarked, Loader2, PenLine, RefreshCw } from 'lucide-react'
import { Note, Status, ago, call, useApi } from '../lib/api'
import { Badge, Button, Card, PageHeader, Skeleton, cx } from '../components/ui'
import { Markdown } from '../components/Markdown'
import { go } from '../App'

type Chapter = {
  id: string
  title: string
  emoji: string
  about: string
  note: string
  rules: number
  strong: number
  emerging: number
  opinion: number
  sources: number
  updated: string | null
  summary: string
}
type Book = { chapters: Chapter[]; due: string[]; rules: number; sources: number; writing: string | null }

export default function Playbook({ id, status }: { id: string; status: Status }) {
  const { data } = useApi<Book>('playbook', {}, 4000)
  if (id) return <ChapterView id={id} book={data} status={status} />
  const active = status.mode === 'active'
  const written = data?.chapters.filter((c) => c.rules > 0).length ?? 0

  return (
    <>
      <PageHeader
        title="Playbook"
        subtitle={data ? `${data.rules} rules from ${data.sources} sources · ${written} of ${data.chapters.length} chapters written` : 'What your team knows about making successful games.'}
        actions={
          active && (
            <Button icon={<PenLine className="size-4" />} disabled={status.busy} onClick={() => call('start', { what: 'playbook' })}>
              Write next chapter
            </Button>
          )
        }
      />
      <Card className="mb-5 flex items-center gap-4 p-5">
        <div className="grid size-11 shrink-0 place-items-center rounded-xl bg-brand/12 text-brand">
          <BookMarked className="size-5" />
        </div>
        <div className="text-[13px] leading-relaxed text-ink-2">
          Your team builds every game from these rules. Claude writes them from your game notes, developer devlogs and Roblox's docs, and refreshes one chapter after each day's learning. Numbers come straight from the games ROX-AI tracks.{' '}
          <span className="text-ink">Write your own rules in any chapter under "My rules" (in Obsidian) — they always win.</span>
        </div>
      </Card>

      {data?.writing && (
        <div className="mb-4 flex items-center gap-2 rounded-xl border border-sky/30 bg-sky/8 px-4 py-3 text-[13px] text-sky">
          <Loader2 className="size-4 animate-spin" /> Writing “{data.writing}” — reading devlogs and docs. This takes 10–20 minutes.
        </div>
      )}

      {!data ? (
        <div className="grid grid-cols-3 gap-4">{[...Array(6)].map((_, i) => <Skeleton key={i} className="h-40" />)}</div>
      ) : (
        <div className="grid grid-cols-3 gap-4">
          {data.chapters.map((c) => (
            <ChapterCard key={c.id} c={c} writing={data.writing === c.title} />
          ))}
        </div>
      )}
    </>
  )
}

function ConfidenceBar({ c }: { c: Chapter }) {
  if (!c.rules) return <div className="h-1.5 rounded-full bg-line" />
  const w = (n: number) => `${(n / c.rules) * 100}%`
  return (
    <div className="flex h-1.5 overflow-hidden rounded-full bg-line" title={`${c.strong} strong · ${c.emerging} emerging · ${c.opinion} opinion`}>
      <div className="bg-brand" style={{ width: w(c.strong) }} />
      <div className="bg-sky" style={{ width: w(c.emerging) }} />
      <div className="bg-line-2" style={{ width: w(c.opinion) }} />
    </div>
  )
}

function ChapterCard({ c, writing }: { c: Chapter; writing: boolean }) {
  return (
    <button onClick={() => go('playbook', c.id)} className={cx('flex flex-col rounded-xl border bg-card p-4 text-left transition-all hover:border-line-2 hover:bg-card-2/40', writing ? 'border-sky/40 ring-1 ring-sky/20' : 'border-line')}>
      <div className="flex items-start justify-between gap-2">
        <span className="text-[22px] leading-none">{c.emoji}</span>
        {writing ? <Badge tone="sky">Writing…</Badge> : c.rules ? <Badge tone="brand">{c.rules} rules</Badge> : <Badge>Not written</Badge>}
      </div>
      <div className="mt-3 text-[14.5px] font-semibold">{c.title}</div>
      <div className="mt-0.5 line-clamp-2 min-h-[36px] text-[12.5px] text-mute">{c.about}</div>
      <div className="mt-3">
        <ConfidenceBar c={c} />
      </div>
      <div className="mt-2 text-[11.5px] text-mute">{c.updated ? `${c.sources} sources · updated ${ago(c.updated)}` : 'Written during a learning run'}</div>
    </button>
  )
}

function ChapterView({ id, book, status }: { id: string; book?: Book; status: Status }) {
  const c = book?.chapters.find((x) => x.id === id)
  const writing = book?.writing === c?.title
  return (
    <>
      <button onClick={() => go('playbook')} className="mb-4 inline-flex items-center gap-1.5 text-[13px] font-medium text-mute hover:text-ink">
        <ArrowLeft className="size-4" /> Playbook
      </button>
      {c && (
        <div className="mb-5 flex items-center gap-3">
          <div className="flex flex-wrap gap-2">
            <Badge tone="brand">{c.strong} strong</Badge>
            <Badge tone="sky">{c.emerging} emerging</Badge>
            <Badge>{c.opinion} opinion</Badge>
            <Badge>{c.sources} sources</Badge>
          </div>
          <div className="ml-auto flex gap-2">
            <Button size="sm" kind="ghost" onClick={() => call('open', { what: 'note', path: c.note })}>
              Write my rules in Obsidian
            </Button>
            {status.mode === 'active' && (
              <Button size="sm" icon={<RefreshCw className="size-3.5" />} loading={writing} disabled={status.busy} onClick={() => call('start', { what: 'playbook', chapter: c.id })}>
                {writing ? 'Writing…' : c.rules ? 'Update now' : 'Write now'}
              </Button>
            )}
          </div>
        </div>
      )}
      {c ? <ChapterNote path={c.note} /> : <Skeleton className="h-60" />}
    </>
  )
}

function ChapterNote({ path }: { path: string }) {
  const { data: note } = useApi<Note>('note', { path }, 8000)
  return (
    <>
      <Card className="p-7">
        {!note ? (
          <div className="space-y-3">
            <Skeleton className="h-9 w-1/2" />
            <Skeleton className="h-60" />
          </div>
        ) : (
          <Markdown body={note.body} onLink={(t) => call<{ path: string | null }>('resolve', { target: t }).then((r) => r.path && go('vault', r.path))} />
        )}
      </Card>
    </>
  )
}
