use crate::db::*;
use crate::markdown;
use base64::Engine;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};

type CmdResult<T> = Result<T, String>;

fn all_settings(conn: &rusqlite::Connection) -> Value {
    let mut map = serde_json::Map::new();
    if let Ok(mut stmt) = conn.prepare("SELECT key,value FROM settings") {
        if let Ok(rows) = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        }) {
            for row in rows.flatten() {
                if let Ok(v) = serde_json::from_str::<Value>(&row.1) {
                    map.insert(row.0, v);
                }
            }
        }
    }
    Value::Object(map)
}

fn versions(conn: &rusqlite::Connection, page_id: &str) -> Vec<Value> {
    get_versions(conn, page_id).unwrap_or_default()
}

fn detail(conn: &rusqlite::Connection, id: &str) -> CmdResult<PageDetail> {
    let page = get_page(conn, id)?;
    let mut stmt = conn
        .prepare("SELECT * FROM blocks WHERE page_id=?1 ORDER BY position, rowid")
        .map_err(|e| e.to_string())?;
    let blocks = stmt
        .query_map(params![id], block_from_row)
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect::<Vec<_>>();

    let mut database: Option<Database> = if let Some(db_id) = page.database_id.as_deref() {
        conn.query_row(
            "SELECT * FROM databases WHERE id=?1",
            params![db_id],
            database_from_row,
        )
        .optional()
        .map_err(|e| e.to_string())?
    } else {
        None
    };
    if database.is_none() {
        database = conn
            .query_row(
                "SELECT * FROM databases WHERE page_id=?1",
                params![id],
                database_from_row,
            )
            .optional()
            .map_err(|e| e.to_string())?;
    }

    let mut rows = Vec::new();
    if let Some(db) = &database {
        let mut stmt = conn
            .prepare(
                "SELECT r.*, p.title AS title, p.icon AS icon, p.trashed_at AS trashed_at
                 FROM db_rows r JOIN pages p ON p.id=r.page_id
                 WHERE r.database_id=?1 ORDER BY r.position, r.rowid",
            )
            .map_err(|e| e.to_string())?;
        rows = stmt
            .query_map(params![db.id], db_row_from_row)
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
    }

    let row: Option<DbRow> = conn
        .query_row(
            "SELECT r.*, p.title AS title, p.icon AS icon, p.trashed_at AS trashed_at
             FROM db_rows r JOIN pages p ON p.id=r.page_id WHERE r.page_id=?1",
            params![id],
            db_row_from_row,
        )
        .optional()
        .map_err(|e| e.to_string())?;

    let mut stmt = conn
        .prepare(&format!(
            "SELECT {} FROM pages WHERE parent_id=?1 AND trashed_at IS NULL AND is_template=0 ORDER BY position, title",
            PAGE_COLS
        ))
        .map_err(|e| e.to_string())?;
    let children = stmt
        .query_map(params![id], page_from_row)
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .map(|p| page_summary(&p))
        .collect();

    let mut stmt = conn
        .prepare("SELECT * FROM comments WHERE page_id=?1 ORDER BY created_at")
        .map_err(|e| e.to_string())?;
    let comments = stmt
        .query_map(params![id], |r| {
            Ok(Comment {
                id: r.get("id")?,
                page_id: r.get("page_id")?,
                block_id: r.get("block_id")?,
                html: r.get("html")?,
                author: r.get("author")?,
                created_at: r.get("created_at")?,
                resolved: r.get::<_, i64>("resolved")? != 0,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    Ok(PageDetail {
        page,
        blocks,
        database,
        rows,
        row,
        ancestors: ancestors(conn, id)?,
        children,
        comments,
        versions: versions(conn, id),
    })
}

#[tauri::command]
pub fn bootstrap(state: State<Db>, app: AppHandle) -> CmdResult<Value> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let workspace = get_workspace(&conn)?;
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {} FROM pages WHERE trashed_at IS NULL AND is_template=0 AND type!='row' ORDER BY position, title",
            PAGE_COLS
        ))
        .map_err(|e| e.to_string())?;
    let pages = stmt
        .query_map([], page_from_row)
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .map(|p| page_summary(&p))
        .collect::<Vec<_>>();

    let mut stmt = conn
        .prepare(&format!(
            "SELECT {} FROM pages WHERE is_template=1 ORDER BY position, title",
            PAGE_COLS
        ))
        .map_err(|e| e.to_string())?;
    let templates = stmt
        .query_map([], page_from_row)
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .map(|p| page_summary(&p))
        .collect::<Vec<_>>();

    let page_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pages WHERE is_template=0 AND trashed_at IS NULL AND type!='row'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let block_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM blocks", [], |r| r.get(0))
        .unwrap_or(0);
    let database_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM databases", [], |r| r.get(0))
        .unwrap_or(0);
    let page_ids = {
        let mut stmt = conn
            .prepare("SELECT id FROM pages WHERE trashed_at IS NULL AND is_template=0")
            .map_err(|e| e.to_string())?;
        let ids = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect::<Vec<_>>();
        ids
    };

    let settings = all_settings(&conn);
    let data_path = app
        .path()
        .app_data_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    Ok(json!({
        "workspace": workspace,
        "pages": pages,
        "pageIds": page_ids,
        "templates": templates,
        "settings": settings,
        "counts": { "pages": page_count, "blocks": block_count, "databases": database_count },
        "app": { "version": env!("CARGO_PKG_VERSION"), "dataPath": data_path, "platform": std::env::consts::OS }
    }))
}

#[tauri::command]
pub fn get_sidebar(state: State<Db>) -> CmdResult<Value> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let workspace = get_workspace(&conn)?;
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {} FROM pages WHERE trashed_at IS NULL AND is_template=0 AND type!='row' ORDER BY position, title",
            PAGE_COLS
        ))
        .map_err(|e| e.to_string())?;
    let pages = stmt
        .query_map([], page_from_row)
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .map(|p| page_summary(&p))
        .collect::<Vec<_>>();
    Ok(json!({ "workspace": workspace, "pages": pages }))
}

#[tauri::command]
pub fn get_page_detail(state: State<Db>, id: String) -> CmdResult<PageDetail> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    detail(&conn, &id)
}

