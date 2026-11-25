const TOKEN = /:(time|jam|date|tanggal|name|nama)\b/gi

export const hasTokens = (text: string) => (TOKEN.lastIndex = 0, TOKEN.test(text))

/** Replaces :time/:jam, :date/:tanggal and :name/:nama in what is shown; other :words and an empty name are left alone. */
export function applyTokens(text: string, now: Date, name: string): string {
  return text.replace(TOKEN, (m, word: string) => {
    switch (word.toLowerCase()) {
      case 'time': case 'jam': return now.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
      case 'date': case 'tanggal': return now.toLocaleDateString()
      default: return name || m
    }
  })
}
