/* The few SQLite calls y2_tags.c makes, for building the responder where no
 * SQLite headers exist (the locked container). Every open fails, so lookups
 * fall back to the file name, as on a player without a library. */
#ifndef Y2_SQLITE3_STUB_H
#define Y2_SQLITE3_STUB_H

typedef struct sqlite3 sqlite3;
typedef struct sqlite3_stmt sqlite3_stmt;
typedef long long sqlite3_int64;

#define SQLITE_OK 0
#define SQLITE_BUSY 5
#define SQLITE_LOCKED 6
#define SQLITE_CANTOPEN 14
#define SQLITE_MISUSE 21
#define SQLITE_ROW 100
#define SQLITE_DONE 101
#define SQLITE_OPEN_READONLY 0x00000001
#define SQLITE_OPEN_URI 0x00000040
#define SQLITE_OPEN_NOMUTEX 0x00008000
#define SQLITE_STATIC ((void (*)(void *))0)

int sqlite3_open_v2(const char *filename, sqlite3 **db, int flags, const char *vfs);
int sqlite3_close(sqlite3 *db);
const char *sqlite3_errmsg(sqlite3 *db);
int sqlite3_busy_timeout(sqlite3 *db, int ms);
int sqlite3_prepare_v2(sqlite3 *db, const char *sql, int bytes, sqlite3_stmt **stmt, const char **tail);
int sqlite3_finalize(sqlite3_stmt *stmt);
int sqlite3_bind_text(sqlite3_stmt *stmt, int index, const char *text, int bytes, void (*destructor)(void *));
int sqlite3_step(sqlite3_stmt *stmt);
int sqlite3_reset(sqlite3_stmt *stmt);
int sqlite3_clear_bindings(sqlite3_stmt *stmt);
int sqlite3_column_count(sqlite3_stmt *stmt);
const unsigned char *sqlite3_column_text(sqlite3_stmt *stmt, int column);
sqlite3_int64 sqlite3_column_int64(sqlite3_stmt *stmt, int column);

#endif
