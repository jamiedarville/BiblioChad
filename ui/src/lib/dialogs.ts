import { confirm as nativeConfirm } from '@tauri-apps/plugin-dialog'
import { isAndroid } from './platform'

export async function confirmAction(message: string): Promise<boolean> {
  return isAndroid ? nativeConfirm(message, { title: 'BiblioChad', kind: 'warning' }) : window.confirm(message)
}

// Android WebViews do not always implement JavaScript prompt dialogs. This
// dialog also gives text entry the correct focus, keyboard and cancel behavior.
export async function askText(title: string, value = ''): Promise<string | null> {
  if (!isAndroid) return window.prompt(title, value)
  return new Promise((resolve) => {
    const dialog = document.createElement('dialog')
    dialog.className = 'text-prompt card'
    const form = document.createElement('form')
    const label = document.createElement('label')
    label.textContent = title
    const field = document.createElement('input')
    field.value = value
    field.setAttribute('aria-label', title)
    label.append(field)
    const controls = document.createElement('div')
    controls.className = 'row'
    const cancel = document.createElement('button')
    cancel.type = 'button'
    cancel.textContent = 'Cancel'
    const save = document.createElement('button')
    save.type = 'submit'
    save.className = 'primary'
    save.textContent = 'Save'
    let result: string | null = null
    cancel.onclick = () => dialog.close()
    form.onsubmit = (e) => { e.preventDefault(); result = field.value; dialog.close() }
    dialog.onclose = () => { dialog.remove(); resolve(result) }
    controls.append(cancel, save)
    form.append(label, controls)
    dialog.append(form)
    document.body.append(dialog)
    dialog.showModal()
    field.focus()
  })
}
