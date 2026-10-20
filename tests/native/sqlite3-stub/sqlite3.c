#include <stddef.h>
#include "sqlite3.h"

int sqlite3_open_v2(const char *filename, sqlite3 **db, int flags, const char *vfs)
{
	(void)filename; (void)flags; (void)vfs;
	*db = NULL;
	return SQLITE_CANTOPEN;
}
int sqlite3_close(sqlite3 *db) { (void)db; return SQLITE_OK; }
const char *sqlite3_errmsg(sqlite3 *db) { (void)db; return "SQLite stub"; }
int sqlite3_busy_timeout(sqlite3 *db, int ms) { (void)db; (void)ms; return SQLITE_OK; }
int sqlite3_prepare_v2(sqlite3 *db, const char *sql, int bytes, sqlite3_stmt **stmt, const char **tail)
{
	(void)db; (void)sql; (void)bytes; (void)tail;
	*stmt = NULL;
	return SQLITE_MISUSE;
}
int sqlite3_finalize(sqlite3_stmt *stmt) { (void)stmt; return SQLITE_OK; }
int sqlite3_bind_text(sqlite3_stmt *stmt, int index, const char *text, int bytes, void (*destructor)(void *))
{
	(void)stmt; (void)index; (void)text; (void)bytes; (void)destructor;
	return SQLITE_MISUSE;
}
int sqlite3_step(sqlite3_stmt *stmt) { (void)stmt; return SQLITE_MISUSE; }
int sqlite3_reset(sqlite3_stmt *stmt) { (void)stmt; return SQLITE_OK; }
int sqlite3_clear_bindings(sqlite3_stmt *stmt) { (void)stmt; return SQLITE_OK; }
int sqlite3_column_count(sqlite3_stmt *stmt) { (void)stmt; return 0; }
const unsigned char *sqlite3_column_text(sqlite3_stmt *stmt, int column) { (void)stmt; (void)column; return NULL; }
sqlite3_int64 sqlite3_column_int64(sqlite3_stmt *stmt, int column) { (void)stmt; (void)column; return 0; }
