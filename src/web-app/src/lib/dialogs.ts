import { createContext } from 'react'

export interface Dialogs {
  notice(text: string, body?: string): Promise<void>
  confirm(text: string): Promise<boolean>
  password(title: string, body?: string): Promise<string | null>
}

const joined = (text: string, body?: string) => (body === undefined ? text : `${text}\n\n${body}`)

export const DialogScope = createContext<Dialogs>({
  notice: async (text, body) => window.alert(joined(text, body)),
  confirm: async (text) => window.confirm(text),
  password: async (title, body) => window.prompt(joined(title, body)),
})