#[tauri::command]
pub fn get_home(state: State<Db>) -> CmdResult<Value> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {} FROM pages p JOIN recents r ON r.page_id=p.id
             WHERE p.trashed_at IS NULL AND p.is_template=0 AND p.type!='row'
             ORDER BY r.visited_at DESC LIMIT 8",
            PAGE_COLS
                .split(',')
                .map(|c| format!("p.{c}"))
                .collect::<Vec<_>>()
                .join(",")
        ))
        .map_err(|e| e.to_string())?;
    let recents = stmt
        .query_map([], page_from_row)
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .map(|p| page_summary(&p))
        .collect::<Vec<_>>();

    let mut stmt = conn
        .prepare(&format!(
            "SELECT {} FROM pages WHERE is_favorite=1 AND trashed_at IS NULL AND is_template=0 AND type!='row' ORDER BY updated_at DESC",
            PAGE_COLS
        ))
        .map_err(|e| e.to_string())?;
    let favorites = stmt
        .query_map([], page_from_row)
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .map(|p| page_summary(&p))
        .collect::<Vec<_>>();

    let mut stmt = conn
        .prepare(&format!(
            "SELECT {} FROM pages WHERE is_template=1 ORDER BY position",
            PAGE_COLS
        ))
        .map_err(|e| e.to_string())?;
    let templates = stmt
        .query_map([], page_from_row)
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .map(|p| page_summary(&p))
        .collect::<Vec<_>>();

    let page_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pages WHERE is_template=0 AND trashed_at IS NULL AND type!='row'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let block_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM blocks", [], |r| r.get(0))
        .unwrap_or(0);
    let db_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM databases", [], |r| r.get(0))
        .unwrap_or(0);
    let settings = all_settings(&conn);
    Ok(json!({
        "recents": recents,
        "favorites": favorites,
        "templates": templates,
        "counts": { "pages": page_count, "blocks": block_count, "databases": db_count },
        "settings": settings
    }))
}

#[tauri::command]
pub fn visit_page(state: State<Db>, id: String) -> CmdResult<()> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    if get_page(&conn, &id).is_err() {
        return Ok(());
    }
    conn.execute(
        "INSERT INTO recents(page_id,visited_at) VALUES(?1,?2)
         ON CONFLICT(page_id) DO UPDATE SET visited_at=excluded.visited_at",
        params![id, now()],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM recents WHERE page_id NOT IN (SELECT page_id FROM recents ORDER BY visited_at DESC LIMIT 60)",
        [],
    )
    .ok();
    Ok(())
}

#[tauri::command]
pub fn create_page(
    state: State<Db>,
    parent_id: Option<String>,
    title: Option<String>,
    page_type: Option<String>,
) -> CmdResult<PageDetail> {
    let mut conn = state.0.lock().map_err(|e| e.to_string())?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let pos = next_position(&tx, parent_id.as_deref())?;
    let ty = page_type.unwrap_or_else(|| "page".into());
    let mut page = empty_page(
        parent_id.clone(),
        title.unwrap_or_default(),
        if ty == "database" { "database" } else { "page" },
        pos,
    );
    if ty == "database" {
        let db = default_database(&page);
        page.database_id = Some(db.id.clone());
        page.title = if page.title.is_empty() {
            "Untitled database".into()
        } else {
            page.title
        };
        page.icon = Some("🗂️".into());
        insert_page(&tx, &page)?;
        tx.execute(
            "INSERT INTO databases(id,page_id,name,properties,views,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                db.id,
                page.id,
                page.title,
                serde_json::to_string(&db.properties).unwrap(),
                serde_json::to_string(&db.views).unwrap(),
                db.created_at,
                db.updated_at
            ],
        )
        .map_err(|e| e.to_string())?;
    } else {
        insert_page(&tx, &page)?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    detail(&conn, &page.id)
}

