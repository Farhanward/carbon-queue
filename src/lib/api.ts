import { invoke } from '@tauri-apps/api/core'

export type LicenseStatus = {
  active: boolean
  plan: string
  product: string
  device_bound: boolean
  expires_at?: string | null
}

export type LedgerDashboard = {
  today_sales: number
  today_expenses: number
  today_net: number
  open_day: string
  rows: LedgerRow[]
  closings: ClosingRow[]
}

export type LedgerRow = {
  id: number
  kind: 'sale' | 'expense'
  amount: number
  category: string
  note: string
  created_at: string
}

export type ClosingRow = {
  id: number
  date: string
  total_sales: number
  total_expenses: number
  net: number
  locked: boolean
}

export type QueueDashboard = {
  current_number?: string | null
  waiting: number
  served_today: number
  patients: PatientRow[]
}

export type PatientRow = {
  id: number
  ticket: string
  name: string
  phone: string
  status: 'waiting' | 'called' | 'served' | 'cancelled'
  created_at: string
}

export type AiDashboard = {
  documents: number
  chunks: number
  last_answer?: string | null
}

export type Bootstrap = {
  device_fingerprint: string
  license: LicenseStatus
  ledger: LedgerDashboard
  queue: QueueDashboard
  ai: AiDashboard
}

export type IntegrationSettings = {
  openai_model: string
  openai_key_set: boolean
  supabase_url: string
  unifonic_sender: string
  update_channel: string
}

export type AiAnswer = {
  answer: string
  sources: string[]
  used_openai: boolean
}

export const api = {
  bootstrap: () => invoke<Bootstrap>('get_bootstrap'),
  activateLicense: (key: string, product: string) =>
    invoke<LicenseStatus>('activate_license', { key, product }),
  settings: () => invoke<IntegrationSettings>('get_settings'),
  saveSettings: (input: Record<string, unknown>) => invoke<IntegrationSettings>('save_settings', { input }),
  addSale: (amount: number, category: string, note: string) =>
    invoke<LedgerDashboard>('ledger_add_sale', { amount, category, note }),
  addExpense: (amount: number, category: string, note: string) =>
    invoke<LedgerDashboard>('ledger_add_expense', { amount, category, note }),
  closeDay: () => invoke<LedgerDashboard>('ledger_close_day'),
  addPatient: (name: string, phone: string) => invoke<QueueDashboard>('queue_add_patient', { name, phone }),
  callNext: () => invoke<QueueDashboard>('queue_call_next'),
  markServed: (id: number) => invoke<QueueDashboard>('queue_mark_served', { id }),
  importText: (title: string, content: string) => invoke<AiDashboard>('ai_import_text', { title, content }),
  importFile: (path: string) => invoke<AiDashboard>('ai_import_file', { path }),
  askAi: (question: string) => invoke<AiAnswer>('ai_ask', { question }),
  updateStatus: () => invoke<string>('check_update_status'),
}
