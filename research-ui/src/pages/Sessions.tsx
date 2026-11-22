import { Clapperboard } from 'lucide-react'
import { Button, Card, Empty, GameIcon, PageHeader, Skeleton } from '../components/ui'
import { go, SessionRow, useApi } from '../lib/api'
import { StatusBadge } from './Session'

export function SessionsPage() {
  const { data } = useApi<SessionRow[]>('sessions', {}, 4000)
  return (
    <div className="fade-in">
      <PageHeader title="Sessions" subtitle="Every game you've filmed, with its segments, your notes and the AI's teardown." />
      {!data ? (
        <div className="space-y-2">
          {[0, 1, 2].map((k) => (
            <Skeleton key={k} className="h-[68px]" />
          ))}
        </div>
      ) : data.length === 0 ? (
        <Empty icon={<Clapperboard className="size-5" />} title="No sessions yet" text="Pick a game to study and the AI will plan what to film." action={<Button kind="primary" onClick={() => go('#/')}>Study a game</Button>} />
      ) : (
        <div className="space-y-2">
          {data.map((s) => (
            <Card key={s.id} onClick={() => go(`#/s/${s.id}`)} className="flex cursor-pointer items-center gap-3.5 p-3 transition-colors hover:border-line-2 hover:bg-card-2">
              <GameIcon src={s.icon} title={s.game} size={44} />
              <div className="min-w-0 flex-1">
                <div className="truncate text-[14px] font-semibold text-ink">{s.game}</div>
                <div className="text-[12px] text-mute">
                  {new Date(s.created).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' })} · {s.recorded}/{s.segments} segments filmed
                </div>
              </div>
              <StatusBadge status={s.status} />
            </Card>
          ))}
        </div>
      )}
    </div>
  )
}