#[tauri::command]
pub fn update_page(state: State<Db>, id: String, patch: Value) -> CmdResult<Page> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let mut p = get_page(&conn, &id)?;
    if let Some(obj) = patch.as_object() {
        for (k, v) in obj {
            match k.as_str() {
                "title" => p.title = v.as_str().unwrap_or_default().to_string(),
                "icon" => p.icon = v.as_str().map(|s| s.to_string()),
                "cover" => p.cover = v.as_str().map(|s| s.to_string()),
                "coverType" => p.cover_type = v.as_str().unwrap_or("none").to_string(),
                "coverPosition" => p.cover_position = v.as_i64().unwrap_or(50),
                "isFavorite" => p.is_favorite = v.as_bool().unwrap_or(false),
                "locked" => p.locked = v.as_bool().unwrap_or(false),
                "font" => p.font = v.as_str().unwrap_or("default").to_string(),
                "smallText" => p.small_text = v.as_bool().unwrap_or(false),
                "fullWidth" => p.full_width = v.as_bool().unwrap_or(false),
                "parentId" => p.parent_id = v.as_str().map(|s| s.to_string()),
                "position" => p.position = v.as_f64().unwrap_or(p.position),
                _ => {}
            }
        }
    }
    p.updated_at = now();
    conn.execute(
        "UPDATE pages SET parent_id=?1,title=?2,icon=?3,cover=?4,cover_type=?5,cover_position=?6,position=?7,is_favorite=?8,locked=?9,font=?10,small_text=?11,full_width=?12,updated_at=?13 WHERE id=?14",
        params![
            p.parent_id,
            p.title,
            p.icon,
            p.cover,
            p.cover_type,
            p.cover_position,
            p.position,
            p.is_favorite as i64,
            p.locked as i64,
            p.font,
            p.small_text as i64,
            p.full_width as i64,
            p.updated_at,
            p.id
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(p)
}

#[tauri::command]
pub fn save_blocks(state: State<Db>, page_id: String, blocks: Vec<BlockInput>) -> CmdResult<Page> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    let page = get_page(&tx, &page_id)?;
    // Keep the state as it was before this edit so history has something to restore.
    snapshot(&tx, &page_id, false).ok();
    tx.execute("DELETE FROM blocks WHERE page_id=?1", params![page_id])
        .map_err(|e| e.to_string())?;
    let t = now();
    for (i, b) in blocks.iter().enumerate() {
        let pos = if b.position == 0.0 {
            (i + 1) as f64
        } else {
            b.position
        };
        let block = Block {
            id: b.id.clone(),
            page_id: page_id.clone(),
            parent_id: b.parent_id.clone(),
            block_type: b.block_type.clone(),
            html: b.html.clone(),
            text: b.text.clone(),
            props: if b.props.is_null() { json!({}) } else { b.props.clone() },
            position: pos,
            created_at: t.clone(),
            updated_at: t.clone(),
        };
        insert_block(&tx, &block)?;
    }
    tx.execute(
        "UPDATE pages SET updated_at=?1,last_edited_by=?2 WHERE id=?3",
        params![t, "You", page_id],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    let mut p = page;
    p.updated_at = t;
    Ok(p)
}

#[tauri::command]
pub fn move_page(
    state: State<Db>,
    id: String,
    parent_id: Option<String>,
    position: Option<f64>,
) -> CmdResult<()> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    if let Some(target) = &parent_id {
        if target == &id {
            return Err("A page cannot be moved inside itself".into());
        }
        let mut guard = Some(target.clone());
        while let Some(cur) = guard {
            if cur == id {
                return Err("A page cannot be moved inside one of its children".into());
            }
            guard = get_page(&conn, &cur).ok().and_then(|p| p.parent_id);
        }
    }
    let pos = match position {
        Some(p) => p,
        None => next_position(&conn, parent_id.as_deref())?,
    };
    conn.execute(
        "UPDATE pages SET parent_id=?1,position=?2,updated_at=?3 WHERE id=?4",
        params![parent_id, pos, now(), id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn collect_ids(conn: &rusqlite::Connection, id: &str) -> CmdResult<Vec<String>> {
    let mut ids = vec![id.to_string()];
    ids.extend(descendants(conn, id)?.into_iter().map(|p| p.id));
    Ok(ids)
}

#[tauri::command]
pub fn trash_page(state: State<Db>, id: String) -> CmdResult<usize> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let ids = collect_ids(&conn, &id)?;
    let t = now();
    for pid in &ids {
        conn.execute(
            "UPDATE pages SET trashed_at=?1,updated_at=?1 WHERE id=?2",
            params![t, pid],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(ids.len())
}

#[tauri::command]
pub fn restore_page(state: State<Db>, id: String) -> CmdResult<usize> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let ids = collect_ids(&conn, &id)?;
    let t = now();
    for pid in &ids {
        conn.execute(
            "UPDATE pages SET trashed_at=NULL,updated_at=?1 WHERE id=?2",
            params![t, pid],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(ids.len())
}

#[tauri::command]
pub fn delete_page_forever(state: State<Db>, id: String) -> CmdResult<usize> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let ids = collect_ids(&conn, &id)?;
    for pid in &ids {
        conn.execute("DELETE FROM blocks WHERE page_id=?1", params![pid]).ok();
        conn.execute("DELETE FROM comments WHERE page_id=?1", params![pid]).ok();
        conn.execute("DELETE FROM snapshots WHERE page_id=?1", params![pid]).ok();
        conn.execute("DELETE FROM recents WHERE page_id=?1", params![pid]).ok();
        conn.execute("DELETE FROM db_rows WHERE page_id=?1", params![pid]).ok();
        conn.execute("DELETE FROM databases WHERE page_id=?1", params![pid]).ok();
        conn.execute("DELETE FROM pages WHERE id=?1", params![pid]).ok();
    }
    Ok(ids.len())
}

#[tauri::command]
pub fn get_trash(state: State<Db>) -> CmdResult<Vec<TrashItem>> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {} FROM pages WHERE trashed_at IS NOT NULL AND type!='row' ORDER BY trashed_at DESC",
            PAGE_COLS
        ))
        .map_err(|e| e.to_string())?;
    let pages: Vec<Page> = stmt
        .query_map([], page_from_row)
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    let trashed: std::collections::HashSet<String> = pages.iter().map(|p| p.id.clone()).collect();
    let mut out = Vec::new();
    for p in pages {
        let parent_trashed = p
            .parent_id
            .as_ref()
            .map(|pid| trashed.contains(pid))
            .unwrap_or(false);
        if parent_trashed {
            continue;
        }
        let parent_title = p
            .parent_id
            .as_ref()
            .and_then(|pid| get_page(&conn, pid).ok())
            .map(|x| x.title);
        out.push(TrashItem {
            page: page_summary(&p),
            parent_title,
            deleted_at: p.trashed_at.clone().unwrap_or_default(),
        });
    }
    Ok(out)
}

#[tauri::command]
pub fn search(state: State<Db>, query: String) -> CmdResult<Vec<SearchResult>> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let q = query.trim().to_string();
    if q.is_empty() {
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {} FROM pages p JOIN recents r ON r.page_id=p.id
                 WHERE p.trashed_at IS NULL AND p.is_template=0 AND p.type!='row'
                 ORDER BY r.visited_at DESC LIMIT 12",
                PAGE_COLS
                    .split(',')
                    .map(|c| format!("p.{c}"))
                    .collect::<Vec<_>>()
                    .join(",")
            ))
            .map_err(|e| e.to_string())?;
        let pages = stmt
            .query_map([], page_from_row)
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect::<Vec<_>>();
        return Ok(pages
            .into_iter()
            .map(|p| SearchResult {
                page: page_summary(&p),
                snippet: "Recently visited".into(),
                matched: "page".into(),
            })
            .collect());
    }
    let like = format!("%{}%", q.to_lowercase());
    let mut results: Vec<SearchResult> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {} FROM pages WHERE trashed_at IS NULL AND is_template=0 AND type!='row'
             AND LOWER(title) LIKE ?1 ORDER BY updated_at DESC LIMIT 25",
            PAGE_COLS
        ))
        .map_err(|e| e.to_string())?;
    for p in stmt
        .query_map(params![like], page_from_row)
        .map_err(|e| e.to_string())?
        .flatten()
    {
        if seen.insert(p.id.clone()) {
            results.push(SearchResult {
                page: page_summary(&p),
                snippet: "".into(),
                matched: "title".into(),
            });
        }
    }

    let mut stmt = conn
        .prepare(
            "SELECT b.page_id, b.text, p.id
             FROM blocks b JOIN pages p ON p.id=b.page_id
             WHERE p.trashed_at IS NULL AND p.is_template=0 AND p.type!='row'
             AND LOWER(b.text) LIKE ?1 ORDER BY p.updated_at DESC LIMIT 60",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![like], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    for row in rows.flatten() {
        let (page_id, text, _) = row;
        if seen.contains(&page_id) {
            continue;
        }
        let lower = text.to_lowercase();
        let idx = lower.find(&q.to_lowercase()).unwrap_or(0);
        let start = idx.saturating_sub(45);
        let mut snippet: String = text.chars().skip(start).take(140).collect();
        if start > 0 {
            snippet = format!("…{snippet}");
        }
        if let Ok(p) = get_page(&conn, &page_id) {
            seen.insert(page_id);
            results.push(SearchResult {
                page: page_summary(&p),
                snippet,
                matched: "content".into(),
            });
        }
    }
    results.truncate(50);
    Ok(results)
}

