import { BookOpen, FolderOpen, GraduationCap } from 'lucide-react'
import { Button, Empty, PageHeader } from '../components/ui'
import { call, go } from '../lib/api'

/** The new Playbook. It is separate from ROX-AI's old one and starts empty; it grows from studied games. */
export function PlaybookPage() {
  return (
    <div className="fade-in">
      <PageHeader
        title="Playbook"
        subtitle="Your new Playbook, built only from games you study here. It's separate from ROX-AI's old Playbook."
        actions={
          <Button icon={<FolderOpen className="size-4" />} onClick={() => call('open', { what: 'vault' })}>
            Open vault
          </Button>
        }
      />
      <Empty
        icon={<BookOpen className="size-5" />}
        title="The Playbook starts empty"
        text="Each game you study adds a full analysis under Studied. Once a few games are studied, their lessons are combined into Playbook pages here."
        action={
          <Button kind="primary" icon={<GraduationCap className="size-4" />} onClick={() => go('#/studied')}>
            See studied games
          </Button>
        }
      />
    </div>
  )
}
