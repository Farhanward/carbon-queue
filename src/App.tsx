import { useEffect, useState } from 'react'
import { CheckCircle2, KeyRound, PhoneCall, Plus, RefreshCw, ShieldCheck, UsersRound } from 'lucide-react'
import type { Bootstrap, IntegrationSettings, QueueDashboard } from './lib/api'
import { api } from './lib/api'
import './App.css'

type Locale = 'ar' | 'en'

const copy = {
  ar: {
    dir: 'rtl',
    language: 'English',
    title: 'CarbonQueue',
    subtitle: 'طابور العيادة لنظام Windows: تسجيل المرضى، شاشة انتظار، واستدعاء مع SMS عبر Unifonic.',
    active: 'مفعل',
    inactive: 'غير مفعل',
    placeholder: 'مفتاح التطوير: CF-DEMO-ALL',
    fingerprint: 'بصمة الجهاز',
    current: 'الرقم الحالي',
    waiting: 'بانتظار الدور',
    servedToday: 'خدموا اليوم',
    patient: 'اسم المريض',
    phone: 'رقم الجوال الدولي',
    add: 'إضافة مريض',
    call: 'استدعاء التالي',
    served: 'تمت الخدمة',
    settings: 'إعدادات SMS',
    appSid: 'Unifonic AppSid',
    sender: 'اسم المرسل',
    save: 'حفظ',
    live: 'قائمة الانتظار',
    loading: 'تحميل CarbonQueue...',
  },
  en: {
    dir: 'ltr',
    language: 'العربية',
    title: 'CarbonQueue',
    subtitle: 'Windows clinic queue: patient check-in, waiting display, call next, and Unifonic SMS alerts.',
    active: 'Active',
    inactive: 'Inactive',
    placeholder: 'Development key: CF-DEMO-ALL',
    fingerprint: 'Device fingerprint',
    current: 'Current ticket',
    waiting: 'Waiting',
    servedToday: 'Served today',
    patient: 'Patient name',
    phone: 'International mobile number',
    add: 'Add patient',
    call: 'Call next',
    served: 'Served',
    settings: 'SMS settings',
    appSid: 'Unifonic AppSid',
    sender: 'Sender name',
    save: 'Save',
    live: 'Live queue',
    loading: 'Loading CarbonQueue...',
  },
} as const