#[tauri::command]
pub fn get_templates(state: State<Db>) -> CmdResult<Vec<Value>> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {} FROM pages WHERE is_template=1 ORDER BY position,title",
            PAGE_COLS
        ))
        .map_err(|e| e.to_string())?;
    let pages = stmt
        .query_map([], page_from_row)
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect::<Vec<_>>();
    let mut out = Vec::new();
    for p in pages {
        let desc: Option<String> = conn
            .query_row(
                "SELECT text FROM blocks WHERE page_id=?1 AND text!='' ORDER BY position LIMIT 1",
                params![p.id],
                |r| r.get(0),
            )
            .optional()
            .ok()
            .flatten();
        out.push(json!({ "page": page_summary(&p), "description": desc.unwrap_or_default() }));
    }
    Ok(out)
}

fn clone_blocks(
    conn: &rusqlite::Connection,
    src_page: &str,
    dst_page: &str,
    t: &str,
) -> CmdResult<()> {
    let mut stmt = conn
        .prepare("SELECT * FROM blocks WHERE page_id=?1 ORDER BY position")
        .map_err(|e| e.to_string())?;
    let blocks: Vec<Block> = stmt
        .query_map(params![src_page], block_from_row)
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    let mut map = std::collections::HashMap::new();
    for b in &blocks {
        map.insert(b.id.clone(), new_id());
    }
    for b in blocks {
        let nb = Block {
            id: map.get(&b.id).cloned().unwrap_or_else(new_id),
            page_id: dst_page.to_string(),
            parent_id: b.parent_id.as_ref().and_then(|p| map.get(p).cloned()),
            block_type: b.block_type,
            html: b.html,
            text: b.text,
            props: b.props,
            position: b.position,
            created_at: t.to_string(),
            updated_at: t.to_string(),
        };
        insert_block(conn, &nb)?;
    }
    Ok(())
}

fn create_page_from_source(
    conn: &rusqlite::Connection,
    src: &Page,
    parent_id: Option<String>,
    title: String,
    t: &str,
) -> CmdResult<Page> {
    let pos = next_position(conn, parent_id.as_deref())?;
    let mut page = empty_page(parent_id.clone(), title, "page", pos);
    page.icon = src.icon.clone();
    page.cover = src.cover.clone();
    page.cover_type = src.cover_type.clone();
    page.cover_position = src.cover_position;
    page.font = src.font.clone();
    insert_page(conn, &page)?;
    clone_blocks(conn, &src.id, &page.id, t)?;
    Ok(page)
}

#[tauri::command]
pub fn create_from_template(
    state: State<Db>,
    template_id: String,
    parent_id: Option<String>,
    title: Option<String>,
) -> CmdResult<PageDetail> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let src = get_page(&conn, &template_id)?;
    let t = now();
    let page = create_page_from_source(
        &conn,
        &src,
        parent_id,
        title.unwrap_or_default(),
        &t,
    )?;
    detail(&conn, &page.id)
}

#[tauri::command]
pub fn duplicate_page(state: State<Db>, id: String) -> CmdResult<PageDetail> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let src = get_page(&conn, &id)?;
    let t = now();
    let pos = next_position(&conn, src.parent_id.as_deref())?;
    let mut new_page = empty_page(
        src.parent_id.clone(),
        if src.title.is_empty() {
            "Untitled".into()
        } else {
            format!("{} copy", src.title)
        },
        &src.page_type,
        pos,
    );
    new_page.icon = src.icon.clone();
    new_page.cover = src.cover.clone();
    new_page.cover_type = src.cover_type.clone();
    new_page.cover_position = src.cover_position;
    new_page.font = src.font.clone();
    new_page.small_text = src.small_text;
    new_page.full_width = src.full_width;

    if let Ok(db) = conn.query_row(
        "SELECT * FROM databases WHERE page_id=?1",
        params![id],
        database_from_row,
    ) {
        let new_db_id = new_id();
        new_page.page_type = "database".into();
        new_page.database_id = Some(new_db_id.clone());
        insert_page(&conn, &new_page)?;
        conn.execute(
            "INSERT INTO databases(id,page_id,name,properties,views,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                new_db_id,
                new_page.id,
                new_page.title,
                serde_json::to_string(&db.properties).unwrap(),
                serde_json::to_string(&db.views).unwrap(),
                t,
                t
            ],
        )
        .map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT r.*, p.title AS title, p.icon AS icon, p.trashed_at AS trashed_at
                 FROM db_rows r JOIN pages p ON p.id=r.page_id WHERE r.database_id=?1",
            )
            .map_err(|e| e.to_string())?;
        let rows: Vec<DbRow> = stmt
            .query_map(params![db.id], db_row_from_row)
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        for (i, row) in rows.into_iter().enumerate() {
            let row_page = empty_page(
                Some(new_page.id.clone()),
                row.title.clone(),
                "row",
                (i + 1) as f64,
            );
            let mut row_page = row_page;
            row_page.icon = row.icon.clone();
            row_page.database_id = Some(new_db_id.clone());
            insert_page(&conn, &row_page)?;
            clone_blocks(&conn, &row.page_id, &row_page.id, &t)?;
            conn.execute(
                "INSERT INTO db_rows(id,database_id,page_id,values_json,position,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![
                    new_id(),
                    new_db_id,
                    row_page.id,
                    serde_json::to_string(&row.values).unwrap(),
                    row.position,
                    t,
                    t
                ],
            )
            .map_err(|e| e.to_string())?;
        }
    } else {
        insert_page(&conn, &new_page)?;
        clone_blocks(&conn, &src.id, &new_page.id, &t)?;
    }
    detail(&conn, &new_page.id)
}

#[tauri::command]
pub fn get_versions_cmd(state: State<Db>, page_id: String) -> CmdResult<Vec<Value>> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    get_versions(&conn, &page_id)
}

