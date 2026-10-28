//! The system SQLite (`libsqlite3.so.0`, BR2_PACKAGE_SQLITE, the library Python's `sqlite3` module
//! uses on the player), loaded at run time: no SQLite crate and no build dependency. What the report
//! (library counts) and the owner export (online backup) need: open, query rows, backup, and
//! Python's exception class for an SQLite error code (`_pysqlite_seterror`).
// SPDX-License-Identifier: GPL-2.0-only
use std::{
    ffi::{c_char, c_int, c_void, CString},
    sync::OnceLock,
};

type Db = *mut c_void;
type Stmt = *mut c_void;

struct Api {
    open_v2: unsafe extern "C" fn(*const c_char, *mut Db, c_int, *const c_char) -> c_int,
    close_v2: unsafe extern "C" fn(Db) -> c_int,
    busy_timeout: unsafe extern "C" fn(Db, c_int) -> c_int,
    prepare_v2:
        unsafe extern "C" fn(Db, *const c_char, c_int, *mut Stmt, *mut *const c_char) -> c_int,
    step: unsafe extern "C" fn(Stmt) -> c_int,
    finalize: unsafe extern "C" fn(Stmt) -> c_int,
    column_count: unsafe extern "C" fn(Stmt) -> c_int,
    column_type: unsafe extern "C" fn(Stmt, c_int) -> c_int,
    column_int64: unsafe extern "C" fn(Stmt, c_int) -> i64,
    column_double: unsafe extern "C" fn(Stmt, c_int) -> f64,
    column_text: unsafe extern "C" fn(Stmt, c_int) -> *const u8,
    column_blob: unsafe extern "C" fn(Stmt, c_int) -> *const u8,
    column_bytes: unsafe extern "C" fn(Stmt, c_int) -> c_int,
    extended_errcode: unsafe extern "C" fn(Db) -> c_int,
    backup_init: unsafe extern "C" fn(Db, *const c_char, Db, *const c_char) -> *mut c_void,
    backup_step: unsafe extern "C" fn(*mut c_void, c_int) -> c_int,
    backup_remaining: unsafe extern "C" fn(*mut c_void) -> c_int,
    backup_pagecount: unsafe extern "C" fn(*mut c_void) -> c_int,
    backup_finish: unsafe extern "C" fn(*mut c_void) -> c_int,
    sleep: unsafe extern "C" fn(c_int) -> c_int,
}

fn api() -> Option<&'static Api> {
    static API: OnceLock<Option<Api>> = OnceLock::new();
    API.get_or_init(|| {
        // SAFETY: dlopen/dlsym with NUL-terminated names; each symbol is SQLite's documented C API
        // with the signature declared in Api.
        unsafe {
            let handle = libc::dlopen(
                c"libsqlite3.so.0".as_ptr(),
                libc::RTLD_NOW | libc::RTLD_LOCAL,
            );
            if handle.is_null() {
                return None;
            }
            macro_rules! symbol {
                ($name:literal) => {{
                    let pointer = libc::dlsym(handle, concat!($name, "\0").as_ptr().cast());
                    if pointer.is_null() {
                        return None;
                    }
                    std::mem::transmute::<*mut c_void, _>(pointer)
                }};
            }
            Some(Api {
                open_v2: symbol!("sqlite3_open_v2"),
                close_v2: symbol!("sqlite3_close_v2"),
                busy_timeout: symbol!("sqlite3_busy_timeout"),
                prepare_v2: symbol!("sqlite3_prepare_v2"),
                step: symbol!("sqlite3_step"),
                finalize: symbol!("sqlite3_finalize"),
                column_count: symbol!("sqlite3_column_count"),
                column_type: symbol!("sqlite3_column_type"),
                column_int64: symbol!("sqlite3_column_int64"),
                column_double: symbol!("sqlite3_column_double"),
                column_text: symbol!("sqlite3_column_text"),
                column_blob: symbol!("sqlite3_column_blob"),
                column_bytes: symbol!("sqlite3_column_bytes"),
                extended_errcode: symbol!("sqlite3_extended_errcode"),
                backup_init: symbol!("sqlite3_backup_init"),
                backup_step: symbol!("sqlite3_backup_step"),
                backup_remaining: symbol!("sqlite3_backup_remaining"),
                backup_pagecount: symbol!("sqlite3_backup_pagecount"),
                backup_finish: symbol!("sqlite3_backup_finish"),
                sleep: symbol!("sqlite3_sleep"),
            })
        }
    })
    .as_ref()
}

