import { Gamepad2, MessageSquare, Play, Send, Upload } from 'lucide-react'
import { useState } from 'react'
import { MyGame, call } from '../lib/api'
import { BuildProgress } from './BuildProgress'
import { Badge, Button, cx } from './ui'

const STAGE: Record<MyGame['stage'], { label: string; tone: 'neutral' | 'brand' | 'sky' | 'amber' | 'rose' }> = {
  idea: { label: 'Designing', tone: 'neutral' },
  building: { label: 'Building', tone: 'sky' },
  testing: { label: 'Testing', tone: 'amber' },
  ready: { label: 'Ready to play', tone: 'brand' },
  published: { label: 'Published', tone: 'brand' },
  failed: { label: 'Stopped', tone: 'rose' },
}

/** A game the team made: preview picture, stage, progress, and the Play / Feedback / Publish buttons. */
export function GameCard({ g, onChange }: { g: MyGame; onChange?: () => void }) {
  const st = STAGE[g.stage] ?? STAGE.building
  const done = g.stage === 'ready' || g.stage === 'published' || g.stage === 'failed'
  const [fb, setFb] = useState(false)
  const [text, setText] = useState('')
  const [sent, setSent] = useState(false)
  const send = () =>
    call('game_feedback', { id: g.id, text }).then(() => {
      setSent(true)
      setFb(false)
      setText('')
      onChange?.()
    })
  return (
    <div className="group overflow-hidden rounded-2xl border border-line bg-card transition-all hover:border-line-2 hover:shadow-xl hover:shadow-black/30">
      <div className="relative aspect-video bg-gradient-to-br from-card-2 to-bg">
        {g.thumb ? <img src={g.thumb} alt="" className="size-full object-cover" /> : <Gamepad2 className="absolute left-1/2 top-1/2 size-10 -translate-x-1/2 -translate-y-1/2 text-line-2" />}
        <div className="absolute left-3 top-3">
          <Badge tone={st.tone}>{st.label}</Badge>
        </div>
      </div>
      <div className="p-4">
        <div className="truncate text-[15px] font-semibold">{g.title}</div>
        <div className="text-[12px] text-mute">{g.genre}</div>
        {(g.summary || g.pitch) && <div className="mt-2 line-clamp-3 text-[12.5px] leading-relaxed text-ink-2">{done ? g.summary || g.pitch : g.pitch}</div>}
        {!done ? (
          <div className="mt-3">
            <BuildProgress size="sm" progress={g.progress} label={g.stage_text} eta={g.eta} />
          </div>
        ) : (
          <div className={cx('mt-3 flex gap-2')}>
            {g.stage !== 'failed' && (
              <Button size="sm" kind="primary" disabled title="Playing your private test version arrives in the next update" icon={<Play className="size-3.5" />}>
                Play
              </Button>
            )}
            <Button size="sm" icon={<MessageSquare className="size-3.5" />} onClick={() => setFb(!fb)}>
              Feedback
            </Button>
            {g.stage === 'ready' && (
              <Button size="sm" disabled title="Publishing arrives in the next update" icon={<Upload className="size-3.5" />}>
                Publish
              </Button>
            )}
          </div>
        )}
        {sent && !fb && <div className="fade-in mt-2 text-[12px] text-brand">Sent. The Lead turns it into tasks and the team gets to work.</div>}
        {fb && (
          <div className="fade-in mt-3 space-y-2">
            <textarea
              autoFocus
              value={text}
              onChange={(e) => setText(e.target.value)}
              rows={3}
              placeholder="What should the team change?"
              className="selectable w-full resize-none rounded-[10px] border border-line bg-bg px-3 py-2 text-[13px] outline-none placeholder:text-mute focus:border-brand/60"
            />
            <Button size="sm" kind="primary" disabled={!text.trim()} icon={<Send className="size-3.5" />} onClick={send}>
              Send to the team
            </Button>
          </div>
        )}
      </div>
    </div>
  )
}
