// The guided tours. English text marked with tk() (translated where it's rendered, through t()).
// Targets are `data-tour` anchors; a missing one centres the popover instead.
import { tk } from '../i18n'

export type GuideId = 'welcome' | 'add-fonts' | 'activate' | 'collections' | 'project-sets' | 'quick' | 'connect'
export type Step = { target?: string; route?: string; title: string; body: string; placement?: 'top' | 'bottom' | 'left' | 'right' | 'auto' }
export type Guide = { id: GuideId; title: string; summary: string; steps: Step[] }

export const GUIDES: Guide[] = [
  {
    id: 'welcome',
    title: tk('Welcome tour'),
    summary: tk('A quick look around Glyphyard.'),
    steps: [
      { target: 'sidebar', route: '/', placement: 'right', title: tk('Your sidebar'), body: tk('Everything starts here: your library, folders, collections and tools.') },
      { target: 'nav-library', route: '/', placement: 'right', title: tk('Library'), body: tk('All your fonts in one place. Come back here any time.') },
      { target: 'library-search', route: '/', placement: 'bottom', title: tk('Search'), body: tk('Type a name to find a font fast.') },
      { target: 'font-list', route: '/', placement: 'top', title: tk('Your fonts'), body: tk('Each row is a font family. Open one to see its styles and details.') },
      { target: 'font-switch', route: '/', placement: 'left', title: tk('Turn fonts on and off'), body: tk('Flip the switch to turn a family on. Your design apps see it right away.') },
      { target: 'folders', placement: 'right', title: tk('Folders'), body: tk('The folders your fonts come from. Pick one to see only its fonts.') },
      { target: 'collections-add', placement: 'right', title: tk('Collections'), body: tk('Make a collection to group fonts for a client, a project or a mood.') },
      { target: 'settings-guides', route: '/settings', placement: 'top', title: tk('Guides'), body: tk('Want a refresher? Every guide lives here in Settings, ready to replay.') },
    ],
  },
  {
    id: 'add-fonts',
    title: tk('Add fonts'),
    summary: tk('Bring font files and folders into your library.'),
    steps: [
      { target: 'library-add', route: '/', placement: 'bottom', title: tk('Add fonts'), body: tk('Add font files, copy a whole folder in, or watch a folder so new fonts show up on their own.') },
      { target: 'folders-add', placement: 'right', title: tk('From the sidebar'), body: tk('The + next to Folders does the same, from any page.') },
      { target: 'folders', placement: 'right', title: tk('Watched folders'), body: tk('Your folders are listed here. A filled dot means all of a folder’s fonts are on, a ring means some are.') },
      { title: tk('Drag and drop'), body: tk('You can also drop font files or folders anywhere on the window.') },
    ],
  },
  {
    id: 'activate',
    title: tk('Turn fonts on and off'),
    summary: tk('Make fonts available to your apps, or put them away.'),
    steps: [
      { target: 'font-switch', route: '/', placement: 'left', title: tk('One switch per family'), body: tk('Turn a whole family on or off with its switch.') },
      { target: 'font-list', route: '/', placement: 'top', title: tk('Single styles'), body: tk('Open a family to switch single styles on or off.') },
      { title: tk('Many at once'), body: tk('Select several families, then use the selection bar to turn them all on or off.') },
    ],
  },
  {
    id: 'collections',
    title: tk('Collections'),
    summary: tk('Group fonts the way you work.'),
    steps: [
      { target: 'collections-add', placement: 'right', title: tk('New collection'), body: tk('Make a collection for a client, a project or a mood.') },
      { title: tk('Fill it up'), body: tk('Drag families onto a collection in the sidebar to add them.') },
    ],
  },
  {
    id: 'project-sets',
    title: tk('Project sets'),
    summary: tk('Turn a whole job’s fonts on or off in one click.'),
    steps: [
      { target: 'sets-new', route: '/sets', placement: 'bottom', title: tk('New project set'), body: tk('Pick the fonts a job needs and save them as a set.') },
      { target: 'sets-list', route: '/sets', placement: 'top', title: tk('Your sets'), body: tk('One click turns all of a set’s fonts on or off.') },
    ],
  },
  {
    id: 'quick',
    title: tk('Quick activate'),
    summary: tk('Turn a font on without leaving your design app.'),
    steps: [
      { target: 'settings-shortcut', route: '/settings', placement: 'bottom', title: tk('Quick activate'), body: tk('Press this shortcut from anywhere to search your fonts and turn one on.') },
    ],
  },
  {
    id: 'connect',
    title: tk('Connect to your team’s server'),
    summary: tk('Get your team’s fonts and settings.'),
    steps: [
      {
        target: 'settings-connection', route: '/settings', placement: 'top', title: tk('Connection'),
        body: tk('Enter the server address and install key from your admin. If the connection is locked, your admin can unlock it with their unlock password.'),
      },
    ],
  },
]
