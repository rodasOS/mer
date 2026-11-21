use std::path::Path;

use mer_core::{
    Accent, AgentMemo, Layout, Pane, PaneId, SpaceData, SpaceId, SpaceState, TabData, TabId,
    WorkspaceData,
};
use rusqlite::{Connection, OptionalExtension, params};
use rusqlite_migration::{M, Migrations};

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("migration error: {0}")]
    Migration(#[from] rusqlite_migration::Error),
    #[error("invalid stored layout: {0}")]
    Json(#[from] serde_json::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("corrupt stored data: {0}")]
    Corrupt(String),
}

const SCHEMA_V1: &str = "
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE spaces (
    id TEXT PRIMARY KEY,
    position INTEGER NOT NULL,
    name TEXT NOT NULL,
    state TEXT NOT NULL,
    active_tab INTEGER NOT NULL
);
CREATE TABLE tabs (
    id TEXT PRIMARY KEY,
    space_id TEXT NOT NULL REFERENCES spaces(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    title TEXT,
    layout TEXT NOT NULL,
    focused TEXT NOT NULL
);
CREATE TABLE panes (
    id TEXT PRIMARY KEY,
    tab_id TEXT NOT NULL REFERENCES tabs(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    cwd TEXT,
    title TEXT NOT NULL,
    agent_kind TEXT,
    agent_session TEXT
);
";

/// Colors for spaces and tabs, and sleeping tabs.
const SCHEMA_V2: &str = "
ALTER TABLE spaces ADD COLUMN color TEXT;
ALTER TABLE tabs ADD COLUMN color TEXT;
ALTER TABLE tabs ADD COLUMN sleeping INTEGER NOT NULL DEFAULT 0;
-- Spaces created before colors existed get the same rotation new spaces use.
UPDATE spaces SET color = CASE position % 10
    WHEN 0 THEN 'red' WHEN 1 THEN 'blue' WHEN 2 THEN 'green' WHEN 3 THEN 'orange'
    WHEN 4 THEN 'violet' WHEN 5 THEN 'teal' WHEN 6 THEN 'amber' WHEN 7 THEN 'pink'
    WHEN 8 THEN 'cyan' ELSE 'lime' END;
";

const MIGRATION_LIST: &[M<'static>] = &[M::up(SCHEMA_V1), M::up(SCHEMA_V2)];
const MIGRATIONS: Migrations<'static> = Migrations::from_slice(MIGRATION_LIST);

/// The workspace database.
#[derive(Debug)]
pub struct Store {
    pub(crate) conn: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        if let Some(parent) = path.parent() {
            crate::private::create_dir(parent)?;
        }
        // The database holds the workspace, and `SQLite` gives the -wal and -shm files the same
        // mode as the main file, so it must be private before the first connection.
        crate::private::touch(path)?;
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self, StoreError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> Result<Self, StoreError> {
        conn.pragma_update(None, "foreign_keys", true)?;
        MIGRATIONS.to_latest(&mut conn)?;
        Ok(Self { conn })
    }

    /// Replaces the stored workspace with `data`, atomically.
    pub fn save(&mut self, data: &WorkspaceData) -> Result<(), StoreError> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM spaces", [])?;
        for (space_position, space) in data.spaces.iter().enumerate() {
            tx.execute(
                "INSERT INTO spaces (id, position, name, state, active_tab, color) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    space.id.as_uuid().to_string(),
                    int(space_position),
                    space.name,
                    state_name(space.state),
                    int(space.active_tab),
                    space.color.map(Accent::name)
                ],
            )?;
            for (tab_position, tab) in space.tabs.iter().enumerate() {
                tx.execute(
                    "INSERT INTO tabs (id, space_id, position, title, layout, focused, color, sleeping) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![
                        tab.id.as_uuid().to_string(),
                        space.id.as_uuid().to_string(),
                        int(tab_position),
                        tab.title,
                        serde_json::to_string(&tab.layout)?,
                        tab.focused.as_uuid().to_string(),
                        tab.color.map(Accent::name),
                        i64::from(tab.sleeping)
                    ],
                )?;
                for (pane_position, pane) in tab.panes.iter().enumerate() {
                    tx.execute(
                        "INSERT INTO panes (id, tab_id, position, cwd, title, agent_kind, agent_session) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                        params![
                            pane.id.as_uuid().to_string(),
                            tab.id.as_uuid().to_string(),
                            int(pane_position),
                            pane.cwd.as_ref().map(|p| p.to_string_lossy().into_owned()),
                            pane.title,
                            pane.agent.as_ref().map(|a| a.kind.as_str()),
                            pane.agent.as_ref().and_then(|a| a.session_id.as_deref()),
                        ],
                    )?;
                }
            }
        }
        for (key, value) in [("active", data.active), ("created", data.created)] {
            tx.execute(
                "INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value.to_string()],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// The stored workspace, or `None` if nothing was saved yet.
    pub fn load(&self) -> Result<Option<WorkspaceData>, StoreError> {
        let mut spaces = Vec::new();
        let mut space_stmt = self
            .conn
            .prepare("SELECT id, name, state, active_tab, color FROM spaces ORDER BY position")?;
        let space_rows = space_stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })?;
        for row in space_rows {
            let (id, name, state, active_tab, color) = row?;
            let active_tab = usize::try_from(active_tab)
                .map_err(|_| StoreError::Corrupt("negative active tab".to_owned()))?;
            spaces.push(SpaceData {
                id: SpaceId::from_uuid(parse_uuid(&id)?),
                name,
                state: parse_state(&state)?,
                color: color.as_deref().and_then(Accent::from_name),
                active_tab,
                tabs: self.load_tabs(&id)?,
            });
        }
        if spaces.is_empty() {
            return Ok(None);
        }
        let meta = |key: &str| -> Result<usize, StoreError> {
            let value: Option<String> = self
                .conn
                .query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| row.get(0))
                .optional()?;
            value
                .and_then(|v| v.parse().ok())
                .ok_or_else(|| StoreError::Corrupt(format!("missing meta `{key}`")))
        };
        Ok(Some(WorkspaceData { spaces, active: meta("active")?, created: meta("created")? }))
    }

    fn load_tabs(&self, space_id: &str) -> Result<Vec<TabData>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, layout, focused, color, sleeping FROM tabs WHERE space_id = ?1 ORDER BY position",
        )?;
        let rows = stmt.query_map([space_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })?;
        let mut tabs = Vec::new();
        for row in rows {
            let (id, title, layout, focused, color, sleeping) = row?;
            tabs.push(TabData {
                id: TabId::from_uuid(parse_uuid(&id)?),
                title,
                color: color.as_deref().and_then(Accent::from_name),
                sleeping: sleeping != 0,
                layout: serde_json::from_str::<Layout>(&layout)?,
                focused: PaneId::from_uuid(parse_uuid(&focused)?),
                panes: self.load_panes(&id)?,
            });
        }
        Ok(tabs)
    }

    fn load_panes(&self, tab_id: &str) -> Result<Vec<Pane>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, cwd, title, agent_kind, agent_session FROM panes WHERE tab_id = ?1 ORDER BY position",
        )?;
        let rows = stmt.query_map([tab_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })?;
        let mut panes = Vec::new();
        for row in rows {
            let (id, cwd, title, kind, session) = row?;
            panes.push(Pane {
                id: PaneId::from_uuid(parse_uuid(&id)?),
                cwd: cwd.map(Into::into),
                title,
                agent: kind.map(|kind| AgentMemo { kind, session_id: session }),
                program: None,
            });
        }
        Ok(panes)
    }
}

