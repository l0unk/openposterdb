// louk-fork: small formatting helpers shared by the Library and Title views.

/** "just now", "5m ago", "3h ago", "12d ago" for a unix-seconds timestamp. */
export function relativeTime(epochSeconds: number, nowMs: number = Date.now()): string {
  const diff = nowMs / 1000 - epochSeconds
  if (diff < 60) return 'just now'
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`
  if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`
  return `${Math.floor(diff / 86400)}d ago`
}

export function formatDate(epochSeconds: number): string {
  return new Date(epochSeconds * 1000).toLocaleDateString()
}
