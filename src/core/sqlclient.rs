//! Minimal SQL client for MySQL and PostgreSQL.
//!
//! Scope is deliberately small: connect, run one statement, return either a
//! result grid (SELECT) or an affected-row count (INSERT/UPDATE/DELETE).

use sqlx::{Column, Row, TypeInfo};
use tokio::sync::mpsc;

/// Which database engine a saved connection targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbKind {
    MySql,
    Postgres,
}

impl DbKind {
    pub fn title(self) -> &'static str {
        match self {
            DbKind::MySql => "MySQL",
            DbKind::Postgres => "PostgreSQL",
        }
    }

    pub fn default_port(self) -> u16 {
        match self {
            DbKind::MySql => 3306,
            DbKind::Postgres => 5432,
        }
    }

    pub fn all() -> [DbKind; 2] {
        [DbKind::MySql, DbKind::Postgres]
    }

    /// Parse the string stored in the vault.
    pub fn from_str(s: &str) -> Self {
        if s.eq_ignore_ascii_case("postgresql") || s.eq_ignore_ascii_case("postgres") {
            DbKind::Postgres
        } else {
            DbKind::MySql
        }
    }
}

/// What a finished statement produced.
pub enum QueryOutcome {
    /// A result set: column headers plus rows of already-stringified cells.
    Rows {
        columns: Vec<String>,
        rows: Vec<Vec<String>>,
        truncated: bool,
    },
    /// A statement that changed data rather than returning it.
    Affected { count: u64 },
    Error { message: String },
}

/// Progress of one query, streamed back to the UI thread.
///
/// Each event carries the id of the result tab it belongs to, because several
/// queries can be in flight at once.
pub enum SqlEvent {
    Started {
        tab_id: u64,
    },
    Finished {
        tab_id: u64,
        outcome: QueryOutcome,
        elapsed_ms: u128,
    },
}

/// Best-effort name of the first table a statement touches.
///
/// Used only as a tab caption, so a wrong guess is cosmetic. Returns `None`
/// when nothing table-like is found, and the caller falls back to a label
/// such as "Result 3".
pub fn primary_table_name(sql: &str) -> Option<String> {
    // Strip comments and collapse whitespace so the keyword scan is simple.
    let mut cleaned = String::with_capacity(sql.len());
    for line in sql.lines() {
        let line = match line.find("--") {
            Some(i) => &line[..i],
            None => line,
        };
        cleaned.push_str(line);
        cleaned.push(' ');
    }

    // Tokenise on whitespace and the punctuation that can hug a table name.
    let tokens: Vec<String> = cleaned
        .split(|c: char| c.is_whitespace() || c == ',' || c == '(' || c == ')' || c == ';')
        .filter(|t| !t.is_empty())
        .map(|t| t.to_string())
        .collect();

    // The table follows one of these keywords.
    const LEAD: [&str; 6] = ["FROM", "JOIN", "INTO", "UPDATE", "TABLE", "DESCRIBE"];

    let mut i = 0;
    while i < tokens.len() {
        let upper = tokens[i].to_ascii_uppercase();
        if LEAD.contains(&upper.as_str()) {
            // "DELETE FROM t" and "INSERT INTO t" both land here.
            if let Some(candidate) = tokens.get(i + 1) {
                let name = clean_identifier(candidate);
                if !name.is_empty() && !is_keyword(&name) {
                    return Some(name);
                }
            }
        }
        i += 1;
    }
    None
}

/// Strip quoting and any schema prefix from an identifier.
fn clean_identifier(raw: &str) -> String {
    let trimmed = raw.trim_matches(|c| c == '`' || c == '"' || c == '[' || c == ']' || c == '\'');
    // "public.users" reads better as just "users".
    let short = trimmed.rsplit('.').next().unwrap_or(trimmed);
    short
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
        .collect()
}

/// Guard against capturing a keyword when the real name is missing.
fn is_keyword(word: &str) -> bool {
    const WORDS: [&str; 12] = [
        "SELECT", "WHERE", "ORDER", "GROUP", "LIMIT", "SET", "VALUES", "ON", "AS", "LEFT",
        "RIGHT", "INNER",
    ];
    WORDS.contains(&word.to_ascii_uppercase().as_str())
}

/// Hard cap on rows pulled into memory, so a careless `SELECT *` on a large
/// table cannot stall the UI.
const MAX_ROWS: usize = 1000;

/// Build the connection URL sqlx expects.
///
/// Credentials are percent-encoded because passwords routinely contain
/// characters that would otherwise break the URL.
fn build_url(
    kind: DbKind,
    host: &str,
    port: u16,
    user: &str,
    pass: &str,
    database: &str,
) -> String {
    let scheme = match kind {
        DbKind::MySql => "mysql",
        DbKind::Postgres => "postgres",
    };
    format!(
        "{}://{}:{}@{}:{}/{}",
        scheme,
        encode(user),
        encode(pass),
        host,
        port,
        encode(database)
    )
}