const OK: c_int = 0;
const BUSY: c_int = 5;
const LOCKED: c_int = 6;
const ROW: c_int = 100;
const DONE: c_int = 101;
const OPEN_READWRITE: c_int = 0x2;
const OPEN_CREATE: c_int = 0x4;
const OPEN_URI: c_int = 0x40;

/// An SQLite failure as Python's sqlite3 raises it, or the module missing.
#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    Unavailable,
    Code(c_int),
    /// A progress callback's own error (Python's exception passes through the backup).
    Callback(String),
}

impl Error {
    /// `type(error).__name__` (Modules/_sqlite/util.c `_pysqlite_seterror`).
    pub fn class(&self) -> &'static str {
        match self {
            Error::Unavailable => "ModuleNotFoundError",
            Error::Callback(_) => "ValueError",
            Error::Code(code) => match code & 0xff {
                2 | 12 => "InternalError",
                7 => "MemoryError",
                1 | 3 | 4 | 5 | 6 | 8 | 9 | 10 | 13 | 14 | 15 | 16 | 17 => "OperationalError",
                11 => "DatabaseError",
                18 => "DataError",
                19 | 20 => "IntegrityError",
                21 | 25 => "InterfaceError",
                _ => "DatabaseError",
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

pub struct Database {
    db: Db,
    api: &'static Api,
}

impl Database {
    /// `sqlite3.connect(name, uri=True, timeout=busy_ms / 1000)` (Python opens read-write-create; a
    /// `mode=ro` URI narrows it).
    pub fn open(name: &str, busy_ms: i32) -> Result<Database, Error> {
        let api = api().ok_or(Error::Unavailable)?;
        let c = CString::new(name).map_err(|_| Error::Code(21))?;
        let mut db: Db = std::ptr::null_mut();
        // SAFETY: a valid NUL-terminated name and an output pointer.
        let rc = unsafe {
            (api.open_v2)(
                c.as_ptr(),
                &mut db,
                OPEN_READWRITE | OPEN_CREATE | OPEN_URI,
                std::ptr::null(),
            )
        };
        let database = Database { db, api };
        if rc != OK {
            let code = if db.is_null() {
                rc
            } else {
                // SAFETY: an open (failed) handle.
                unsafe { (api.extended_errcode)(db) }
            };
            return Err(Error::Code(code));
        }
        // SAFETY: an open handle.
        unsafe { (api.busy_timeout)(db, busy_ms) };
        Ok(database)
    }

    fn error(&self) -> Error {
        // SAFETY: an open handle.
        Error::Code(unsafe { (self.api.extended_errcode)(self.db) })
    }

    /// Every row of one statement.
    pub fn query(&self, sql: &str) -> Result<Vec<Vec<Value>>, Error> {
        let c = CString::new(sql).map_err(|_| Error::Code(21))?;
        let mut stmt: Stmt = std::ptr::null_mut();
        // SAFETY: an open handle, a NUL-terminated statement, output pointers.
        let rc = unsafe {
            (self.api.prepare_v2)(self.db, c.as_ptr(), -1, &mut stmt, std::ptr::null_mut())
        };
        if rc != OK {
            return Err(self.error());
        }
        let mut rows = Vec::new();
        let result = loop {
            // SAFETY: a prepared statement.
            match unsafe { (self.api.step)(stmt) } {
                ROW => {
                    // SAFETY: a statement positioned on a row; column pointers stay valid until the
                    // next step and are copied at once.
                    let row = unsafe {
                        (0..(self.api.column_count)(stmt))
                            .map(|i| match (self.api.column_type)(stmt, i) {
                                1 => Value::Integer((self.api.column_int64)(stmt, i)),
                                2 => Value::Real((self.api.column_double)(stmt, i)),
                                3 => {
                                    let p = (self.api.column_text)(stmt, i);
                                    let n = (self.api.column_bytes)(stmt, i) as usize;
                                    Value::Text(if p.is_null() {
                                        String::new()
                                    } else {
                                        String::from_utf8_lossy(std::slice::from_raw_parts(p, n))
                                            .into_owned()
                                    })
                                }
                                4 => {
                                    let p = (self.api.column_blob)(stmt, i);
                                    let n = (self.api.column_bytes)(stmt, i) as usize;
                                    Value::Blob(if p.is_null() {
                                        Vec::new()
                                    } else {
                                        std::slice::from_raw_parts(p, n).to_vec()
                                    })
                                }
                                _ => Value::Null,
                            })
                            .collect()
                    };
                    rows.push(row);
                }
                DONE => break Ok(rows),
                _ => break Err(self.error()),
            }
        };
        // SAFETY: the statement is finalized once.
        unsafe { (self.api.finalize)(stmt) };
        result
    }

    /// `self.backup(target, pages=pages, progress=progress, sleep=sleep_ms / 1000)`: progress gets
    /// (status, remaining, pagecount) after every step and may stop the copy with its error.
    pub fn backup(
        &self,
        target: &Database,
        pages: i32,
        sleep_ms: i32,
        mut progress: impl FnMut(i32, i32, i32) -> Result<(), String>,
    ) -> Result<(), Error> {
        let main = c"main";
        // SAFETY: two open handles and NUL-terminated schema names.
        let handle =
            unsafe { (self.api.backup_init)(target.db, main.as_ptr(), self.db, main.as_ptr()) };
        if handle.is_null() {
            return Err(target.error());
        }
        loop {
            // SAFETY: a live backup handle.
            let rc = unsafe { (self.api.backup_step)(handle, pages) };
            // SAFETY: a live backup handle.
            let (remaining, total) = unsafe {
                (
                    (self.api.backup_remaining)(handle),
                    (self.api.backup_pagecount)(handle),
                )
            };
            if let Err(error) = progress(rc, remaining, total) {
                // SAFETY: the handle is finished once.
                unsafe { (self.api.backup_finish)(handle) };
                return Err(Error::Callback(error));
            }
            if rc == BUSY || rc == LOCKED {
                // SAFETY: plain sleep.
                unsafe { (self.api.sleep)(sleep_ms) };
            }
            if !(rc == OK || rc == BUSY || rc == LOCKED) {
                break;
            }
        }
        // SAFETY: the handle is finished once.
        if unsafe { (self.api.backup_finish)(handle) } != OK {
            return Err(target.error());
        }
        Ok(())
    }
}

impl Drop for Database {
    fn drop(&mut self) {
        if !self.db.is_null() {
            // SAFETY: the handle is closed once; statements are finalized by query().
            unsafe { (self.api.close_v2)(self.db) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queries_and_backups_through_the_system_library() {
        if api().is_none() {
            return; // no libsqlite3 on this host
        }
        let dir = std::env::temp_dir().join(format!("y2p-sqlite-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.db");
        let db = Database::open(&path.to_string_lossy(), 500).unwrap();
        db.query("create table tracks (codec text, sample_rate int)")
            .unwrap();
        db.query("insert into tracks values ('flac', 44100), ('flac', 48000), ('mp3', 44100)")
            .unwrap();
        assert_eq!(
            db.query("select codec, count(*) from tracks group by 1 order by 2 desc")
                .unwrap(),
            vec![
                vec![Value::Text("flac".into()), Value::Integer(2)],
                vec![Value::Text("mp3".into()), Value::Integer(1)]
            ]
        );
        assert_eq!(
            db.query("select * from missing").unwrap_err().class(),
            "OperationalError"
        );
        let copy = Database::open(&dir.join("b.db").to_string_lossy(), 500).unwrap();
        let mut calls = 0;
        db.backup(&copy, 128, 50, |_, _, _| {
            calls += 1;
            Ok(())
        })
        .unwrap();
        assert!(calls >= 1);
        assert_eq!(
            copy.query("pragma quick_check").unwrap(),
            vec![vec![Value::Text("ok".into())]]
        );
        std::fs::write(dir.join("bad.db"), b"not a database at all, really not").unwrap();
        let bad = Database::open(
            &format!("file:{}?mode=ro&immutable=1", dir.join("bad.db").display()),
            500,
        )
        .unwrap();
        assert_eq!(
            bad.query("select 1 from sqlite_master")
                .unwrap_err()
                .class(),
            "DatabaseError"
        );
        drop((db, copy, bad));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
