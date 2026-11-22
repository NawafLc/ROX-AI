import { ArrowLeft, FolderOpen, GraduationCap } from 'lucide-react'
import { Button, Card, Empty, GameIcon, PageHeader, Skeleton } from '../components/ui'
import { call, compact, go, Session, SessionRow, useApi } from '../lib/api'
import { Teardown } from './Session'

/** Every game whose analysis is finished, by name. */
export function StudiedPage() {
  const { data } = useApi<SessionRow[]>('sessions', {}, 5000)
  const done = (data ?? []).filter((s) => !!s.teardown)
  return (
    <div className="fade-in">
      <PageHeader title="Studied" subtitle="Every game you've filmed and the AI has analysed, with its full breakdown and screenshots." />
      {!data ? (
        <div className="grid grid-cols-3 gap-3">
          {[0, 1, 2].map((k) => (
            <Skeleton key={k} className="h-[120px]" />
          ))}
        </div>
      ) : done.length === 0 ? (
        <Empty
          icon={<GraduationCap className="size-5" />}
          title="No games studied yet"
          text="Film a game and push it. When the AI finishes its analysis, the game shows up here."
          action={
            <Button kind="primary" onClick={() => go('#/')}>
              Study a game
            </Button>
          }
        />
      ) : (
        <div className="grid grid-cols-3 gap-3">
          {done.map((s) => (
            <Card key={s.id} onClick={() => go(`#/studied/${s.id}`)} className="cursor-pointer p-4 transition-colors hover:border-line-2 hover:bg-card-2">
              <GameIcon src={s.icon} title={s.game} size={56} />
              <div className="mt-3 truncate text-[15px] font-semibold text-ink">{s.game}</div>
              <div className="mt-0.5 text-[12px] text-mute">
                Studied {new Date(s.created).toLocaleDateString(undefined, { dateStyle: 'medium' })} · {s.recorded} segments
              </div>
            </Card>
          ))}
        </div>
      )}
    </div>
  )
}

/** One studied game: its full analysis. */
export function StudiedGame({ id }: { id: string }) {
  const { data } = useApi<{ session: Session }>('session', { id })
  const s = data?.session
  if (!s) return <Skeleton className="h-40" />
  return (
    <div className="fade-in">
      <button onClick={() => go('#/studied')} className="mb-4 flex items-center gap-1.5 text-[13px] text-mute hover:text-ink">
        <ArrowLeft className="size-4" /> Studied
      </button>
      <div className="mb-6 flex items-center gap-4">
        <GameIcon src={s.game.icon} title={s.game.name} size={64} />
        <div className="min-w-0 flex-1">
          <h1 className="truncate text-[24px] font-bold tracking-tight text-ink">{s.game.name}</h1>
          <div className="mt-0.5 text-[13px] text-mute">
            by {s.game.creator} · {compact(s.game.playing)} playing · {compact(s.game.visits)} visits · {s.game.genre || 'Roblox'}
          </div>
        </div>
        <Button kind="ghost" size="sm" icon={<FolderOpen className="size-4" />} onClick={() => call('open', { what: 'teardown', id })}>
          Open note
        </Button>
        <Button size="sm" onClick={() => go(`#/s/${id}`)}>
          Segments & notes
        </Button>
      </div>
      {s.teardown ? <Teardown id={id} /> : <div className="text-[13px] text-mute">The analysis isn't ready yet.</div>}
    </div>
  )
}