#[tauri::command]
pub fn restore_version(state: State<Db>, page_id: String, version_id: String) -> CmdResult<PageDetail> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let data: String = conn
        .query_row(
            "SELECT data FROM snapshots WHERE id=?1 AND page_id=?2",
            params![version_id, page_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    let v: Value = serde_json::from_str(&data).map_err(|e| e.to_string())?;
    // Preserve the current state before replacing it, as the dialog promises.
    snapshot(&conn, &page_id, true).ok();
    if let Some(p) = v.get("page") {
        let mut page: Page = serde_json::from_value(p.clone()).map_err(|e| e.to_string())?;
        page.trashed_at = None;
        page.updated_at = now();
        page.parent_id = get_page(&conn, &page_id).ok().and_then(|x| x.parent_id);
        conn.execute(
            "UPDATE pages SET title=?1,icon=?2,cover=?3,cover_type=?4,cover_position=?5,font=?6,small_text=?7,full_width=?8,updated_at=?9 WHERE id=?10",
            params![
                page.title,
                page.icon,
                page.cover,
                page.cover_type,
                page.cover_position,
                page.font,
                page.small_text as i64,
                page.full_width as i64,
                page.updated_at,
                page_id
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    if let Some(blocks) = v.get("blocks").and_then(|b| b.as_array()) {
        conn.execute("DELETE FROM blocks WHERE page_id=?1", params![page_id])
            .map_err(|e| e.to_string())?;
        for b in blocks {
            if let Ok(block) = serde_json::from_value::<Block>(b.clone()) {
                let mut block = block;
                block.page_id = page_id.clone();
                insert_block(&conn, &block)?;
            }
        }
    }
    detail(&conn, &page_id)
}

#[tauri::command]
pub fn save_workspace(state: State<Db>, name: String, icon: Option<String>) -> CmdResult<Workspace> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let ws = get_workspace(&conn)?;
    conn.execute(
        "UPDATE workspace SET name=?1,icon=?2 WHERE id=?3",
        params![name, icon, ws.id],
    )
    .map_err(|e| e.to_string())?;
    get_workspace(&conn)
}

#[tauri::command]
pub fn get_settings(state: State<Db>) -> CmdResult<Value> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    Ok(all_settings(&conn))
}

#[tauri::command(rename = "set_setting")]
pub fn set_setting_cmd(state: State<Db>, key: String, value: Value) -> CmdResult<()> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    crate::db::set_setting(&conn, &key, &value)
}

#[tauri::command]
pub fn set_settings(state: State<Db>, settings: Value) -> CmdResult<()> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    if let Some(obj) = settings.as_object() {
        for (k, v) in obj {
            crate::db::set_setting(&conn, k, v)?;
        }
    }
    Ok(())
}

#[tauri::command]
pub fn create_comment(
    state: State<Db>,
    page_id: String,
    block_id: Option<String>,
    html: String,
) -> CmdResult<Comment> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let id = new_id();
    let author = get_setting(&conn, "userName")
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "You".into());
    conn.execute(
        "INSERT INTO comments(id,page_id,block_id,html,author,created_at,resolved) VALUES(?1,?2,?3,?4,?5,?6,0)",
        params![id, page_id, block_id, html, author, now()],
    )
    .map_err(|e| e.to_string())?;
    Ok(Comment {
        id,
        page_id,
        block_id,
        html,
        author,
        created_at: now(),
        resolved: false,
    })
}

#[tauri::command]
pub fn resolve_comment(state: State<Db>, id: String, resolved: bool) -> CmdResult<()> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE comments SET resolved=?1 WHERE id=?2",
        params![resolved as i64, id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn delete_comment(state: State<Db>, id: String) -> CmdResult<()> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM comments WHERE id=?1", params![id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn get_database(state: State<Db>, page_id: String) -> CmdResult<Value> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let db: Database = conn
        .query_row(
            "SELECT * FROM databases WHERE page_id=?1",
            params![page_id],
            database_from_row,
        )
        .map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT r.*, p.title AS title, p.icon AS icon, p.trashed_at AS trashed_at
             FROM db_rows r JOIN pages p ON p.id=r.page_id
             WHERE r.database_id=?1 ORDER BY r.position, r.rowid",
        )
        .map_err(|e| e.to_string())?;
    let rows: Vec<DbRow> = stmt
        .query_map(params![db.id], db_row_from_row)
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    Ok(json!({ "database": db, "rows": rows }))
}

#[tauri::command]
pub fn update_database(
    state: State<Db>,
    id: String,
    name: Option<String>,
    properties: Option<Value>,
    views: Option<Value>,
) -> CmdResult<Database> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let mut db: Database = conn
        .query_row(
            "SELECT * FROM databases WHERE id=?1",
            params![id],
            database_from_row,
        )
        .map_err(|e| e.to_string())?;
    if let Some(n) = name {
        db.name = n.clone();
        conn.execute(
            "UPDATE pages SET title=?1,updated_at=?2 WHERE id=?3",
            params![n, now(), db.page_id],
        )
        .ok();
    }
    if let Some(p) = properties {
        db.properties = p;
    }
    if let Some(v) = views {
        db.views = v;
    }
    db.updated_at = now();
    conn.execute(
        "UPDATE databases SET name=?1,properties=?2,views=?3,updated_at=?4 WHERE id=?5",
        params![
            db.name,
            serde_json::to_string(&db.properties).unwrap(),
            serde_json::to_string(&db.views).unwrap(),
            db.updated_at,
            db.id
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(db)
}

fn title_prop(properties: &Value) -> String {
    properties
        .as_array()
        .and_then(|arr| {
            arr.iter()
                .find(|p| p.get("type").and_then(|t| t.as_str()) == Some("title"))
        })
        .and_then(|p| p.get("id").and_then(|id| id.as_str()))
        .unwrap_or("title")
        .to_string()
}

#[tauri::command]
pub fn create_row(
    state: State<Db>,
    database_id: String,
    values: Value,
    position: Option<f64>,
) -> CmdResult<Value> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let db: Database = conn
        .query_row(
            "SELECT * FROM databases WHERE id=?1",
            params![database_id],
            database_from_row,
        )
        .map_err(|e| e.to_string())?;
    let tp = title_prop(&db.properties);
    let title = values
        .get(&tp)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let pos = match position {
        Some(p) => p,
        None => conn
            .query_row(
                "SELECT MAX(position) FROM db_rows WHERE database_id=?1",
                params![database_id],
                |r| r.get::<_, Option<f64>>(0),
            )
            .ok()
            .flatten()
            .unwrap_or(0.0)
            + 1.0,
    };
    let t = now();
    let mut page = empty_page(
        Some(db.page_id.clone()),
        if title.is_empty() { "Untitled".into() } else { title.clone() },
        "row",
        pos,
    );
    page.database_id = Some(db.id.clone());
    insert_page(&conn, &page)?;
    let row_id = new_id();
    conn.execute(
        "INSERT INTO db_rows(id,database_id,page_id,values_json,position,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",
        params![
            row_id,
            db.id,
            page.id,
            serde_json::to_string(&values).unwrap(),
            pos,
            t,
            t
        ],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE databases SET updated_at=?1 WHERE id=?2",
        params![t, db.id],
    )
    .ok();
    let row = conn
        .query_row(
            "SELECT r.*, p.title AS title, p.icon AS icon, p.trashed_at AS trashed_at
             FROM db_rows r JOIN pages p ON p.id=r.page_id WHERE r.id=?1",
            params![row_id],
            db_row_from_row,
        )
        .map_err(|e| e.to_string())?;
    Ok(json!({ "row": row, "page": page_summary(&page) }))
}

