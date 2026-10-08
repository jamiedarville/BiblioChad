export interface Toast {
  id: number
  kind: 'info' | 'error' | 'success'
  message: string
  action?: { label: string; run: () => void }
}

class Toasts {
  items = $state<Toast[]>([])
  #next = 1

  push(kind: Toast['kind'], message: string, action?: Toast['action'], ms = kind === 'error' ? 8000 : 4000) {
    const id = this.#next++
    this.items.push({ id, kind, message, action })
    if (ms > 0) setTimeout(() => this.dismiss(id), ms)
    return id
  }

  dismiss(id: number) {
    this.items = this.items.filter((t) => t.id !== id)
  }
}

export const toasts = new Toasts()
