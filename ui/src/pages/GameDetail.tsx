import { ArrowLeft, Brain, ExternalLink, FileText, Play, Video } from 'lucide-react'
import { Note, Status, ago, call, fmt, useApi } from '../lib/api'
import { Badge, Button, Card, GameIcon, MomentumBar, Skeleton, SourceBadge, Sparkline } from '../components/ui'
import { Markdown } from '../components/Markdown'
import { go } from '../App'

type Game = {
  uid: number
  title: string
  genre: string
  subgenre: string
  rank: number | null
  best_rank: number | null
  playing: number
  like_ratio: number
  momentum: number
  fun: number | null
  one_liner: string
  source: string
  source_label: string
  last_analysis: string
  videos: string[]
  url: string
  creator: string
  first_seen: string
  history: { day: string; rank: number; playing: number }[]
  note: Note | null
}

export default function GameDetail({ uid, status }: { uid: number; status: Status }) {
  const { data: g, error } = useApi<Game>('game', { uid })
  if (error) return <div className="text-mute">{error}</div>
  if (!g)
    return (
      <div className="space-y-4">
        <Skeleton className="h-24" />
        <Skeleton className="h-40" />
        <Skeleton className="h-96" />
      </div>
    )
  const hist = [...g.history].sort((a, b) => a.day.localeCompare(b.day))
  const learning = status.activity.kind === 'learning' && status.activity.title.includes(g.title)
  const active = status.mode === 'active'

  return (
    <>
      <button onClick={() => history.back()} className="mb-4 inline-flex items-center gap-1.5 text-[13px] font-medium text-mute hover:text-ink">
        <ArrowLeft className="size-4" /> Back
      </button>

      <div className="flex items-start gap-5">
        <GameIcon uid={g.uid} title={g.title} size={84} />
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <h1 className="text-[26px] font-bold tracking-tight">{g.title}</h1>
            <SourceBadge source={g.source} label={g.source_label} />
          </div>
          <div className="mt-1 text-[13.5px] text-mute">
            {[g.genre, g.subgenre].filter(Boolean).join(' · ') || 'Unknown genre'}
            {g.creator && <> · by {g.creator}</>}
          </div>
          {g.one_liner && <div className="mt-2.5 max-w-2xl text-[14px] text-ink-2">{g.one_liner}</div>}
        </div>
        <div className="flex shrink-0 gap-2">
          <Button icon={<Play className="size-4" />} onClick={() => call('open', { what: 'url', url: g.url })}>
            Roblox page
          </Button>
          {active && (
            <Button kind="primary" icon={<Brain className="size-4" />} loading={learning} disabled={status.busy} onClick={() => call('start', { what: 'learn_game', uid: g.uid })}>
              {learning ? 'Learning…' : g.source ? 'Learn again' : 'Learn now'}
            </Button>
          )}
        </div>
      </div>

      <div className="mt-6 grid grid-cols-6 gap-3">
        <Mini label="Chart rank" value={g.rank ? `#${g.rank}` : 'Off chart'} sub={g.best_rank ? `Best #${g.best_rank}` : undefined} />
        <Mini label="Playing now" value={fmt(g.playing)} />
        <Mini label="Likes" value={g.like_ratio ? `${Math.round(g.like_ratio)}%` : '–'} />
        <Mini label="Momentum" value={<MomentumBar value={g.momentum} />} />
        <Mini label="Fun score" value={g.fun !== null ? `${g.fun}/10` : '–'} />
        <Mini label="Learned" value={g.last_analysis ? ago(g.last_analysis) : 'Not yet'} />
      </div>

      <div className="mt-4 grid grid-cols-3 gap-4">
        <Card className="col-span-2 p-5">
          <div className="text-[13px] font-semibold text-ink-2">Players over time</div>
          <div className="mt-3">
            <Sparkline values={hist.map((h) => h.playing)} width={640} height={70} />
          </div>
          {hist.length >= 2 && (
            <div className="mt-2 flex justify-between text-[11px] text-mute">
              <span>{hist[0].day}</span>
              <span>{hist[hist.length - 1].day}</span>
            </div>
          )}
        </Card>
        <Card className="p-5">
          <div className="flex items-center gap-2 text-[13px] font-semibold text-ink-2">
            <Video className="size-4 text-rose" /> Videos it learned from
          </div>
          {g.videos.length === 0 ? (
            <div className="mt-3 text-[12.5px] text-mute">{active ? 'None yet — it will learn from YouTube soon.' : 'Active mode learns games from YouTube videos.'}</div>
          ) : (
            <div className="mt-3 space-y-1.5">
              {g.videos.slice(0, 6).map((v) => {
                const id = v.replace(/.*[?&]v=|.*youtu\.be\//, '').slice(0, 11)
                return (
                  <button key={v} onClick={() => call('open', { what: 'url', url: `https://www.youtube.com/watch?v=${id}` })} className="flex w-full items-center gap-2 rounded-lg bg-card-2 px-2.5 py-2 text-left text-[12.5px] text-ink-2 hover:text-ink">
                    <ExternalLink className="size-3.5 shrink-0 text-mute" />
                    <span className="truncate">youtube.com/watch?v={id}</span>
                  </button>
                )
              })}
            </div>
          )}
        </Card>
      </div>

      <Card className="mt-4 p-7">
        {g.note ? (
          <>
            <div className="mb-2 flex items-center justify-between">
              <Badge>
                <FileText className="size-3" /> Vault note
              </Badge>
              <button onClick={() => call('open', { what: 'note', path: g.note!.path })} className="text-[12.5px] font-medium text-brand hover:underline">
                Open in Obsidian
              </button>
            </div>
            <Markdown body={g.note.body} onLink={(t) => call<{ path: string | null }>('resolve', { target: t }).then((r) => r.path && go('vault', r.path))} />
          </>
        ) : (
          <div className="text-[13px] text-mute">No vault note for this game yet.</div>
        )}
      </Card>
    </>
  )
}

function Mini({ label, value, sub }: { label: string; value: React.ReactNode; sub?: string }) {
  return (
    <Card className="px-4 py-3">
      <div className="text-[11.5px] font-medium text-mute">{label}</div>
      <div className="mt-1 text-[18px] font-bold tracking-tight">{value}</div>
      {sub && <div className="text-[11px] text-mute">{sub}</div>}
    </Card>
  )
}
