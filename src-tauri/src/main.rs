use chrono::Local;
use regex::Regex;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    env,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::Mutex,
};
use tauri::{Manager, State};
use winreg::{enums::HKEY_LOCAL_MACHINE, RegKey};
use zip::ZipArchive;

struct AppState {
    db: Mutex<Connection>,
    device_fingerprint: String,
}

#[derive(Debug, thiserror::Error)]
enum AppError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("zip error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("{0}")]
    Message(String),
}

impl serde::Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

type AppResult<T> = Result<T, AppError>;

#[derive(Serialize)]
struct Bootstrap {
    device_fingerprint: String,
    license: LicenseStatus,
    ledger: LedgerDashboard,
    queue: QueueDashboard,
    ai: AiDashboard,
}

#[derive(Serialize)]
struct LicenseStatus {
    active: bool,
    plan: String,
    product: String,
    device_bound: bool,
    expires_at: Option<String>,
}

#[derive(Serialize)]
struct LedgerDashboard {
    today_sales: f64,
    today_expenses: f64,
    today_net: f64,
    open_day: String,
    rows: Vec<LedgerRow>,
    closings: Vec<ClosingRow>,
}

#[derive(Serialize)]
struct LedgerRow {
    id: i64,
    kind: String,
    amount: f64,
    category: String,
    note: String,
    created_at: String,
}

#[derive(Serialize)]
struct ClosingRow {
    id: i64,
    date: String,
    total_sales: f64,
    total_expenses: f64,
    net: f64,
    locked: bool,
}

#[derive(Serialize)]
struct QueueDashboard {
    current_number: Option<String>,
    waiting: i64,
    served_today: i64,
    patients: Vec<PatientRow>,
}

#[derive(Serialize)]
struct PatientRow {
    id: i64,
    ticket: String,
    name: String,
    phone: String,
    status: String,
    created_at: String,
}

#[derive(Serialize)]
struct AiDashboard {
    documents: i64,
    chunks: i64,
    last_answer: Option<String>,
}

#[derive(Serialize)]
struct IntegrationSettings {
    openai_model: String,
    openai_key_set: bool,
    supabase_url: String,
    unifonic_sender: String,
    update_channel: String,
}

#[derive(Deserialize)]
struct SettingsInput {
    openai_api_key: Option<String>,
    openai_model: String,
    supabase_url: String,
    supabase_anon_key: Option<String>,
    unifonic_app_sid: Option<String>,
    unifonic_sender: String,
    update_channel: String,
}

#[derive(Serialize)]
struct AiAnswer {
    answer: String,
    sources: Vec<String>,
    used_openai: bool,
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let db_path = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join("carbonqueue.db");
            if let Some(parent) = db_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let connection = Connection::open(db_path)?;
            migrate(&connection)?;
            let device_fingerprint = device_fingerprint();
            app.manage(AppState {
                db: Mutex::new(connection),
                device_fingerprint,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_bootstrap,
            activate_license,
            get_settings,
            save_settings,
            ledger_add_sale,
            ledger_add_expense,
            ledger_close_day,
            ledger_dashboard,
            queue_add_patient,
            queue_call_next,
            queue_mark_served,
            queue_dashboard,
            ai_import_text,
            ai_import_file,
            ai_ask,
            ai_dashboard,
            check_update_status
        ])
        .run(tauri::generate_context!())
        .expect("error while running CarbonQueue");
}

fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "
        PRAGMA journal_mode = WAL;
        PRAGMA foreign_keys = ON;

        CREATE TABLE IF NOT EXISTS license (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            key TEXT NOT NULL,
            product TEXT NOT NULL,
            plan TEXT NOT NULL,
            expires_at TEXT,
            device_hash TEXT NOT NULL,
            activated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS ledger_entries (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            kind TEXT NOT NULL CHECK (kind IN ('sale', 'expense')),
            amount REAL NOT NULL CHECK (amount >= 0),
            category TEXT NOT NULL,
            note TEXT NOT NULL,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS ledger_closings (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            date TEXT NOT NULL UNIQUE,
            total_sales REAL NOT NULL,
            total_expenses REAL NOT NULL,
            net REAL NOT NULL,
            locked INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS queue_patients (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            ticket TEXT NOT NULL UNIQUE,
            name TEXT NOT NULL,
            phone TEXT NOT NULL,
            status TEXT NOT NULL CHECK (status IN ('waiting', 'called', 'served', 'cancelled')),
            created_at TEXT NOT NULL,
            called_at TEXT,
            served_at TEXT
        );

        CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS ai_documents (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            title TEXT NOT NULL,
            source TEXT NOT NULL,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS ai_chunks (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            document_id INTEGER NOT NULL REFERENCES ai_documents(id) ON DELETE CASCADE,
            chunk_index INTEGER NOT NULL,
            content TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS ai_answers (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            question TEXT NOT NULL,
            answer TEXT NOT NULL,
            sources TEXT NOT NULL,
            created_at TEXT NOT NULL
        );
        ",
    )?;

    set_default(conn, "openai_model", "gpt-5-mini")?;
    set_default(conn, "supabase_url", "")?;
    set_default(conn, "supabase_anon_key", "")?;
    set_default(conn, "unifonic_app_sid", "")?;
    set_default(conn, "unifonic_sender", "CarbonFlow")?;
    set_default(conn, "update_channel", "stable")?;
    set_default(conn, "openai_api_key", "")?;
    Ok(())
}

fn set_default(conn: &Connection, key: &str, value: &str) -> AppResult<()> {
    conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES (?1, ?2)",
        params![key, value],
    )?;
    Ok(())
}

#[tauri::command]
fn get_bootstrap(state: State<AppState>) -> AppResult<Bootstrap> {
    let conn = state.db.lock().map_err(|_| AppError::Message("database lock failed".into()))?;
    Ok(Bootstrap {
        device_fingerprint: state.device_fingerprint.clone(),
        license: license_status(&conn, &state.device_fingerprint)?,
        ledger: ledger_dashboard_inner(&conn)?,
        queue: queue_dashboard_inner(&conn)?,
        ai: ai_dashboard_inner(&conn)?,
    })
}

#[tauri::command]
fn activate_license(key: String, product: String, state: State<AppState>) -> AppResult<LicenseStatus> {
    let normalized = key.trim().to_uppercase();
    let valid_demo = normalized == "CF-DEMO-ALL" || normalized == format!("CF-DEMO-{}", product.to_uppercase());
    let valid_signed_shape = normalized.starts_with("CF1.") && normalized.split('.').count() == 3;

    if !valid_demo && !valid_signed_shape {
        return Err(AppError::Message(
            "License rejected. Use CF-DEMO-ALL for development or provide a signed CF1 license.".into(),
        ));
    }

    let conn = state.db.lock().map_err(|_| AppError::Message("database lock failed".into()))?;
    let plan = if product == "ai" { "Premium" } else { "Standard" };
    conn.execute(
        "INSERT OR REPLACE INTO license (id, key, product, plan, expires_at, device_hash, activated_at)
         VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            normalized,
            product,
            plan,
            Option::<String>::None,
            state.device_fingerprint,
            now()
        ],
    )?;
    license_status(&conn, &state.device_fingerprint)
}

#[tauri::command]
fn get_settings(state: State<AppState>) -> AppResult<IntegrationSettings> {
    let conn = state.db.lock().map_err(|_| AppError::Message("database lock failed".into()))?;
    Ok(IntegrationSettings {
        openai_model: setting(&conn, "openai_model")?,
        openai_key_set: !setting(&conn, "openai_api_key")?.is_empty(),
        supabase_url: setting(&conn, "supabase_url")?,
        unifonic_sender: setting(&conn, "unifonic_sender")?,
        update_channel: setting(&conn, "update_channel")?,
    })
}

#[tauri::command]
fn save_settings(input: SettingsInput, state: State<AppState>) -> AppResult<IntegrationSettings> {
    let conn = state.db.lock().map_err(|_| AppError::Message("database lock failed".into()))?;
    upsert_setting(&conn, "openai_model", &input.openai_model)?;
    upsert_setting(&conn, "supabase_url", &input.supabase_url)?;
    upsert_setting(&conn, "unifonic_sender", &input.unifonic_sender)?;
    upsert_setting(&conn, "update_channel", &input.update_channel)?;
    if let Some(key) = input.openai_api_key {
        upsert_setting(&conn, "openai_api_key", key.trim())?;
    }
    if let Some(key) = input.supabase_anon_key {
        upsert_setting(&conn, "supabase_anon_key", key.trim())?;
    }
    if let Some(key) = input.unifonic_app_sid {
        upsert_setting(&conn, "unifonic_app_sid", key.trim())?;
    }
    drop(conn);
    get_settings(state)
}

#[tauri::command]
fn ledger_add_sale(amount: f64, category: String, note: String, state: State<AppState>) -> AppResult<LedgerDashboard> {
    ledger_insert("sale", amount, category, note, state)
}

#[tauri::command]
fn ledger_add_expense(amount: f64, category: String, note: String, state: State<AppState>) -> AppResult<LedgerDashboard> {
    ledger_insert("expense", amount, category, note, state)
}

fn ledger_insert(kind: &str, amount: f64, category: String, note: String, state: State<AppState>) -> AppResult<LedgerDashboard> {
    if amount <= 0.0 {
        return Err(AppError::Message("amount must be greater than zero".into()));
    }
    let conn = state.db.lock().map_err(|_| AppError::Message("database lock failed".into()))?;
    conn.execute(
        "INSERT INTO ledger_entries (kind, amount, category, note, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![kind, amount, clean(&category), clean(&note), now()],
    )?;
    ledger_dashboard_inner(&conn)
}

#[tauri::command]
fn ledger_close_day(state: State<AppState>) -> AppResult<LedgerDashboard> {
    let conn = state.db.lock().map_err(|_| AppError::Message("database lock failed".into()))?;
    let date = today();
    let (sales, expenses) = ledger_totals_for_today(&conn)?;
    conn.execute(
        "INSERT OR REPLACE INTO ledger_closings (date, total_sales, total_expenses, net, locked, created_at)
         VALUES (?1, ?2, ?3, ?4, 1, ?5)",
        params![date, sales, expenses, sales - expenses, now()],
    )?;
    ledger_dashboard_inner(&conn)
}

#[tauri::command]
fn ledger_dashboard(state: State<AppState>) -> AppResult<LedgerDashboard> {
    let conn = state.db.lock().map_err(|_| AppError::Message("database lock failed".into()))?;
    ledger_dashboard_inner(&conn)
}

#[tauri::command]
fn queue_add_patient(name: String, phone: String, state: State<AppState>) -> AppResult<QueueDashboard> {
    let name = clean(&name);
    if name.is_empty() {
        return Err(AppError::Message("patient name is required".into()));
    }
    let conn = state.db.lock().map_err(|_| AppError::Message("database lock failed".into()))?;
    let next: i64 = conn.query_row(
        "SELECT COALESCE(MAX(id), 0) + 1 FROM queue_patients WHERE date(created_at) = date('now', 'localtime')",
        [],
        |row| row.get(0),
    )?;
    let ticket = format!("A-{next:02}");
    conn.execute(
        "INSERT INTO queue_patients (ticket, name, phone, status, created_at) VALUES (?1, ?2, ?3, 'waiting', ?4)",
        params![ticket, name, clean(&phone), now()],
    )?;
    queue_dashboard_inner(&conn)
}

#[tauri::command]
async fn queue_call_next(state: State<'_, AppState>) -> AppResult<QueueDashboard> {
    let (sms_job, dashboard) = {
        let conn = state.db.lock().map_err(|_| AppError::Message("database lock failed".into()))?;
    conn.execute("UPDATE queue_patients SET status = 'waiting' WHERE status = 'called'", [])?;
        let next_row: Option<(i64, String, String)> = conn
        .query_row(
                "SELECT id, ticket, phone FROM queue_patients WHERE status = 'waiting' ORDER BY id ASC LIMIT 1",
            [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
        let mut sms_job = None;
        if let Some((id, ticket, phone)) = next_row {
        conn.execute(
            "UPDATE queue_patients SET status = 'called', called_at = ?1 WHERE id = ?2",
            params![now(), id],
        )?;
            let app_sid = setting(&conn, "unifonic_app_sid")?;
            let sender = setting(&conn, "unifonic_sender")?;
            if !app_sid.is_empty() && !phone.is_empty() {
                sms_job = Some((
                    app_sid,
                    sender,
                    phone,
                    format!("اقترب دورك في العيادة. رقمك الحالي: {ticket}"),
                ));
            }
        }
        (sms_job, queue_dashboard_inner(&conn)?)
    };

    if let Some((app_sid, sender, phone, body)) = sms_job {
        send_unifonic_sms(&app_sid, &sender, &phone, &body).await?;
    }
    Ok(dashboard)
}

#[tauri::command]
fn queue_mark_served(id: i64, state: State<AppState>) -> AppResult<QueueDashboard> {
    let conn = state.db.lock().map_err(|_| AppError::Message("database lock failed".into()))?;
    conn.execute(
        "UPDATE queue_patients SET status = 'served', served_at = ?1 WHERE id = ?2",
        params![now(), id],
    )?;
    queue_dashboard_inner(&conn)
}

#[tauri::command]
fn queue_dashboard(state: State<AppState>) -> AppResult<QueueDashboard> {
    let conn = state.db.lock().map_err(|_| AppError::Message("database lock failed".into()))?;
    queue_dashboard_inner(&conn)
}

#[tauri::command]
fn ai_import_text(title: String, content: String, state: State<AppState>) -> AppResult<AiDashboard> {
    import_document(&title, "manual", &content, state)
}

#[tauri::command]
fn ai_import_file(path: String, state: State<AppState>) -> AppResult<AiDashboard> {
    let path = PathBuf::from(path);
    let title = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Document")
        .to_string();
    let content = extract_text(&path)?;
    import_document(&title, &path.to_string_lossy(), &content, state)
}

fn import_document(title: &str, source: &str, content: &str, state: State<AppState>) -> AppResult<AiDashboard> {
    let text = clean_multiline(content);
    if text.len() < 20 {
        return Err(AppError::Message("document text is too short".into()));
    }
    let conn = state.db.lock().map_err(|_| AppError::Message("database lock failed".into()))?;
    conn.execute(
        "INSERT INTO ai_documents (title, source, created_at) VALUES (?1, ?2, ?3)",
        params![clean(title), source, now()],
    )?;
    let doc_id = conn.last_insert_rowid();
    for (index, chunk) in chunk_text(&text).iter().enumerate() {
        conn.execute(
            "INSERT INTO ai_chunks (document_id, chunk_index, content) VALUES (?1, ?2, ?3)",
            params![doc_id, index as i64, chunk],
        )?;
    }
    ai_dashboard_inner(&conn)
}

#[tauri::command]
async fn ai_ask(question: String, state: State<'_, AppState>) -> AppResult<AiAnswer> {
    let question = clean(&question);
    if question.is_empty() {
        return Err(AppError::Message("question is required".into()));
    }

    let (context, sources, api_key, model) = {
        let conn = state.db.lock().map_err(|_| AppError::Message("database lock failed".into()))?;
        let matches = retrieve_chunks(&conn, &question)?;
        let sources = matches.iter().map(|(title, _)| title.clone()).collect::<Vec<_>>();
        let context = matches
            .iter()
            .map(|(title, chunk)| format!("Source: {title}\n{chunk}"))
            .collect::<Vec<_>>()
            .join("\n\n---\n\n");
        (context, sources, setting(&conn, "openai_api_key")?, setting(&conn, "openai_model")?)
    };

    if context.is_empty() {
        return Ok(AiAnswer {
            answer: "لا توجد مستندات كافية للإجابة. أضف مستنداً أولاً.".into(),
            sources: vec![],
            used_openai: false,
        });
    }

    let (answer, used_openai) = if api_key.is_empty() {
        (
            format!(
                "إجابة محلية مقيدة بالمستندات:\n\n{}\n\nالسؤال: {}",
                context.lines().take(12).collect::<Vec<_>>().join("\n"),
                question
            ),
            false,
        )
    } else {
        (ask_openai(&api_key, &model, &question, &context).await?, true)
    };

    let conn = state.db.lock().map_err(|_| AppError::Message("database lock failed".into()))?;
    conn.execute(
        "INSERT INTO ai_answers (question, answer, sources, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![question, answer, serde_json::to_string(&sources).unwrap_or_default(), now()],
    )?;

    Ok(AiAnswer { answer, sources, used_openai })
}

#[tauri::command]
fn ai_dashboard(state: State<AppState>) -> AppResult<AiDashboard> {
    let conn = state.db.lock().map_err(|_| AppError::Message("database lock failed".into()))?;
    ai_dashboard_inner(&conn)
}

#[tauri::command]
fn check_update_status() -> String {
    "Updater is configured for signed Tauri artifacts. Set the production endpoint and public key before release.".into()
}

fn license_status(conn: &Connection, fingerprint: &str) -> AppResult<LicenseStatus> {
    let row = conn
        .query_row(
            "SELECT product, plan, expires_at, device_hash FROM license WHERE id = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()?;

    Ok(match row {
        Some((product, plan, expires_at, device_hash)) => LicenseStatus {
            active: device_hash == fingerprint,
            plan,
            product,
            device_bound: device_hash == fingerprint,
            expires_at,
        },
        None => LicenseStatus {
            active: false,
            plan: "Trial".into(),
            product: "none".into(),
            device_bound: false,
            expires_at: None,
        },
    })
}

fn ledger_dashboard_inner(conn: &Connection) -> AppResult<LedgerDashboard> {
    let (sales, expenses) = ledger_totals_for_today(conn)?;
    let mut stmt = conn.prepare(
        "SELECT id, kind, amount, category, note, created_at FROM ledger_entries ORDER BY id DESC LIMIT 12",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(LedgerRow {
                id: row.get(0)?,
                kind: row.get(1)?,
                amount: row.get(2)?,
                category: row.get(3)?,
                note: row.get(4)?,
                created_at: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut stmt = conn.prepare(
        "SELECT id, date, total_sales, total_expenses, net, locked FROM ledger_closings ORDER BY date DESC LIMIT 8",
    )?;
    let closings = stmt
        .query_map([], |row| {
            Ok(ClosingRow {
                id: row.get(0)?,
                date: row.get(1)?,
                total_sales: row.get(2)?,
                total_expenses: row.get(3)?,
                net: row.get(4)?,
                locked: row.get::<_, i64>(5)? == 1,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(LedgerDashboard {
        today_sales: sales,
        today_expenses: expenses,
        today_net: sales - expenses,
        open_day: today(),
        rows,
        closings,
    })
}

fn ledger_totals_for_today(conn: &Connection) -> AppResult<(f64, f64)> {
    let sales = ledger_sum(conn, "sale")?;
    let expenses = ledger_sum(conn, "expense")?;
    Ok((sales, expenses))
}

fn ledger_sum(conn: &Connection, kind: &str) -> AppResult<f64> {
    Ok(conn.query_row(
        "SELECT COALESCE(SUM(amount), 0) FROM ledger_entries WHERE kind = ?1 AND date(created_at) = date('now', 'localtime')",
        params![kind],
        |row| row.get(0),
    )?)
}

fn queue_dashboard_inner(conn: &Connection) -> AppResult<QueueDashboard> {
    let current_number = conn
        .query_row(
            "SELECT ticket FROM queue_patients WHERE status = 'called' ORDER BY called_at DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let waiting = conn.query_row(
        "SELECT COUNT(*) FROM queue_patients WHERE status = 'waiting'",
        [],
        |row| row.get(0),
    )?;
    let served_today = conn.query_row(
        "SELECT COUNT(*) FROM queue_patients WHERE status = 'served' AND date(served_at) = date('now', 'localtime')",
        [],
        |row| row.get(0),
    )?;
    let mut stmt = conn.prepare(
        "SELECT id, ticket, name, phone, status, created_at FROM queue_patients
         WHERE status IN ('waiting', 'called') ORDER BY id ASC LIMIT 16",
    )?;
    let patients = stmt
        .query_map([], |row| {
            Ok(PatientRow {
                id: row.get(0)?,
                ticket: row.get(1)?,
                name: row.get(2)?,
                phone: row.get(3)?,
                status: row.get(4)?,
                created_at: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(QueueDashboard {
        current_number,
        waiting,
        served_today,
        patients,
    })
}

fn ai_dashboard_inner(conn: &Connection) -> AppResult<AiDashboard> {
    let documents = conn.query_row("SELECT COUNT(*) FROM ai_documents", [], |row| row.get(0))?;
    let chunks = conn.query_row("SELECT COUNT(*) FROM ai_chunks", [], |row| row.get(0))?;
    let last_answer = conn
        .query_row(
            "SELECT answer FROM ai_answers ORDER BY id DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()?;
    Ok(AiDashboard {
        documents,
        chunks,
        last_answer,
    })
}

fn retrieve_chunks(conn: &Connection, question: &str) -> AppResult<Vec<(String, String)>> {
    let words = question
        .split_whitespace()
        .map(|word| word.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
        .filter(|word| word.len() > 2)
        .collect::<Vec<_>>();
    let mut stmt = conn.prepare(
        "SELECT d.title, c.content FROM ai_chunks c JOIN ai_documents d ON d.id = c.document_id",
    )?;
    let mut rows = stmt
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;

    rows.sort_by_key(|(_, content)| {
        let lowered = content.to_lowercase();
        let score = words.iter().filter(|word| lowered.contains(word.as_str())).count() as i64;
        -score
    });
    Ok(rows.into_iter().take(4).collect())
}

async fn ask_openai(api_key: &str, model: &str, question: &str, context: &str) -> AppResult<String> {
    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "model": model,
        "instructions": "Answer using only the provided customer documents. If the documents do not contain the answer, say that the answer is not available in the documents. Reply in the same language as the user.",
        "input": format!("Documents:\n{}\n\nQuestion:\n{}", context, question)
    });
    let value: serde_json::Value = client
        .post("https://api.openai.com/v1/responses")
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    if let Some(text) = value.get("output_text").and_then(|v| v.as_str()) {
        return Ok(text.to_string());
    }

    let text = value
        .get("output")
        .and_then(|v| v.as_array())
        .and_then(|items| {
            items.iter().find_map(|item| {
                item.get("content")
                    .and_then(|c| c.as_array())
                    .and_then(|content| {
                        content.iter().find_map(|part| part.get("text").and_then(|t| t.as_str()))
                    })
            })
        })
        .unwrap_or("No text response returned by OpenAI.");
    Ok(text.to_string())
}

async fn send_unifonic_sms(app_sid: &str, sender: &str, phone: &str, body: &str) -> AppResult<()> {
    let client = reqwest::Client::new();
    let response = client
        .post("https://el.cloud.unifonic.com/rest/SMS/messages")
        .header("Accept", "application/json")
        .form(&[
            ("AppSid", app_sid),
            ("SenderID", sender),
            ("Recipient", phone),
            ("Body", body),
        ])
        .send()
        .await?
        .error_for_status()?;
    let value: serde_json::Value = response.json().await.unwrap_or_default();
    if value.get("success").and_then(|item| item.as_bool()) == Some(false) {
        return Err(AppError::Message(format!("Unifonic SMS failed: {value}")));
    }
    Ok(())
}

fn extract_text(path: &Path) -> AppResult<String> {
    match path.extension().and_then(|ext| ext.to_str()).unwrap_or("").to_lowercase().as_str() {
        "txt" | "md" | "csv" => Ok(std::fs::read_to_string(path)?),
        "docx" => extract_docx(path),
        "pdf" => pdf_extract::extract_text(path).map_err(|err| AppError::Message(err.to_string())),
        other => Err(AppError::Message(format!("unsupported document type: {other}"))),
    }
}

fn extract_docx(path: &Path) -> AppResult<String> {
    let file = File::open(path)?;
    let mut archive = ZipArchive::new(file)?;
    let mut xml = String::new();
    archive.by_name("word/document.xml")?.read_to_string(&mut xml)?;
    let re = Regex::new(r"<[^>]+>").map_err(|err| AppError::Message(err.to_string()))?;
    Ok(re.replace_all(&xml, " ").replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">"))
}

fn chunk_text(text: &str) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    for sentence in text.split_terminator(['.', '!', '?', '\n']) {
        if current.len() + sentence.len() > 1200 && !current.is_empty() {
            chunks.push(current.trim().to_string());
            current.clear();
        }
        current.push_str(sentence.trim());
        current.push_str(". ");
    }
    if !current.trim().is_empty() {
        chunks.push(current.trim().to_string());
    }
    chunks
}

fn setting(conn: &Connection, key: &str) -> AppResult<String> {
    Ok(conn.query_row("SELECT value FROM settings WHERE key = ?1", params![key], |row| row.get(0))?)
}

fn upsert_setting(conn: &Connection, key: &str, value: &str) -> AppResult<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

fn now() -> String {
    Local::now().naive_local().format("%Y-%m-%d %H:%M:%S").to_string()
}

fn today() -> String {
    Local::now().date_naive().to_string()
}

fn clean(input: &str) -> String {
    input.trim().chars().take(240).collect()
}

fn clean_multiline(input: &str) -> String {
    input
        .replace('\u{0000}', "")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn device_fingerprint() -> String {
    let machine_guid = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey("SOFTWARE\\Microsoft\\Cryptography")
        .and_then(|key| key.get_value::<String, _>("MachineGuid"))
        .unwrap_or_else(|_| "unknown-machine".into());
    let username = env::var("USERNAME").unwrap_or_else(|_| "unknown-user".into());
    let computername = env::var("COMPUTERNAME").unwrap_or_else(|_| "unknown-computer".into());
    let mut hasher = Sha256::new();
    hasher.update(format!("carbonflow::{machine_guid}::{username}::{computername}"));
    hex::encode(hasher.finalize())[..32].to_string()
}