/// Percent-encode everything outside the unreserved set.
fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// True when the statement is expected to return rows.
fn returns_rows(sql: &str) -> bool {
    let head = sql
        .trim_start()
        .trim_start_matches(|c: char| c == '(' || c.is_whitespace())
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_uppercase();

    matches!(
        head.as_str(),
        "SELECT" | "SHOW" | "DESCRIBE" | "DESC" | "EXPLAIN" | "WITH" | "PRAGMA" | "TABLE"
    )
}

/// Run one statement and report the result.
///
/// Connects fresh for each execution: simple, and avoids holding a socket open
/// across an SSH tunnel that the user may stop at any moment.
#[allow(clippy::too_many_arguments)]
pub fn spawn_query(
    rt: &tokio::runtime::Handle,
    tab_id: u64,
    kind: DbKind,
    host: String,
    port: u16,
    user: String,
    pass: String,
    database: String,
    sql: String,
    tx: mpsc::UnboundedSender<SqlEvent>,
    ctx: eframe::egui::Context,
) {
    let _ = tx.send(SqlEvent::Started { tab_id });
    ctx.request_repaint();

    rt.spawn(async move {
        let started = std::time::Instant::now();
        let url = build_url(kind, &host, port, &user, &pass, &database);

        let outcome = match kind {
            DbKind::MySql => run_mysql(&url, &sql).await,
            DbKind::Postgres => run_postgres(&url, &sql).await,
        };

        let _ = tx.send(SqlEvent::Finished {
            tab_id,
            outcome,
            elapsed_ms: started.elapsed().as_millis(),
        });
        ctx.request_repaint();
    });
}

/// Connect with a timeout so an unreachable host fails promptly.
macro_rules! connect_or_bail {
    ($fut:expr) => {
        match tokio::time::timeout(std::time::Duration::from_secs(12), $fut).await {
            Ok(Ok(c)) => c,
            Ok(Err(e)) => {
                return QueryOutcome::Error {
                    message: format!("Connection failed: {}", e),
                }
            }
            Err(_) => {
                return QueryOutcome::Error {
                    message: "Connection timed out after 12s.".to_string(),
                }
            }
        }
    };
}

async fn run_mysql(url: &str, sql: &str) -> QueryOutcome {
    use sqlx::mysql::MySqlConnection;
    use sqlx::Connection;

    let mut conn: MySqlConnection = connect_or_bail!(MySqlConnection::connect(url));

    if returns_rows(sql) {
        match sqlx::query(sql).fetch_all(&mut conn).await {
            Ok(rows) => mysql_rows_to_outcome(rows),
            Err(e) => QueryOutcome::Error {
                message: format!("{}", e),
            },
        }
    } else {
        match sqlx::query(sql).execute(&mut conn).await {
            Ok(res) => QueryOutcome::Affected {
                count: res.rows_affected(),
            },
            Err(e) => QueryOutcome::Error {
                message: format!("{}", e),
            },
        }
    }
}

async fn run_postgres(url: &str, sql: &str) -> QueryOutcome {
    use sqlx::postgres::PgConnection;
    use sqlx::Connection;

    let mut conn: PgConnection = connect_or_bail!(PgConnection::connect(url));

    if returns_rows(sql) {
        match sqlx::query(sql).fetch_all(&mut conn).await {
            Ok(rows) => pg_rows_to_outcome(rows),
            Err(e) => QueryOutcome::Error {
                message: format!("{}", e),
            },
        }
    } else {
        match sqlx::query(sql).execute(&mut conn).await {
            Ok(res) => QueryOutcome::Affected {
                count: res.rows_affected(),
            },
            Err(e) => QueryOutcome::Error {
                message: format!("{}", e),
            },
        }
    }
}

fn mysql_rows_to_outcome(rows: Vec<sqlx::mysql::MySqlRow>) -> QueryOutcome {
    if rows.is_empty() {
        return QueryOutcome::Rows {
            columns: Vec::new(),
            rows: Vec::new(),
            truncated: false,
        };
    }

    let columns: Vec<String> = rows[0]
        .columns()
        .iter()
        .map(|c| c.name().to_string())
        .collect();

    let truncated = rows.len() > MAX_ROWS;
    let out: Vec<Vec<String>> = rows
        .iter()
        .take(MAX_ROWS)
        .map(|row| {
            (0..columns.len())
                .map(|i| mysql_cell_to_string(row, i))
                .collect()
        })
        .collect();

    QueryOutcome::Rows {
        columns,
        rows: out,
        truncated,
    }
}

