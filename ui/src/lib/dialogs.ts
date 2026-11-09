// Desktop dialogs: WebView2 and WebKitGTK both implement window.confirm
// and window.prompt, so no platform branch is needed.
export async function confirmAction(message: string): Promise<boolean> {
  return window.confirm(message)
}

export async function askText(title: string, value = ''): Promise<string | null> {
  return window.prompt(title, value)
}