function App() {
  const [locale, setLocale] = useState<Locale>('ar')
  const [data, setData] = useState<Bootstrap | null>(null)
  const [settings, setSettings] = useState<IntegrationSettings | null>(null)
  const [licenseKey, setLicenseKey] = useState('')
  const [patient, setPatient] = useState({ name: '', phone: '' })
  const [sms, setSms] = useState({ unifonic_app_sid: '', unifonic_sender: 'CarbonQueue' })
  const [message, setMessage] = useState('')
  const [busy, setBusy] = useState(false)
  const t = copy[locale]

  useEffect(() => {
    api.bootstrap().then(setData).catch((error) => setMessage(String(error)))
    api.settings().then((next) => {
      setSettings(next)
      setSms({ unifonic_app_sid: '', unifonic_sender: next.unifonic_sender || 'CarbonQueue' })
    }).catch((error) => setMessage(String(error)))
  }, [])

  async function run(work: () => Promise<QueueDashboard>) {
    setBusy(true)
    setMessage('')
    try {
      const queue = await work()
      setData((current) => current ? { ...current, queue } : current)
      setPatient({ name: '', phone: '' })
    } catch (error) {
      setMessage(String(error))
    } finally {
      setBusy(false)
    }
  }

  async function activate() {
    setBusy(true)
    try {
      const license = await api.activateLicense(licenseKey, 'queue')
      setData((current) => current ? { ...current, license } : current)
      setLicenseKey('')
    } catch (error) {
      setMessage(String(error))
    } finally {
      setBusy(false)
    }
  }

  async function saveSms() {
    const next = await api.saveSettings({
      openai_model: settings?.openai_model ?? 'gpt-5-mini',
      supabase_url: settings?.supabase_url ?? '',
      unifonic_sender: sms.unifonic_sender,
      unifonic_app_sid: sms.unifonic_app_sid,
      update_channel: settings?.update_channel ?? 'stable',
    })
    setSettings(next)
  }

  if (!data) {
    return <main className="app-shell single" dir={t.dir} lang={locale}><div className="empty-state">{t.loading}</div></main>
  }

  return (
    <main className="app-shell single" dir={t.dir} lang={locale}>
      <section className="workspace">
        <header className="module-header">
          <div className="module-icon"><UsersRound /></div>
          <div>
            <h2>{t.title}</h2>
            <p>{t.subtitle}</p>
          </div>
          <button onClick={() => setLocale(locale === 'ar' ? 'en' : 'ar')}>{t.language}</button>
        </header>

        {message && <div className="notice">{message}</div>}

        <article className="license-panel top-license">
          <div className={data.license.active ? 'status status--ok' : 'status'}>
            <ShieldCheck size={18} />
            {data.license.active ? t.active : t.inactive}
          </div>
          <label>{t.fingerprint}<input readOnly value={data.device_fingerprint} /></label>
          <div className="license-row">
            <input value={licenseKey} onChange={(event) => setLicenseKey(event.target.value)} placeholder={t.placeholder} />
            <button onClick={activate} disabled={busy || !licenseKey}><KeyRound size={18} /></button>
          </div>
        </article>

        <div className="metric-grid">
          <Metric label={t.current} value={data.queue.current_number ?? '-'} />
          <Metric label={t.waiting} value={String(data.queue.waiting)} />
          <Metric label={t.servedToday} value={String(data.queue.served_today)} />
        </div>

        <div className="split-grid">
          <Panel title={t.add}>
            <div className="form-grid">
              <input value={patient.name} onChange={(event) => setPatient({ ...patient, name: event.target.value })} placeholder={t.patient} />
              <input value={patient.phone} onChange={(event) => setPatient({ ...patient, phone: event.target.value })} placeholder={t.phone} />
              <button onClick={() => run(() => api.addPatient(patient.name, patient.phone))} disabled={busy}><Plus size={18} />{t.add}</button>
              <button className="dark" onClick={() => run(api.callNext)} disabled={busy}><PhoneCall size={18} />{t.call}</button>
            </div>
          </Panel>
          <Panel title={t.settings}>
            <div className="form-grid">
              <input value={sms.unifonic_app_sid} onChange={(event) => setSms({ ...sms, unifonic_app_sid: event.target.value })} placeholder={t.appSid} type="password" />
              <input value={sms.unifonic_sender} onChange={(event) => setSms({ ...sms, unifonic_sender: event.target.value })} placeholder={t.sender} />
              <button className="dark" onClick={saveSms} disabled={busy}><CheckCircle2 size={18} />{t.save}</button>
            </div>
          </Panel>
        </div>

        <Panel title={t.live}>
          <DataList rows={data.queue.patients.map((row) => [row.ticket, row.name, row.phone, row.status, <button className="mini" onClick={() => run(() => api.markServed(row.id))}>{t.served}</button>])} />
        </Panel>

        <button className="floating-sync" onClick={() => api.bootstrap().then(setData)}><RefreshCw size={18} /></button>
      </section>
    </main>
  )
}

function Metric({ label, value }: { label: string; value: string }) {
  return <article className="metric"><span>{label}</span><strong>{value}</strong></article>
}

function Panel({ title, children }: { title: string; children: React.ReactNode }) {
  return <article className="panel"><h3>{title}</h3>{children}</article>
}

function DataList({ rows }: { rows: Array<Array<React.ReactNode>> }) {
  if (!rows.length) return <div className="empty-state">No records yet</div>
  return <div className="data-list">{rows.map((row, index) => <div className="data-row" key={index}>{row.map((cell, cellIndex) => <span key={cellIndex}>{cell}</span>)}</div>)}</div>
}

export default App
