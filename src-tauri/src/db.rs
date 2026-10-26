use chrono::{SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row as SqlRow};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Mutex;
use uuid::Uuid;

pub struct Db(pub Mutex<Connection>);

pub fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub fn new_id() -> String {
    Uuid::new_v4().to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub id: String,
    pub parent_id: Option<String>,
    pub title: String,
    pub icon: Option<String>,
    pub cover: Option<String>,
    pub cover_type: String,
    pub cover_position: i64,
    #[serde(rename = "type")]
    pub page_type: String,
    pub database_id: Option<String>,
    pub position: f64,
    pub is_favorite: bool,
    pub is_template: bool,
    pub locked: bool,
    pub font: String,
    pub small_text: bool,
    pub full_width: bool,
    pub created_at: String,
    pub updated_at: String,
    pub trashed_at: Option<String>,
    pub last_edited_by: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageSummary {
    pub id: String,
    pub parent_id: Option<String>,
    pub title: String,
    pub icon: Option<String>,
    pub cover: Option<String>,
    pub cover_type: String,
    #[serde(rename = "type")]
    pub page_type: String,
    pub database_id: Option<String>,
    pub position: f64,
    pub is_favorite: bool,
    pub is_template: bool,
    pub updated_at: String,
    pub trashed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Block {
    pub id: String,
    pub page_id: String,
    pub parent_id: Option<String>,
    #[serde(rename = "type")]
    pub block_type: String,
    pub html: String,
    pub text: String,
    pub props: Value,
    pub position: f64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockInput {
    pub id: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(rename = "type")]
    pub block_type: String,
    #[serde(default)]
    pub html: String,
    #[serde(default)]
    pub text: String,
    #[serde(default = "empty_obj")]
    pub props: Value,
    #[serde(default)]
    pub position: f64,
}

fn empty_obj() -> Value {
    json!({})
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Database {
    pub id: String,
    pub page_id: String,
    pub name: String,
    pub properties: Value,
    pub views: Value,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DbRow {
    pub id: String,
    pub database_id: String,
    pub page_id: String,
    pub values: Value,
    pub position: f64,
    pub created_at: String,
    pub updated_at: String,
    pub title: String,
    pub icon: Option<String>,
    pub trashed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment {
    pub id: String,
    pub page_id: String,
    pub block_id: Option<String>,
    pub html: String,
    pub author: String,
    pub created_at: String,
    pub resolved: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub page: PageSummary,
    pub snippet: String,
    pub matched: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrashItem {
    pub page: PageSummary,
    pub parent_title: Option<String>,
    pub deleted_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageDetail {
    pub page: Page,
    pub blocks: Vec<Block>,
    pub database: Option<Database>,
    pub rows: Vec<DbRow>,
    pub row: Option<DbRow>,
    pub ancestors: Vec<PageSummary>,
    pub children: Vec<PageSummary>,
    pub comments: Vec<Comment>,
    pub versions: Vec<Value>,
}

pub const PAGE_COLS: &str = "id,parent_id,title,icon,cover,cover_type,cover_position,type,database_id,position,is_favorite,is_template,locked,font,small_text,full_width,created_at,updated_at,trashed_at,last_edited_by";

pub fn page_from_row(r: &SqlRow) -> rusqlite::Result<Page> {
    Ok(Page {
        id: r.get("id")?,
        parent_id: r.get("parent_id")?,
        title: r.get("title")?,
        icon: r.get("icon")?,
        cover: r.get("cover")?,
        cover_type: r.get("cover_type")?,
        cover_position: r.get("cover_position")?,
        page_type: r.get("type")?,
        database_id: r.get("database_id")?,
        position: r.get("position")?,
        is_favorite: r.get::<_, i64>("is_favorite")? != 0,
        is_template: r.get::<_, i64>("is_template")? != 0,
        locked: r.get::<_, i64>("locked")? != 0,
        font: r.get("font")?,
        small_text: r.get::<_, i64>("small_text")? != 0,
        full_width: r.get::<_, i64>("full_width")? != 0,
        created_at: r.get("created_at")?,
        updated_at: r.get("updated_at")?,
        trashed_at: r.get("trashed_at")?,
        last_edited_by: r.get("last_edited_by")?,
    })
}

pub fn page_summary(p: &Page) -> PageSummary {
    PageSummary {
        id: p.id.clone(),
        parent_id: p.parent_id.clone(),
        title: p.title.clone(),
        icon: p.icon.clone(),
        cover: p.cover.clone(),
        cover_type: p.cover_type.clone(),
        page_type: p.page_type.clone(),
        database_id: p.database_id.clone(),
        position: p.position,
        is_favorite: p.is_favorite,
        is_template: p.is_template,
        updated_at: p.updated_at.clone(),
        trashed_at: p.trashed_at.clone(),
    }
}

pub fn block_from_row(r: &SqlRow) -> rusqlite::Result<Block> {
    let props_raw: String = r.get("props")?;
    Ok(Block {
        id: r.get("id")?,
        page_id: r.get("page_id")?,
        parent_id: r.get("parent_id")?,
        block_type: r.get("type")?,
        html: r.get("html")?,
        text: r.get("text")?,
        props: serde_json::from_str(&props_raw).unwrap_or_else(|_| json!({})),
        position: r.get("position")?,
        created_at: r.get("created_at")?,
        updated_at: r.get("updated_at")?,
    })
}

pub fn database_from_row(r: &SqlRow) -> rusqlite::Result<Database> {
    let props_raw: String = r.get("properties")?;
    let views_raw: String = r.get("views")?;
    Ok(Database {
        id: r.get("id")?,
        page_id: r.get("page_id")?,
        name: r.get("name")?,
        properties: serde_json::from_str(&props_raw).unwrap_or_else(|_| json!([])),
        views: serde_json::from_str(&views_raw).unwrap_or_else(|_| json!([])),
        created_at: r.get("created_at")?,
        updated_at: r.get("updated_at")?,
    })
}

pub fn db_row_from_row(r: &SqlRow) -> rusqlite::Result<DbRow> {
    let values_raw: String = r.get("values_json")?;
    let trashed: Option<String> = r.get("trashed_at")?;
    Ok(DbRow {
        id: r.get("id")?,
        database_id: r.get("database_id")?,
        page_id: r.get("page_id")?,
        values: serde_json::from_str(&values_raw).unwrap_or_else(|_| json!({})),
        position: r.get("position")?,
        created_at: r.get("created_at")?,
        updated_at: r.get("updated_at")?,
        title: r.get("title")?,
        icon: r.get("icon")?,
        trashed: trashed.is_some(),
    })
}

pub fn open(path: &Path) -> Result<Connection, String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let conn = Connection::open(path).map_err(|e| e.to_string())?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;
         PRAGMA foreign_keys=ON;
         PRAGMA busy_timeout=5000;",
    )
    .map_err(|e| e.to_string())?;
    migrate(&conn)?;
    Ok(conn)
}

pub fn migrate(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS workspace(
          id TEXT PRIMARY KEY,
          name TEXT NOT NULL,
          icon TEXT,
          created_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS pages(
          id TEXT PRIMARY KEY,
          parent_id TEXT,
          title TEXT NOT NULL DEFAULT '',
          icon TEXT,
          cover TEXT,
          cover_type TEXT NOT NULL DEFAULT 'none',
          cover_position INTEGER NOT NULL DEFAULT 50,
          type TEXT NOT NULL DEFAULT 'page',
          database_id TEXT,
          position REAL NOT NULL DEFAULT 0,
          is_favorite INTEGER NOT NULL DEFAULT 0,
          is_template INTEGER NOT NULL DEFAULT 0,
          locked INTEGER NOT NULL DEFAULT 0,
          font TEXT NOT NULL DEFAULT 'default',
          small_text INTEGER NOT NULL DEFAULT 0,
          full_width INTEGER NOT NULL DEFAULT 0,
          created_at TEXT NOT NULL,
          updated_at TEXT NOT NULL,
          trashed_at TEXT,
          last_edited_by TEXT NOT NULL DEFAULT 'You'
        );
        CREATE INDEX IF NOT EXISTS idx_pages_parent ON pages(parent_id);
        CREATE INDEX IF NOT EXISTS idx_pages_trashed ON pages(trashed_at);
        CREATE TABLE IF NOT EXISTS blocks(
          id TEXT PRIMARY KEY,
          page_id TEXT NOT NULL,
          parent_id TEXT,
          type TEXT NOT NULL,
          html TEXT NOT NULL DEFAULT '',
          text TEXT NOT NULL DEFAULT '',
          props TEXT NOT NULL DEFAULT '{}',
          position REAL NOT NULL DEFAULT 0,
          created_at TEXT NOT NULL,
          updated_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_blocks_page ON blocks(page_id, position);
        CREATE TABLE IF NOT EXISTS databases(
          id TEXT PRIMARY KEY,
          page_id TEXT NOT NULL UNIQUE,
          name TEXT NOT NULL DEFAULT '',
          properties TEXT NOT NULL DEFAULT '[]',
          views TEXT NOT NULL DEFAULT '[]',
          created_at TEXT NOT NULL,
          updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS db_rows(
          id TEXT PRIMARY KEY,
          database_id TEXT NOT NULL,
          page_id TEXT NOT NULL UNIQUE,
          values_json TEXT NOT NULL DEFAULT '{}',
          position REAL NOT NULL DEFAULT 0,
          created_at TEXT NOT NULL,
          updated_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_rows_db ON db_rows(database_id, position);
        CREATE TABLE IF NOT EXISTS settings(
          key TEXT PRIMARY KEY,
          value TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS comments(
          id TEXT PRIMARY KEY,
          page_id TEXT NOT NULL,
          block_id TEXT,
          html TEXT NOT NULL,
          author TEXT NOT NULL DEFAULT 'You',
          created_at TEXT NOT NULL,
          resolved INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_comments_page ON comments(page_id);
        CREATE TABLE IF NOT EXISTS snapshots(
          id TEXT PRIMARY KEY,
          page_id TEXT NOT NULL,
          data TEXT NOT NULL,
          created_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_snapshots_page ON snapshots(page_id, created_at);
        CREATE TABLE IF NOT EXISTS recents(
          page_id TEXT PRIMARY KEY,
          visited_at TEXT NOT NULL
        );
        "#,
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn get_page(conn: &Connection, id: &str) -> Result<Page, String> {
    conn.query_row(
        &format!("SELECT {} FROM pages WHERE id=?1", PAGE_COLS),
        params![id],
        page_from_row,
    )
    .optional()
    .map_err(|e| e.to_string())?
    .ok_or_else(|| "Page not found".to_string())
}

pub fn get_page_summary(conn: &Connection, id: &str) -> Result<PageSummary, String> {
    Ok(page_summary(&get_page(conn, id)?))
}

pub fn get_workspace(conn: &Connection) -> Result<Workspace, String> {
    conn.query_row(
        "SELECT id,name,icon,created_at FROM workspace LIMIT 1",
        [],
        |r| {
            Ok(Workspace {
                id: r.get(0)?,
                name: r.get(1)?,
                icon: r.get(2)?,
                created_at: r.get(3)?,
            })
        },
    )
    .optional()
    .map_err(|e| e.to_string())?
    .ok_or_else(|| "Workspace not found".to_string())
}

pub fn next_position(conn: &Connection, parent: Option<&str>) -> Result<f64, String> {
    let val: Option<f64> = match parent {
        Some(p) => conn
            .query_row(
                "SELECT MAX(position) FROM pages WHERE parent_id=?1 AND trashed_at IS NULL",
                params![p],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?,
        None => conn
            .query_row(
                "SELECT MAX(position) FROM pages WHERE parent_id IS NULL AND trashed_at IS NULL",
                [],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?,
    };
    Ok(val.unwrap_or(0.0) + 1.0)
}

pub fn descendants(conn: &Connection, id: &str) -> Result<Vec<Page>, String> {
    let mut out = Vec::new();
    let mut stack = vec![id.to_string()];
    while let Some(cur) = stack.pop() {
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {} FROM pages WHERE parent_id=?1",
                PAGE_COLS
            ))
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![cur], page_from_row)
            .map_err(|e| e.to_string())?;
        for r in rows {
            let p = r.map_err(|e| e.to_string())?;
            stack.push(p.id.clone());
            out.push(p);
        }
    }
    Ok(out)
}

pub fn ancestors(conn: &Connection, id: &str) -> Result<Vec<PageSummary>, String> {
    let mut out = Vec::new();
    let mut cur = get_page(conn, id)?.parent_id;
    let mut guard = 0;
    while let Some(pid) = cur {
        guard += 1;
        if guard > 64 {
            break;
        }
        let p = get_page(conn, &pid)?;
        cur = p.parent_id.clone();
        out.push(page_summary(&p));
    }
    out.reverse();
    Ok(out)
}

pub fn snapshot(conn: &Connection, page_id: &str, force: bool) -> Result<(), String> {
    if !force {
        let last: Option<String> = conn
            .query_row(
                "SELECT created_at FROM snapshots WHERE page_id=?1 ORDER BY created_at DESC LIMIT 1",
                params![page_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if let Some(last) = last {
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&last) {
                if Utc::now().signed_duration_since(dt.with_timezone(&Utc)).num_seconds() < 120 {
                    return Ok(());
                }
            }
        }
    }
    let page = get_page(conn, page_id)?;
    let mut stmt = conn
        .prepare("SELECT * FROM blocks WHERE page_id=?1 ORDER BY position")
        .map_err(|e| e.to_string())?;
    let blocks: Vec<Block> = stmt
        .query_map(params![page_id], block_from_row)
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    let data = json!({ "page": page, "blocks": blocks });
    conn.execute(
        "INSERT INTO snapshots(id,page_id,data,created_at) VALUES(?1,?2,?3,?4)",
        params![
            new_id(),
            page_id,
            serde_json::to_string(&data).map_err(|e| e.to_string())?,
            now()
        ],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM snapshots WHERE page_id=?1 AND id NOT IN (SELECT id FROM snapshots WHERE page_id=?1 ORDER BY created_at DESC LIMIT 30)",
        params![page_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn get_versions(conn: &Connection, page_id: &str) -> Result<Vec<Value>, String> {
    let mut stmt = conn
        .prepare("SELECT id,created_at,data FROM snapshots WHERE page_id=?1 ORDER BY created_at DESC LIMIT 30")
        .map_err(|e| e.to_string())?;
    let versions = stmt
        .query_map(params![page_id], |r| {
            let id: String = r.get(0)?;
            let created: String = r.get(1)?;
            let data: String = r.get(2)?;
            let v: Value = serde_json::from_str(&data).unwrap_or_else(|_| json!({}));
            let block_count = v
                .get("blocks")
                .and_then(|b| b.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            Ok(json!({ "id": id, "createdAt": created, "blockCount": block_count }))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    Ok(versions)
}

pub fn get_setting(conn: &Connection, key: &str) -> Option<Value> {
    conn.query_row(
        "SELECT value FROM settings WHERE key=?1",
        params![key],
        |r| r.get::<_, String>(0),
    )
    .optional()
    .ok()
    .flatten()
    .and_then(|s| serde_json::from_str(&s).ok())
}

pub fn set_setting(conn: &Connection, key: &str, value: &Value) -> Result<(), String> {
    conn.execute(
        "INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, serde_json::to_string(value).map_err(|e| e.to_string())?],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn insert_page(conn: &Connection, p: &Page) -> Result<(), String> {
    conn.execute(
        "INSERT INTO pages(id,parent_id,title,icon,cover,cover_type,cover_position,type,database_id,position,is_favorite,is_template,locked,font,small_text,full_width,created_at,updated_at,trashed_at,last_edited_by)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20)",
        params![
            p.id, p.parent_id, p.title, p.icon, p.cover, p.cover_type, p.cover_position, p.page_type,
            p.database_id, p.position, p.is_favorite as i64, p.is_template as i64, p.locked as i64,
            p.font, p.small_text as i64, p.full_width as i64, p.created_at, p.updated_at,
            p.trashed_at, p.last_edited_by
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn insert_block(conn: &Connection, b: &Block) -> Result<(), String> {
    conn.execute(
        "INSERT INTO blocks(id,page_id,parent_id,type,html,text,props,position,created_at,updated_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        params![
            b.id,
            b.page_id,
            b.parent_id,
            b.block_type,
            b.html,
            b.text,
            serde_json::to_string(&b.props).unwrap_or_else(|_| "{}".into()),
            b.position,
            b.created_at,
            b.updated_at
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn empty_page(
    parent_id: Option<String>,
    title: String,
    page_type: &str,
    position: f64,
) -> Page {
    let t = now();
    Page {
        id: new_id(),
        parent_id,
        title,
        icon: None,
        cover: None,
        cover_type: "none".into(),
        cover_position: 50,
        page_type: page_type.into(),
        database_id: None,
        position,
        is_favorite: false,
        is_template: false,
        locked: false,
        font: "default".into(),
        small_text: false,
        full_width: false,
        created_at: t.clone(),
        updated_at: t,
        trashed_at: None,
        last_edited_by: "You".into(),
    }
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct SeedWorkspace {
    id: String,
    name: String,
    icon: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct SeedPage {
    id: String,
    parent_id: Option<String>,
    title: String,
    icon: Option<String>,
    cover: Option<String>,
    cover_type: String,
    cover_position: i64,
    #[serde(rename = "type")]
    page_type: String,
    database_id: Option<String>,
    position: f64,
    is_favorite: bool,
    is_template: bool,
    updated_at: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct SeedBlock {
    id: String,
    page_id: String,
    parent_id: Option<String>,
    #[serde(rename = "type")]
    block_type: String,
    html: String,
    text: String,
    props: Value,
    position: f64,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct SeedDatabase {
    id: String,
    page_id: String,
    name: String,
    properties: Value,
    views: Value,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct SeedRow {
    id: String,
    database_id: String,
    page_id: String,
    values: Value,
    position: f64,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct Seed {
    workspace: SeedWorkspace,
    pages: Vec<SeedPage>,
    blocks: Vec<SeedBlock>,
    databases: Vec<SeedDatabase>,
    rows: Vec<SeedRow>,
}

fn seed_default_str(s: String, fallback: &str) -> String {
    if s.is_empty() {
        fallback.to_string()
    } else {
        s
    }
}

pub fn seed_if_empty(conn: &mut Connection) -> Result<(), String> {
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM pages", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    if count > 0 {
        return Ok(());
    }
    let raw = include_str!("../seed.json");
    let seed: Seed = serde_json::from_str(raw).map_err(|e| format!("Seed parse error: {e}"))?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let t = now();
    let ws = &seed.workspace;
    tx.execute(
        "INSERT INTO workspace(id,name,icon,created_at) VALUES(?1,?2,?3,?4)",
        params![
            seed_default_str(ws.id.clone(), "ws-ideacraft"),
            seed_default_str(ws.name.clone(), "IdeaCraft"),
            ws.icon,
            t
        ],
    )
    .map_err(|e| e.to_string())?;

    for sp in &seed.pages {
        let updated = sp.updated_at.clone().unwrap_or_else(|| t.clone());
        let page_type = seed_default_str(sp.page_type.clone(), "page");
        let cover_type = if sp.cover.is_some() {
            seed_default_str(sp.cover_type.clone(), "gradient")
        } else {
            "none".to_string()
        };
        let p = Page {
            id: sp.id.clone(),
            parent_id: sp.parent_id.clone(),
            title: sp.title.clone(),
            icon: sp.icon.clone(),
            cover: sp.cover.clone(),
            cover_type,
            cover_position: if sp.cover_position == 0 { 50 } else { sp.cover_position },
            page_type,
            database_id: sp.database_id.clone(),
            position: sp.position,
            is_favorite: sp.is_favorite,
            is_template: sp.is_template,
            locked: false,
            font: "default".into(),
            small_text: false,
            full_width: false,
            created_at: updated.clone(),
            updated_at: updated,
            trashed_at: None,
            last_edited_by: "You".into(),
        };
        insert_page(&tx, &p)?;
    }

    for sb in &seed.blocks {
        let props = if sb.props.is_null() {
            json!({})
        } else {
            sb.props.clone()
        };
        let b = Block {
            id: sb.id.clone(),
            page_id: sb.page_id.clone(),
            parent_id: sb.parent_id.clone(),
            block_type: seed_default_str(sb.block_type.clone(), "paragraph"),
            html: sb.html.clone(),
            text: sb.text.clone(),
            props,
            position: sb.position,
            created_at: t.clone(),
            updated_at: t.clone(),
        };
        insert_block(&tx, &b)?;
    }

    for sd in &seed.databases {
        let props = if sd.properties.is_null() {
            json!([])
        } else {
            sd.properties.clone()
        };
        let views = if sd.views.is_null() {
            json!([])
        } else {
            sd.views.clone()
        };
        tx.execute(
            "INSERT INTO databases(id,page_id,name,properties,views,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                sd.id,
                sd.page_id,
                seed_default_str(sd.name.clone(), "Database"),
                serde_json::to_string(&props).unwrap(),
                serde_json::to_string(&views).unwrap(),
                t,
                t
            ],
        )
        .map_err(|e| e.to_string())?;
    }

    for sr in &seed.rows {
        let values = if sr.values.is_null() {
            json!({})
        } else {
            sr.values.clone()
        };
        tx.execute(
            "INSERT INTO db_rows(id,database_id,page_id,values_json,position,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                sr.id,
                sr.database_id,
                sr.page_id,
                serde_json::to_string(&values).unwrap(),
                sr.position,
                t,
                t
            ],
        )
        .map_err(|e| e.to_string())?;
    }

    for (page, at) in [("p-welcome", "2026-10-21T08:42:00.000Z"), ("p-todo", "2026-10-21T07:15:00.000Z")] {
        tx.execute(
            "INSERT OR REPLACE INTO recents(page_id,visited_at) VALUES(?1,?2)",
            params![page, at],
        )
        .ok();
    }

    let defaults = [
        ("theme", json!("system")),
        ("sidebarWidth", json!(275)),
        ("sidebarCollapsed", json!(false)),
        ("font", json!("default")),
        ("smallText", json!(false)),
        ("fullWidth", json!(false)),
        ("keepInBackground", json!(false)),
        ("userName", json!("You")),
        ("userEmail", json!("")),
        ("userAvatar", json!("")),
        ("aiEnabled", json!(false)),
        ("aiBaseUrl", json!("http://127.0.0.1:11434")),
        ("aiModel", json!("llama3.2")),
        ("syncFolder", json!("")),
        ("syncAuto", json!(false)),
        ("onboardingDone", json!(false)),
        ("tabsState", Value::Null),
    ];
    for (k, v) in defaults {
        set_setting(&tx, k, &v)?;
    }
    set_setting(&tx, "seeded", &json!(true))?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(())
}

pub fn default_database(page: &Page) -> Database {
    let t = now();
    let props = json!([
        {"id":"title","name":"Name","type":"title"},
        {"id":"status","name":"Status","type":"status","options":[
            {"id":"not_started","name":"Not started","color":"gray"},
            {"id":"in_progress","name":"In progress","color":"blue"},
            {"id":"done","name":"Done","color":"green"}
        ]},
        {"id":"notes","name":"Notes","type":"text"}
    ]);
    let views = json!([
        {"id":"table","name":"Table","type":"table","groupBy":null,"dateProperty":null,"filters":[],"sorts":[]},
        {"id":"board","name":"Board","type":"board","groupBy":"status","dateProperty":null,"filters":[],"sorts":[]}
    ]);
    Database {
        id: new_id(),
        page_id: page.id.clone(),
        name: page.title.clone(),
        properties: props,
        views,
        created_at: t.clone(),
        updated_at: t,
    }
}
