import { Fragment, useId, type ReactNode } from 'react'
import { getLangSetting, setLangSetting, t, useLang, type LangSetting } from '.'

export function LanguageSelect({ className = 'lang-select' }: { className?: string }) {
  useLang() // re-render when the language changes so the labels follow it
  const id = useId()
  return (
    <span className="lang-field">
      <label htmlFor={id} className="lang-label">{t('Language')}</label>
      <select id={id} className={className} value={getLangSetting()} onChange={(e) => setLangSetting(e.target.value as LangSetting)}>
        <option value="system">{t('Match system')}</option>
        <option value="en">English</option>
        <option value="id">Bahasa Indonesia</option>
      </select>
    </span>
  )
}

/** A language change remounts its children, so every t() re-runs. */
export const Remount = ({ children }: { children: ReactNode }) => <Fragment key={useLang()}>{children}</Fragment>
