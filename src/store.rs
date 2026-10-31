use crate::{model::*, util};
use anyhow::{Result, bail, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use std::{path::Path, sync::Mutex};

pub struct Store {
    db: Mutex<Connection>,
}
impl Store {
    pub fn open(root: &Path) -> Result<Self> {
        util::private_dir(root)?;
        let db = Connection::open(root.join("state.sqlite3"))?;
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;
          CREATE TABLE IF NOT EXISTS apps(name TEXT PRIMARY KEY, body TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS releases(id TEXT PRIMARY KEY, app TEXT NOT NULL, body TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS jobs(id TEXT PRIMARY KEY, app TEXT NOT NULL, state TEXT NOT NULL, body TEXT NOT NULL);
          CREATE UNIQUE INDEX IF NOT EXISTS one_active_job ON jobs(app) WHERE state IN ('queued','running');")?;
        Ok(Self { db: Mutex::new(db) })
    }
    pub fn apps(&self) -> Result<Vec<Application>> {
        self.all("SELECT body FROM apps ORDER BY name")
    }
    pub fn jobs(&self) -> Result<Vec<Job>> {
        self.all("SELECT body FROM jobs ORDER BY rowid DESC LIMIT 200")
    }
    fn all<T: serde::de::DeserializeOwned>(&self, sql: &str) -> Result<Vec<T>> {
        let db = self.db.lock().unwrap();
        let mut q = db.prepare(sql)?;
        let texts = q
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        texts
            .into_iter()
            .map(|s| Ok(serde_json::from_str(&s)?))
            .collect()
    }
    pub fn app(&self, name: &str) -> Result<Application> {
        self.one("SELECT body FROM apps WHERE name=?", name)
    }
    pub fn job(&self, id: &str) -> Result<Job> {
        self.one("SELECT body FROM jobs WHERE id=?", id)
    }
    pub fn release(&self, id: &str) -> Result<Release> {
        self.one("SELECT body FROM releases WHERE id=?", id)
    }
    fn one<T: serde::de::DeserializeOwned>(&self, sql: &str, id: &str) -> Result<T> {
        let s: Option<String> = self
            .db
            .lock()
            .unwrap()
            .query_row(sql, [id], |r| r.get(0))
            .optional()?;
        match s {
            Some(s) => Ok(serde_json::from_str(&s)?),
            None => bail!("not found: {id}"),
        }
    }
    pub fn insert_app(&self, a: &Application) -> Result<()> {
        self.db.lock().unwrap().execute(
            "INSERT INTO apps VALUES (?,?)",
            params![a.spec.name, serde_json::to_string(a)?],
        )?;
        Ok(())
    }
    pub fn save_app(&self, a: &Application) -> Result<()> {
        let n = self.db.lock().unwrap().execute(
            "UPDATE apps SET body=? WHERE name=?",
            params![serde_json::to_string(a)?, a.spec.name],
        )?;
        ensure!(n == 1, "application disappeared");
        Ok(())
    }
    pub fn delete_app(&self, name: &str) -> Result<()> {
        self.db
            .lock()
            .unwrap()
            .execute("DELETE FROM apps WHERE name=?", [name])?;
        Ok(())
    }
    pub fn releases(&self, app: &str) -> Result<Vec<Release>> {
        let db = self.db.lock().unwrap();
        let mut q = db.prepare("SELECT body FROM releases WHERE app=? ORDER BY rowid DESC")?;
        let s = q
            .query_map([app], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        s.into_iter()
            .map(|v| Ok(serde_json::from_str(&v)?))
            .collect()
    }
    pub fn add_release(&self, r: &Release) -> Result<()> {
        self.db.lock().unwrap().execute(
            "INSERT INTO releases VALUES (?,?,?)",
            params![r.id, r.app, serde_json::to_string(r)?],
        )?;
        Ok(())
    }
    pub fn insert_job(&self, j: &Job) -> Result<()> {
        self.db.lock().unwrap().execute(
            "INSERT INTO jobs VALUES (?,?,?,?)",
            params![j.id, j.app, j.state, serde_json::to_string(j)?],
        )?;
        Ok(())
    }
    pub fn save_job(&self, j: &Job) -> Result<()> {
        self.db.lock().unwrap().execute(
            "UPDATE jobs SET state=?,body=? WHERE id=?",
            params![j.state, serde_json::to_string(j)?, j.id],
        )?;
        Ok(())
    }
    pub fn recover_jobs(&self) -> Result<()> {
        let jobs: Vec<Job> =
            self.all("SELECT body FROM jobs WHERE state IN ('queued','running')")?;
        for mut j in jobs {
            j.state = "interrupted".into();
            j.completed_at = Some(util::now());
            j.error=Some("Daemon exited during operation. Inspect machine before retrying; data was not deleted.".into());
            self.save_job(&j)?;
        }
        Ok(())
    }
}
