import '@fontsource-variable/geist'
import '@fontsource/geist-mono'
import '@fontsource/instrument-serif'
import '@fontsource/instrument-serif/400-italic.css'
import '../../../../web/shared/tokens.css'
import './app.css'
import './theme'
import { Fragment, StrictMode, type ReactNode } from 'react'
import { createRoot } from 'react-dom/client'
import { motion, MotionConfig } from 'motion/react'
import { BrowserRouter, Navigate, Route, Routes, useLocation } from 'react-router'
import { AppProvider, useApp } from './state'
import { Shell } from './Shell'
import { Onboarding } from './pages/Onboarding'
import { Library } from './pages/Library'
import { FamilyPage } from './pages/Family'
import { Playground } from './pages/Playground'
import { ProjectSets } from './pages/ProjectSets'
import { Organize } from './pages/Organize'
import { PdfFonts } from './pages/PdfFonts'
import { Stats } from './pages/Stats'
import { GoogleFonts } from './pages/GoogleFonts'
import { SettingsPage } from './pages/Settings'
import { Quick } from './pages/Quick'
import { PrintCollection } from './pages/Print'
import { pop, spring } from './motion'
import { ErrorBoundary } from './ErrorBoundary'
import { useLang } from './i18n'
import { TourProvider } from './tour/Tour'

/** Remounts the routed content when the language changes, so nothing keeps stale strings. */
// oxlint-disable-next-line react/only-export-components -- the entry file exports nothing
function ByLang({ children }: { children: ReactNode }) {
  return <Fragment key={useLang()}>{children}</Fragment>
}

function MainRoutes() {
  const { app } = useApp()
  const { pathname } = useLocation()
  if (!app.onboarded && pathname !== '/onboarding') return <Navigate to="/onboarding" replace />
  return (
    <Routes>
      <Route path="/onboarding" element={app.onboarded ? <Navigate to="/" replace /> : <Onboarding />} />
      <Route path="/print/collection/:id" element={<PrintCollection />} />
      <Route element={<Shell />}>
        <Route index element={<Library />} />
        <Route path="/family/:key" element={<FamilyPage />} />
        <Route path="/playground" element={<Playground />} />
        <Route path="/sets" element={<ProjectSets />} />
        <Route path="/organize" element={<Organize />} />
        <Route path="/pdf" element={<PdfFonts />} />
        <Route path="/stats" element={<Stats />} />
        <Route path="/google" element={<GoogleFonts />} />
        <Route path="/settings" element={<SettingsPage />} />
        <Route path="*" element={<Navigate to="/" replace />} />
      </Route>
    </Routes>
  )
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <MotionConfig reducedMotion="user">
      <BrowserRouter>
        <Routes>
          <Route
            path="/quick"
            element={<AppProvider main={false}><motion.div style={{ height: '100%' }} {...pop} initial={{ opacity: 0, scale: 0.94 }} transition={spring.panel}><ByLang><ErrorBoundary><Quick /></ErrorBoundary></ByLang></motion.div></AppProvider>}
          />
          <Route path="*" element={<AppProvider main><TourProvider><ByLang><ErrorBoundary><MainRoutes /></ErrorBoundary></ByLang></TourProvider></AppProvider>} />
        </Routes>
      </BrowserRouter>
    </MotionConfig>
  </StrictMode>,
)