#[tauri::command]
pub fn update_row(
    state: State<Db>,
    row_id: String,
    values: Option<Value>,
    position: Option<f64>,
) -> CmdResult<DbRow> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let mut row: DbRow = conn
        .query_row(
            "SELECT r.*, p.title AS title, p.icon AS icon, p.trashed_at AS trashed_at
             FROM db_rows r JOIN pages p ON p.id=r.page_id WHERE r.id=?1",
            params![row_id],
            db_row_from_row,
        )
        .map_err(|e| e.to_string())?;
    if let Some(v) = values {
        row.values = v;
    }
    if let Some(p) = position {
        row.position = p;
    }
    let db: Database = conn
        .query_row(
            "SELECT * FROM databases WHERE id=?1",
            params![row.database_id],
            database_from_row,
        )
        .map_err(|e| e.to_string())?;
    let tp = title_prop(&db.properties);
    let title = row
        .values
        .get(&tp)
        .and_then(|v| v.as_str())
        .unwrap_or("Untitled")
        .to_string();
    let t = now();
    conn.execute(
        "UPDATE db_rows SET values_json=?1,position=?2,updated_at=?3 WHERE id=?4",
        params![
            serde_json::to_string(&row.values).unwrap(),
            row.position,
            t,
            row.id
        ],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE pages SET title=?1,updated_at=?2 WHERE id=?3",
        params![title, t, row.page_id],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE databases SET updated_at=?1 WHERE id=?2",
        params![t, row.database_id],
    )
    .ok();
    row.title = title;
    row.updated_at = t;
    Ok(row)
}