/// `SQLite` integers are signed 64-bit.
fn int(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn state_name(state: SpaceState) -> &'static str {
    match state {
        SpaceState::Awake => "awake",
        SpaceState::Sleeping => "sleeping",
    }
}

fn parse_state(name: &str) -> Result<SpaceState, StoreError> {
    match name {
        "awake" => Ok(SpaceState::Awake),
        "sleeping" => Ok(SpaceState::Sleeping),
        other => Err(StoreError::Corrupt(format!("unknown space state `{other}`"))),
    }
}

fn parse_uuid(text: &str) -> Result<uuid::Uuid, StoreError> {
    text.parse().map_err(|_| StoreError::Corrupt(format!("bad id `{text}`")))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use mer_core::{Axis, Workspace};

    use super::*;

    fn rich_workspace() -> Workspace {
        let mut ws = Workspace::new(Some(PathBuf::from("/tmp")));
        ws.split_focused(Axis::Row, Some(PathBuf::from("/var/log")));
        let pane = ws.focused_pane();
        ws.set_pane_title(pane, "tail -f".to_owned());
        ws.set_pane_agent(
            pane,
            Some(AgentMemo { kind: "claude".to_owned(), session_id: Some("s-1".to_owned()) }),
        );
        ws.add_tab(None);
        ws.rename_active_tab("build".to_owned());
        ws.split_focused(Axis::Column, None);
        ws.add_space(Some(PathBuf::from("/opt")));
        ws.rename_space(1, "api".to_owned());
        ws.sleep_space(0);
        ws
    }

    #[test]
    fn the_database_and_its_directory_are_private() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().expect("temp dir");
        let path = root.path().join("state").join("state.db");
        let mut store = Store::open(&path).expect("opens");
        store.save(&rich_workspace().to_data()).expect("saves");

        let mode = |p: &Path| std::fs::metadata(p).expect("exists").permissions().mode() & 0o777;
        assert_eq!(mode(path.parent().expect("parent")), 0o700);
        for suffix in ["", "-wal", "-shm"] {
            let file = PathBuf::from(format!("{}{suffix}", path.display()));
            if file.exists() {
                assert_eq!(mode(&file), 0o600, "{} must be private", file.display());
            }
        }
    }

    #[test]
    fn empty_database_loads_nothing() {
        let store = Store::open_in_memory().expect("opens");
        assert_eq!(store.load().expect("loads"), None);
    }

    #[test]
    fn workspace_survives_a_save_and_load() {
        let data = rich_workspace().to_data();
        let mut store = Store::open_in_memory().expect("opens");
        store.save(&data).expect("saves");
        let loaded = store.load().expect("loads").expect("something saved");
        assert_eq!(loaded, data);
        assert!(Workspace::from_data(loaded).is_ok());
    }

    #[test]
    fn saving_again_replaces_the_previous_state() {
        let mut store = Store::open_in_memory().expect("opens");
        store.save(&rich_workspace().to_data()).expect("saves");

        let small = Workspace::new(None).to_data();
        store.save(&small).expect("saves");
        assert_eq!(store.load().expect("loads"), Some(small));
    }

    #[test]
    fn state_persists_on_disk_and_reopens() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("nested/state.db");
        let data = rich_workspace().to_data();
        Store::open(&path).expect("opens").save(&data).expect("saves");
        assert_eq!(Store::open(&path).expect("reopens").load().expect("loads"), Some(data));
    }

    #[test]
    fn colors_and_sleeping_tabs_survive_a_save_and_load() {
        let mut ws = rich_workspace();
        ws.select_space(1);
        ws.set_space_color(1, Some(Accent::Violet));
        ws.add_tab(None);
        ws.set_tab_color(0, Some(Accent::Amber));
        ws.sleep_tab(1);
        let data = ws.to_data();
        let mut store = Store::open_in_memory().expect("opens");
        store.save(&data).expect("saves");
        let loaded = store.load().expect("loads").expect("something saved");
        assert_eq!(loaded, data);
        assert_eq!(loaded.spaces[1].color, Some(Accent::Violet));
        assert!(loaded.spaces[1].tabs.iter().any(|tab| tab.sleeping));
    }

    #[test]
    fn a_version_1_database_upgrades_without_losing_data() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("old.db");
        let data = rich_workspace().to_data();
        {
            // Build a database exactly as the first release left it.
            let mut conn = Connection::open(&path).expect("opens");
            Migrations::from_slice(&[M::up(SCHEMA_V1)]).to_latest(&mut conn).expect("v1");
            conn.execute("PRAGMA foreign_keys = ON", []).expect("pragma");
            conn.execute(
                "INSERT INTO spaces (id, position, name, state, active_tab) VALUES ('11111111-1111-1111-1111-111111111111', 0, 'old', 'awake', 0)",
                [],
            )
            .expect("space");
        }
        let mut store = Store::open(&path).expect("upgrades");
        let version: i64 =
            store.conn.query_row("PRAGMA user_version", [], |row| row.get(0)).expect("version");
        assert_eq!(version, 2);
        let old: i64 = store
            .conn
            .query_row(
                "SELECT count(*) FROM spaces WHERE name = 'old' AND color = 'red'",
                [],
                |r| r.get(0),
            )
            .expect("row");
        assert_eq!(old, 1, "existing rows are kept and get a color");

        store.save(&data).expect("the new schema accepts a full workspace");
        assert_eq!(store.load().expect("loads"), Some(data));
    }
}
