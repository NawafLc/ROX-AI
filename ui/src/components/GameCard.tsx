import { Gamepad2, MessageSquare, Play, Upload } from 'lucide-react'
import { MyGame } from '../lib/api'
import { Badge, Button, Progress, cx } from './ui'

const STAGE: Record<MyGame['stage'], { label: string; tone: 'neutral' | 'brand' | 'sky' | 'amber' }> = {
  idea: { label: 'Idea', tone: 'neutral' },
  building: { label: 'Building', tone: 'sky' },
  testing: { label: 'Testing', tone: 'amber' },
  ready: { label: 'Ready to play', tone: 'brand' },
  published: { label: 'Published', tone: 'brand' },
}

/** A game the team made: preview picture, stage, progress, and the Play / Feedback / Publish buttons. */
export function GameCard({ g }: { g: MyGame }) {
  const st = STAGE[g.stage]
  const done = g.stage === 'ready' || g.stage === 'published'
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
        {!done ? (
          <div className="mt-3">
            <Progress value={g.progress} label={g.stage_text} />
          </div>
        ) : (
          <div className={cx('mt-3 flex gap-2')}>
            <Button size="sm" kind="primary" icon={<Play className="size-3.5" />}>
              Play
            </Button>
            <Button size="sm" icon={<MessageSquare className="size-3.5" />}>
              Feedback
            </Button>
            {g.stage === 'ready' && (
              <Button size="sm" icon={<Upload className="size-3.5" />}>
                Publish
              </Button>
            )}
          </div>
        )}
      </div>
    </div>
  )
}