#[tauri::command]
pub fn delete_row(state: State<Db>, row_id: String) -> CmdResult<()> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let page_id: String = conn
        .query_row(
            "SELECT page_id FROM db_rows WHERE id=?1",
            params![row_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    let t = now();
    conn.execute(
        "UPDATE pages SET trashed_at=?1,updated_at=?1 WHERE id=?2",
        params![t, page_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn restore_row(state: State<Db>, row_id: String) -> CmdResult<()> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let page_id: String = conn
        .query_row(
            "SELECT page_id FROM db_rows WHERE id=?1",
            params![row_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    let t = now();
    conn.execute(
        "UPDATE pages SET trashed_at=NULL,updated_at=?1 WHERE id=?2",
        params![t, page_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn sanitize_filename(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| if r#"\/:*?"<>|"#.contains(c) { '-' } else { c })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').to_string();
    if trimmed.is_empty() {
        "Untitled".into()
    } else {
        trimmed.chars().take(80).collect()
    }
}

async fn save_dialog(
    window: &WebviewWindow,
    default_name: &str,
    filter_name: &str,
    ext: &str,
) -> Option<PathBuf> {
    rfd::AsyncFileDialog::new()
        .set_parent(window)
        .set_file_name(default_name)
        .add_filter(filter_name, &[ext])
        .save_file()
        .await
        .map(|f| f.path().to_path_buf())
}

#[tauri::command]
pub async fn export_markdown(
    state: State<'_, Db>,
    window: WebviewWindow,
    page_id: String,
) -> CmdResult<Option<String>> {
    let (page, blocks, database, rows) = {
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        let page = get_page(&conn, &page_id)?;
        let mut stmt = conn
            .prepare("SELECT * FROM blocks WHERE page_id=?1 ORDER BY position")
            .map_err(|e| e.to_string())?;
        let blocks: Vec<Block> = stmt
            .query_map(params![page_id], block_from_row)
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        let database: Option<Database> = conn
            .query_row(
                "SELECT * FROM databases WHERE page_id=?1",
                params![page_id],
                database_from_row,
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let mut rows = Vec::new();
        if let Some(db) = &database {
            let mut stmt = conn
                .prepare(
                    "SELECT r.*, p.title AS title, p.icon AS icon, p.trashed_at AS trashed_at
                     FROM db_rows r JOIN pages p ON p.id=r.page_id WHERE r.database_id=?1 ORDER BY r.position",
                )
                .map_err(|e| e.to_string())?;
            rows = stmt
                .query_map(params![db.id], db_row_from_row)
                .map_err(|e| e.to_string())?
                .filter_map(|r| r.ok())
                .collect();
        }
        (page, blocks, database, rows)
    };
    let md = markdown::to_markdown(&page, &blocks, &database, &rows);
    let path = save_dialog(
        &window,
        &format!("{}.md", sanitize_filename(if page.title.is_empty() { "Untitled" } else { &page.title })),
        "Markdown",
        "md",
    )
    .await;
    match path {
        Some(p) => {
            std::fs::write(&p, md).map_err(|e| e.to_string())?;
            Ok(Some(p.to_string_lossy().to_string()))
        }
        None => Ok(None),
    }
}

#[tauri::command]
pub async fn export_html(
    state: State<'_, Db>,
    window: WebviewWindow,
    page_id: String,
) -> CmdResult<Option<String>> {
    let (page, blocks) = {
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        let page = get_page(&conn, &page_id)?;
        let mut stmt = conn
            .prepare("SELECT * FROM blocks WHERE page_id=?1 ORDER BY position")
            .map_err(|e| e.to_string())?;
        let blocks: Vec<Block> = stmt
            .query_map(params![page_id], block_from_row)
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        (page, blocks)
    };
    let html = markdown::to_html_document(&page, &blocks);
    let path = save_dialog(
        &window,
        &format!("{}.html", sanitize_filename(if page.title.is_empty() { "Untitled" } else { &page.title })),
        "HTML",
        "html",
    )
    .await;
    match path {
        Some(p) => {
            std::fs::write(&p, html).map_err(|e| e.to_string())?;
            Ok(Some(p.to_string_lossy().to_string()))
        }
        None => Ok(None),
    }
}

#[tauri::command]
pub async fn export_backup(
    state: State<'_, Db>,
    window: WebviewWindow,
    app: AppHandle,
) -> CmdResult<Option<String>> {
    let data = {
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        markdown::backup_json(&conn)?
    };
    let date = chrono::Local::now().format("%Y-%m-%d").to_string();
    let path = save_dialog(
        &window,
        &format!("IdeaCraft-backup-{date}.json"),
        "IdeaCraft backup",
        "json",
    )
    .await;
    let _ = app;
    match path {
        Some(p) => {
            std::fs::write(&p, serde_json::to_string_pretty(&data).unwrap()).map_err(|e| e.to_string())?;
            Ok(Some(p.to_string_lossy().to_string()))
        }
        None => Ok(None),
    }
}

#[tauri::command]
pub async fn import_markdown(
    state: State<'_, Db>,
    window: WebviewWindow,
    parent_id: Option<String>,
) -> CmdResult<Option<PageDetail>> {
    let file = rfd::AsyncFileDialog::new()
        .set_parent(&window)
        .add_filter("Markdown", &["md", "markdown", "txt"])
        .pick_file()
        .await;
    let Some(file) = file else { return Ok(None) };
    let bytes = std::fs::read(file.path()).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&bytes).to_string();
    let (title, blocks) = markdown::parse_markdown(&text);
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let pos = next_position(&conn, parent_id.as_deref())?;
    let page = empty_page(parent_id, title, "page", pos);
    insert_page(&conn, &page)?;
    for mut b in blocks {
        b.page_id = page.id.clone();
        insert_block(&conn, &b)?;
    }
    detail(&conn, &page.id).map(Some)
}

#[tauri::command]
pub async fn import_backup(
    state: State<'_, Db>,
    window: WebviewWindow,
    replace: bool,
) -> CmdResult<Option<Value>> {
    let file = rfd::AsyncFileDialog::new()
        .set_parent(&window)
        .add_filter("IdeaCraft backup", &["json"])
        .pick_file()
        .await;
    let Some(file) = file else { return Ok(None) };
    let bytes = std::fs::read(file.path()).map_err(|e| e.to_string())?;
    let data: Value = serde_json::from_slice(&bytes).map_err(|e| format!("Invalid backup: {e}"))?;
    let mut conn = state.0.lock().map_err(|e| e.to_string())?;
    let result = markdown::restore_backup(&mut conn, &data, replace)?;
    Ok(Some(result))
}

#[tauri::command]
pub async fn pick_image(window: WebviewWindow) -> CmdResult<Option<Value>> {
    let file = rfd::AsyncFileDialog::new()
        .set_parent(&window)
        .add_filter("Images", &["png", "jpg", "jpeg", "gif", "webp", "svg", "bmp"])
        .pick_file()
        .await;
    let Some(file) = file else { return Ok(None) };
    let path = file.path().to_path_buf();
    let meta = std::fs::metadata(&path).map_err(|e| e.to_string())?;
    if meta.len() > 12 * 1024 * 1024 {
        return Err("Image is larger than 12 MB".into());
    }
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png")
        .to_lowercase();
    let mime = match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "bmp" => "image/bmp",
        _ => "image/png",
    };
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    Ok(Some(json!({
        "dataUrl": format!("data:{mime};base64,{encoded}"),
        "name": path.file_name().and_then(|n| n.to_str()).unwrap_or("image"),
        "size": meta.len()
    })))
}

#[tauri::command]
pub fn copy_text(text: String) -> CmdResult<()> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    clipboard.set_text(text).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn open_url(url: String) -> CmdResult<()> {
    if !(url.starts_with("http://") || url.starts_with("https://") || url.starts_with("mailto:")) {
        return Err("Unsupported URL".into());
    }
    open::that(url).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn open_data_folder(app: AppHandle) -> CmdResult<()> {
    let path = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&path).ok();
    open::that(path).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn app_info(app: AppHandle) -> CmdResult<Value> {
    let data_path = app
        .path()
        .app_data_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    Ok(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "dataPath": data_path,
        "platform": std::env::consts::OS,
        "arch": std::env::consts::ARCH
    }))
}

#[tauri::command]
pub fn window_action(window: tauri::Window, state: State<Db>, action: String) -> CmdResult<()> {
    match action.as_str() {
        "minimize" => window.minimize().map_err(|e| e.to_string())?,
        "maximize" => {
            if window.is_maximized().unwrap_or(false) {
                window.unmaximize().map_err(|e| e.to_string())?
            } else {
                window.maximize().map_err(|e| e.to_string())?
            }
        }
        "fullscreen" => {
            let fs = window.is_fullscreen().unwrap_or(false);
            window.set_fullscreen(!fs).map_err(|e| e.to_string())?
        }
        "close" => {
            let keep = {
                let conn = state.0.lock().map_err(|e| e.to_string())?;
                get_setting(&conn, "keepInBackground")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
            };
            if keep {
                window.hide().map_err(|e| e.to_string())?;
            } else {
                window.close().map_err(|e| e.to_string())?;
            }
        }
        "hide" => window.hide().map_err(|e| e.to_string())?,
        "show" => {
            window.unminimize().map_err(|e| e.to_string())?;
            window.show().map_err(|e| e.to_string())?;
            window.set_focus().map_err(|e| e.to_string())?;
        }
        _ => return Err("Unknown window action".into()),
    }
    Ok(())
}

#[tauri::command]
pub fn window_state(window: tauri::Window) -> CmdResult<Value> {
    let size = window.inner_size().map_err(|e| e.to_string())?;
    let pos = window.outer_position().ok();
    Ok(json!({
        "maximized": window.is_maximized().unwrap_or(false),
        "fullscreen": window.is_fullscreen().unwrap_or(false),
        "visible": window.is_visible().unwrap_or(true),
        "width": size.width,
        "height": size.height,
        "x": pos.map(|p| p.x),
        "y": pos.map(|p| p.y)
    }))
}

#[tauri::command]
pub fn save_window_state(state: State<Db>, window: tauri::Window) -> CmdResult<()> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let size = window.inner_size().ok();
    let pos = window.outer_position().ok();
    let maximized = window.is_maximized().unwrap_or(false);
    if let Some(s) = size {
        let v = json!({
            "width": s.width, "height": s.height,
            "x": pos.map(|p| p.x), "y": pos.map(|p| p.y),
            "maximized": maximized
        });
        crate::db::set_setting(&conn, "windowState", &v)?;
    }
    Ok(())
}

#[tauri::command]
pub async fn pick_sync_folder(
    state: State<'_, Db>,
    window: WebviewWindow,
) -> CmdResult<Option<String>> {
    let folder = rfd::AsyncFileDialog::new()
        .set_parent(&window)
        .pick_folder()
        .await;
    let Some(folder) = folder else { return Ok(None) };
    let path = folder.path().to_string_lossy().to_string();
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    crate::db::set_setting(&conn, "syncFolder", &json!(path))?;
    Ok(Some(path))
}

#[tauri::command]
pub async fn sync_now(state: State<'_, Db>, app: AppHandle) -> CmdResult<Value> {
    let (folder, device, device_id) = {
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        let folder = get_setting(&conn, "syncFolder")
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or_default();
        let name = std::env::var("COMPUTERNAME")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| {
                get_setting(&conn, "userName")
                    .and_then(|v| v.as_str().map(|s| s.to_string()))
            })
            .unwrap_or_else(|| "device".into());
        let mut id = get_setting(&conn, "syncDeviceId")
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or_default();
        if id.is_empty() {
            id = new_id();
            crate::db::set_setting(&conn, "syncDeviceId", &json!(id))?;
        }
        (folder, sanitize_filename(&name), id)
    };
    if folder.is_empty() {
        return Err("Choose a sync folder in Settings → Sync".into());
    }
    let dir = PathBuf::from(&folder).join("IdeaCraft");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let short_id = &device_id[..device_id.len().min(8)];
    let file = dir.join(format!("ideacraft-{device}-{short_id}.json"));
    let mut imported = 0usize;
    if file.exists() {
        if let Ok(bytes) = std::fs::read(&file) {
            if let Ok(remote) = serde_json::from_slice::<Value>(&bytes) {
                let mut conn = state.0.lock().map_err(|e| e.to_string())?;
                if let Ok(res) = markdown::merge_backup(&mut conn, &remote) {
                    imported = ["pagesAdded", "pagesUpdated"]
                        .iter()
                        .filter_map(|k| res.get(*k).and_then(|v| v.as_u64()))
                        .sum::<u64>() as usize;
                }
            }
        }
    }
    let merged = {
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        markdown::backup_json(&conn)?
    };
    std::fs::write(
        &file,
        serde_json::to_string_pretty(&merged).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    {
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        crate::db::set_setting(&conn, "lastSync", &json!(now()))?;
    }
    let _ = app;
    Ok(json!({ "path": file.to_string_lossy(), "imported": imported, "time": now() }))
}

#[tauri::command]
pub fn ai_status(state: State<Db>) -> CmdResult<Value> {
    let (enabled, base, model) = {
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        (
            get_setting(&conn, "aiEnabled").and_then(|v| v.as_bool()).unwrap_or(false),
            get_setting(&conn, "aiBaseUrl")
                .and_then(|v| v.as_str().map(|s| s.to_string()))
                .unwrap_or_else(|| "http://127.0.0.1:11434".into()),
            get_setting(&conn, "aiModel")
                .and_then(|v| v.as_str().map(|s| s.to_string()))
                .unwrap_or_else(|| "llama3.2".into()),
        )
    };
    if !enabled {
        return Ok(json!({ "running": false, "enabled": false, "models": [], "model": model }));
    }
    let url = format!("{}/api/tags", base.trim_end_matches('/'));
    match ureq::get(&url).timeout(std::time::Duration::from_secs(3)).call() {
        Ok(resp) => {
            let body: Value = resp.into_json().unwrap_or_else(|_| json!({}));
            let models = body
                .get("models")
                .and_then(|m| m.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|m| m.get("name").and_then(|n| n.as_str()))
                        .map(|s| json!(s))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            Ok(json!({ "running": true, "enabled": true, "models": models, "model": model }))
        }
        Err(_) => Ok(json!({ "running": false, "enabled": true, "models": [], "model": model })),
    }
}

const AI_SYSTEM_PROMPT: &str = include_str!("ai_prompt.md");

#[derive(Deserialize)]
pub struct AiMessage {
    pub role: String,
    pub content: String,
}

#[tauri::command]
pub async fn ai_complete(
    state: State<'_, Db>,
    messages: Vec<AiMessage>,
    context: Option<String>,
    model: Option<String>,
) -> CmdResult<String> {
    let (base, default_model) = {
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        (
            get_setting(&conn, "aiBaseUrl")
                .and_then(|v| v.as_str().map(|s| s.to_string()))
                .unwrap_or_else(|| "http://127.0.0.1:11434".into()),
            get_setting(&conn, "aiModel")
                .and_then(|v| v.as_str().map(|s| s.to_string()))
                .unwrap_or_else(|| "llama3.2".into()),
        )
    };
    let url = format!("{}/api/chat", base.trim_end_matches('/'));
    let mut msgs: Vec<Value> = vec![json!({
        "role": "system",
        "content": AI_SYSTEM_PROMPT
    })];
    if let Some(ctx) = context.filter(|c| !c.trim().is_empty()) {
        msgs.push(json!({
            "role": "system",
            "content": format!("Current page context:\n\n{}", ctx.chars().take(12000).collect::<String>())
        }));
    }
    for m in messages {
        msgs.push(json!({ "role": m.role, "content": m.content }));
    }
    let body = json!({
        "model": model.unwrap_or(default_model),
        "messages": msgs,
        "stream": false,
        "options": { "temperature": 0.6 }
    });
    let result = tauri::async_runtime::spawn_blocking(move || {
        ureq::post(&url)
            .timeout(std::time::Duration::from_secs(180))
            .send_json(body)
            .map_err(|e| e.to_string())
            .and_then(|resp| {
                let v: Value = resp.into_json().map_err(|e| e.to_string())?;
                v.get("message")
                    .and_then(|m| m.get("content"))
                    .and_then(|c| c.as_str())
                    .map(|s| s.to_string())
                    .ok_or_else(|| "AI returned an empty response".to_string())
            })
    })
    .await
    .map_err(|e| e.to_string())?;
    result
}

use tauri_plugin_updater::UpdaterExt;

/// Check the configured update feed. Returns null when up to date.
#[tauri::command]
pub async fn check_updates(app: AppHandle) -> CmdResult<Value> {
    let update = app
        .updater()
        .map_err(|e| format!("Updater not configured: {e}"))?
        .check()
        .await
        .map_err(|e| e.to_string())?;
    Ok(match update {
        Some(u) => json!({
            "available": true,
            "version": u.version,
            "currentVersion": u.current_version,
            "notes": u.body.clone().unwrap_or_default()
        }),
        None => json!({ "available": false })
    })
}

/// Download + install an available update; the updater exits the app to run the installer.
#[tauri::command]
pub async fn install_update(app: AppHandle) -> CmdResult<Value> {
    let updater = app.updater().map_err(|e| e.to_string())?;
    let update = updater
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "No update available".to_string())?;
    let version = update.version.clone();
    let _ = app.emit("update-progress", json!({ "phase": "download" }));
    let mut downloaded: u64 = 0;
    let mut last_sent: u32 = 0;
    let app_for_progress = app.clone();
    update
        .download_and_install(
            &mut |chunk, total| {
                downloaded += chunk as u64;
                if let Some(t) = total {
                    if t > 0 {
                        let pct = ((downloaded as f64 / t as f64) * 100.0).min(100.0) as u32;
                        if pct >= last_sent + 10 {
                            last_sent = pct;
                            let _ = app_for_progress.emit(
                                "update-progress",
                                json!({ "phase": "download", "percent": pct }),
                            );
                        }
                    }
                }
            },
            || {},
        )
        .await
        .map_err(|e| e.to_string())?;
    let _ = app.emit("update-progress", json!({ "phase": "install" }));
    Ok(json!({ "installed": true, "version": version }))
}
