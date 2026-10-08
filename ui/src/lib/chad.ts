// Chad's voice. Jokes live in copy and empty states only; anything that
// blocks the user from fixing a problem stays plain (see project.md §13).

import type { Rank, Stats } from './api'

const pick = <T,>(xs: T[]): T => xs[Math.floor(Math.random() * xs.length)]

export function greeting(chad: boolean, streak: number): string {
  if (!chad) return 'Library'
  if (streak >= 7) return `${streak}-day streak. Chad salutes you.`
  if (streak >= 2) return `${streak} days in a row. Keep lifting.`
  return pick(['Finish the book.', 'Your Drive, but jacked.', 'Reads more than you.', 'Welcome back, scholar.'])
}

export function emptyLibrary(chad: boolean, connected: boolean): { title: string; body: string } {
  if (!connected) {
    return chad
      ? { title: "No books yet.", body: "Connect your Drive and let's get lifting. Or import a few files to warm up." }
      : { title: 'No books yet', body: 'Connect Google Drive or import EPUB and PDF files.' }
  }
  return chad
    ? { title: 'This shelf is empty.', body: 'Even Chad needs something to read. Drop EPUBs or PDFs into this Drive folder.' }
    : { title: 'Empty folder', body: 'Add EPUB or PDF files to this Drive folder.' }
}

export function resumeLine(chad: boolean, label: string): string {
  return chad
    ? `You were on ${label} on another device. Chad remembers. Chad always remembers.`
    : `Continue from ${label} on your other device?`
}

export function loadingLine(chad: boolean): string {
  if (!chad) return 'Opening…'
  return pick(['Cracking the spine…', 'Stretching before the first chapter…', 'Fetching from the cloud gym…', 'Warming up the eyes…'])
}

export function finishedLine(chad: boolean, title: string): string {
  return chad ? `Finished “${title}”. Another one for the trophy shelf.` : `Marked “${title}” as finished.`
}

export function searchEmpty(chad: boolean): string {
  return chad ? 'Nothing. Not even Chad could find it.' : 'No matches.'
}

export function rankProgress(rank: Rank, finished: number): number {
  if (rank.next_threshold == null) return 1
  const span = rank.next_threshold - rank.threshold
  return Math.min(1, Math.max(0, (finished - rank.threshold) / span))
}

export interface Achievement {
  id: string
  name: string
  description: string
  earned: boolean
}

export function achievements(s: Stats): Achievement[] {
  return [
    { id: 'first', name: 'First Rep', description: 'Finish your first book.', earned: s.finished >= 1 },
    { id: 'ten', name: 'Tome Raider', description: 'Finish ten books.', earned: s.finished >= 10 },
    { id: 'streak7', name: 'Seven-Day Streak', description: 'Read seven days in a row.', earned: s.streak_days >= 7 },
    {
      id: 'cleared',
      name: 'Cleared the Unread Pile',
      description: 'Finish every book in your library.',
      earned: s.total_books > 0 && s.finished >= s.total_books,
    },
  ]
}
