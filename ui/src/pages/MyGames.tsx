import { Camera, Gamepad2, MessageSquare, Play, Upload } from 'lucide-react'
import { Card, Empty, PageHeader } from '../components/ui'

/** Finished games from the AI team land here (milestone M3+). */
export default function MyGames() {
  const steps = [
    { icon: Camera, title: 'Preview', text: 'Screenshots of every finished game, taken out of sight.' },
    { icon: Play, title: 'Play', text: 'One click opens a private test copy only you can join.' },
    { icon: MessageSquare, title: 'Feedback', text: 'Write what to change. The team fixes it on its own.' },
    { icon: Upload, title: 'Publish', text: 'The only button you ever need to press.' },
  ]
  return (
    <>
      <PageHeader title="My Games" subtitle="Games your AI team has built for you." />
      <Empty
        icon={<Gamepad2 className="size-6" />}
        title="No games yet"
        text="Your team starts building once it has learned enough from the charts. Each finished game will show up here."
      />
      <div className="mt-6 grid grid-cols-4 gap-4">
        {steps.map((s, i) => (
          <Card key={s.title} className="p-5">
            <div className="flex items-center gap-2.5">
              <div className="grid size-9 place-items-center rounded-xl bg-card-2 text-brand">
                <s.icon className="size-[18px]" />
              </div>
              <span className="text-[11px] font-semibold uppercase tracking-wider text-mute">Step {i + 1}</span>
            </div>
            <div className="mt-3 text-[14px] font-semibold">{s.title}</div>
            <div className="mt-1 text-[12.5px] leading-relaxed text-mute">{s.text}</div>
          </Card>
        ))}
      </div>
    </>
  )
}
