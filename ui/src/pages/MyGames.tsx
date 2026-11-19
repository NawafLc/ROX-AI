import { MyGame, useApi } from '../lib/api'
import { PageHeader, Skeleton } from '../components/ui'
import { GameCard } from '../components/GameCard'
import { FirstGame } from './Home'

/** Every game the AI team has made for you. */
export default function MyGames() {
  const { data: games } = useApi<MyGame[]>('my_games', {}, 5000)
  const ready = (games ?? []).filter((g) => g.stage === 'ready').length
  return (
    <>
      <PageHeader title="My Games" subtitle={games?.length ? `${games.length} games · ${ready} ready to play` : 'Games your AI team has built for you.'} />
      {!games ? (
        <div className="grid grid-cols-3 gap-4">{[0, 1, 2].map((i) => <Skeleton key={i} className="h-64" />)}</div>
      ) : games.length === 0 ? (
        <FirstGame />
      ) : (
        <div className="grid grid-cols-3 gap-4">
          {games.map((g) => (
            <GameCard key={g.id} g={g} />
          ))}
        </div>
      )}
    </>
  )
}
