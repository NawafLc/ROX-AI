import { BookOpen, FileText, FolderOpen } from 'lucide-react'
import { Button, Card, Empty, PageHeader } from '../components/ui'
import { call, go, SessionRow, useApi } from '../lib/api'

/** The new Playbook. It is separate from ROX-AI's old one and starts empty; it grows from teardowns. */
export function PlaybookPage() {
  const { data } = useApi<SessionRow[]>('sessions', {})
  const done = (data ?? []).filter((s) => s.status === 'done' && s.teardown)
  return (
    <div className="fade-in">
      <PageHeader
        title="Playbook"
        subtitle="Your new Playbook, built only from games you film here. It starts empty and grows with every teardown."
        actions={
          <Button icon={<FolderOpen className="size-4" />} onClick={() => call('open', { what: 'vault' })}>
            Open vault
          </Button>
        }
      />
      <div className="mb-3 text-[13px] font-semibold text-ink-2">Teardowns ({done.length})</div>
      {done.length === 0 ? (
        <Empty icon={<BookOpen className="size-5" />} title="Nothing learned yet" text="Film a game and push the session. Its teardown shows up here." action={<Button kind="primary" onClick={() => go('#/')}>Study a game</Button>} />
      ) : (
        <div className="grid grid-cols-2 gap-2">
          {done.map((s) => (
            <Card key={s.id} onClick={() => go(`#/s/${s.id}`)} className="flex cursor-pointer items-center gap-3 p-3.5 hover:border-line-2 hover:bg-card-2">
              <FileText className="size-4 text-brand" />
              <span className="flex-1 truncate text-[13.5px] font-semibold text-ink">{s.game}</span>
              <span className="text-[12px] text-mute">{new Date(s.created).toLocaleDateString()}</span>
            </Card>
          ))}
        </div>
      )}
    </div>
  )
}
