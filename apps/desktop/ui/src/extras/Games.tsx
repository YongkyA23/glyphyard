// The version number in Settings; three quick clicks reveal the hidden games.
import { useState } from 'react'
import { Dialog } from '../ui'
import { playful, useTaps } from './effects'
import { KernIt } from './KernIt'
import { NameThatFont } from './NameThatFont'

export function VersionGames({ version }: { version: string }) {
  const [shown, setShown] = useState(false)
  const [game, setGame] = useState<'kern' | 'name' | null>(null)
  const tap = useTaps(3, () => void playful().then((on) => on && setShown(true)), true)
  return (
    <span className="egg-games">
      <button type="button" className="egg-plain" onClick={tap}>Typeshelf {version}</button>
      {shown && (
        <>
          {' · Games: '}
          <button type="button" className="egg-plain egg-link" onClick={() => setGame('kern')}>Kern it</button>
          {', '}
          <button type="button" className="egg-plain egg-link" onClick={() => setGame('name')}>Name that font</button>
        </>
      )}
      <Dialog open={game === 'kern'} onClose={() => setGame(null)} title="Kern it" wide><KernIt /></Dialog>
      <Dialog open={game === 'name'} onClose={() => setGame(null)} title="Name that font" wide><NameThatFont /></Dialog>
    </span>
  )
}