fn pg_rows_to_outcome(rows: Vec<sqlx::postgres::PgRow>) -> QueryOutcome {
    if rows.is_empty() {
        return QueryOutcome::Rows {
            columns: Vec::new(),
            rows: Vec::new(),
            truncated: false,
        };
    }

    let columns: Vec<String> = rows[0]
        .columns()
        .iter()
        .map(|c| c.name().to_string())
        .collect();

    let truncated = rows.len() > MAX_ROWS;
    let out: Vec<Vec<String>> = rows
        .iter()
        .take(MAX_ROWS)
        .map(|row| {
            (0..columns.len())
                .map(|i| pg_cell_to_string(row, i))
                .collect()
        })
        .collect();

    QueryOutcome::Rows {
        columns,
        rows: out,
        truncated,
    }
}

/// Render one MySQL cell as text.
///
/// sqlx is statically typed, so each candidate type is tried in turn; the
/// display layer only ever needs a string.
fn mysql_cell_to_string(row: &sqlx::mysql::MySqlRow, idx: usize) -> String {
    use sqlx::ValueRef;

    // NULL first: decoding a NULL as any concrete type would error.
    if let Ok(raw) = row.try_get_raw(idx) {
        if raw.is_null() {
            return "NULL".to_string();
        }
    }

    let type_name = row
        .columns()
        .get(idx)
        .map(|c| c.type_info().name().to_ascii_uppercase())
        .unwrap_or_default();

    // Strings cover the majority of columns, so try them first.
    if let Ok(v) = row.try_get::<String, _>(idx) {
        return v;
    }
    if let Ok(v) = row.try_get::<i64, _>(idx) {
        return v.to_string();
    }
    if let Ok(v) = row.try_get::<u64, _>(idx) {
        return v.to_string();
    }
    if let Ok(v) = row.try_get::<f64, _>(idx) {
        return v.to_string();
    }
    if let Ok(v) = row.try_get::<bool, _>(idx) {
        return v.to_string();
    }
    if let Ok(v) = row.try_get::<chrono::NaiveDateTime, _>(idx) {
        return v.to_string();
    }
    if let Ok(v) = row.try_get::<chrono::NaiveDate, _>(idx) {
        return v.to_string();
    }
    if let Ok(v) = row.try_get::<chrono::NaiveTime, _>(idx) {
        return v.to_string();
    }
    if let Ok(v) = row.try_get::<Vec<u8>, _>(idx) {
        return match String::from_utf8(v.clone()) {
            Ok(s) => s,
            Err(_) => format!("<{} bytes>", v.len()),
        };
    }

    format!("<{}>", type_name)
}

/// Render one PostgreSQL cell as text.
fn pg_cell_to_string(row: &sqlx::postgres::PgRow, idx: usize) -> String {
    use sqlx::ValueRef;

    if let Ok(raw) = row.try_get_raw(idx) {
        if raw.is_null() {
            return "NULL".to_string();
        }
    }

    let type_name = row
        .columns()
        .get(idx)
        .map(|c| c.type_info().name().to_ascii_uppercase())
        .unwrap_or_default();

    if let Ok(v) = row.try_get::<String, _>(idx) {
        return v;
    }
    if let Ok(v) = row.try_get::<i64, _>(idx) {
        return v.to_string();
    }
    if let Ok(v) = row.try_get::<i32, _>(idx) {
        return v.to_string();
    }
    if let Ok(v) = row.try_get::<i16, _>(idx) {
        return v.to_string();
    }
    if let Ok(v) = row.try_get::<f64, _>(idx) {
        return v.to_string();
    }
    if let Ok(v) = row.try_get::<f32, _>(idx) {
        return v.to_string();
    }
    if let Ok(v) = row.try_get::<bool, _>(idx) {
        return v.to_string();
    }
    if let Ok(v) = row.try_get::<chrono::NaiveDateTime, _>(idx) {
        return v.to_string();
    }
    if let Ok(v) = row.try_get::<chrono::DateTime<chrono::Utc>, _>(idx) {
        return v.to_string();
    }
    if let Ok(v) = row.try_get::<chrono::NaiveDate, _>(idx) {
        return v.to_string();
    }
    if let Ok(v) = row.try_get::<chrono::NaiveTime, _>(idx) {
        return v.to_string();
    }
    if let Ok(v) = row.try_get::<Vec<u8>, _>(idx) {
        return match String::from_utf8(v.clone()) {
            Ok(s) => s,
            Err(_) => format!("<{} bytes>", v.len()),
        };
    }

    format!("<{}>", type_name)
}
