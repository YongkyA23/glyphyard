import '@fontsource-variable/geist'
import '@fontsource/geist-mono'
import '@fontsource/instrument-serif'
import '@fontsource/instrument-serif/400-italic.css'
import '../../../web/shared/tokens.css'
import './app.css'
import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { BrowserRouter, Route, Routes } from 'react-router'
import { MotionConfig } from 'motion/react'
import { Layout } from './ui'
import { ErrorBoundary } from './ErrorBoundary'
import { Toaster } from './toast'
import { Login } from './pages/Login'
import { Overview } from './pages/Overview'
import { Users } from './pages/Users'
import { UserDetail } from './pages/UserDetail'
import { Merge } from './pages/Merge'
import { Names } from './pages/Names'
import { Library } from './pages/Library'
import { Organize } from './pages/Organize'
import { SettingsPage } from './pages/Settings'
import { Requests } from './pages/Requests'
import { Audit } from './pages/Audit'

// Dev-only fixture mode: open any page with ?mock=1 (sticks for the tab session, ?mock=0 turns it off).
if (import.meta.env.DEV) {
  const m = new URLSearchParams(location.search).get('mock')
  if (m !== null) sessionStorage.setItem('mock', m)
  if (sessionStorage.getItem('mock') === '1') await import('./mock')
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <MotionConfig reducedMotion="user">
      <BrowserRouter>
        <Routes>
          <Route path="/login" element={<ErrorBoundary><Login /></ErrorBoundary>} />
          <Route element={<Layout />}>
            <Route index element={<Overview />} />
            <Route path="users" element={<Users />} />
            <Route path="users/merge" element={<Merge />} />
            <Route path="users/:id" element={<UserDetail />} />
            <Route path="names" element={<Names />} />
            <Route path="library" element={<Library />} />
            <Route path="organize" element={<Organize />} />
            <Route path="requests" element={<Requests />} />
            <Route path="settings" element={<SettingsPage />} />
            <Route path="audit" element={<Audit />} />
            <Route path="*" element={<p className="empty">Page not found.</p>} />
          </Route>
        </Routes>
        <Toaster />
      </BrowserRouter>
    </MotionConfig>
  </StrictMode>,
)
