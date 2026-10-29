import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class MeasurementTests(unittest.TestCase):
    def test_real_sqlite_enospc_wal_and_checkpoint_preserve_committed_data(self):
        # Inject ENOSPC at the actual SQLite pwrite boundary, limited to one new
        # private scratch database. This does not emulate electrical power loss.
        with tempfile.TemporaryDirectory() as root:
            library = Path(root) / 'fault.so'
            subprocess.run(['cc', '-shared', '-fPIC', '-O2', '-Wall', '-Wextra',
                            str(ROOT / 'tests/fixtures/sqlite-enospc.c'), '-ldl', '-o', str(library)], check=True)
            child = r'''
import ctypes, os, pathlib, sqlite3, sys
root = pathlib.Path(sys.argv[1])
inject = ctypes.CDLL(sys.argv[2]).y2_test_enospc
fd = os.open(root, os.O_RDONLY | os.O_DIRECTORY)
path = root / 'fault.sqlite'
for wal in (1, 0):
    if path.exists(): path.unlink()
    c = sqlite3.connect(path, isolation_level=None)
    c.executescript('PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA wal_autocheckpoint=0; CREATE TABLE tracks(id INTEGER PRIMARY KEY, title TEXT); INSERT INTO tracks VALUES(1,"committed");')
    if wal: c.execute('PRAGMA wal_checkpoint(TRUNCATE)').fetchall()
    assert inject(fd, wal) == 0
    try:
        if wal:
            c.execute('INSERT INTO tracks VALUES(2,"uncommitted")')
        else:
            c.execute('PRAGMA wal_checkpoint(TRUNCATE)').fetchall()
        raise AssertionError('injection did not reach SQLite')
    except sqlite3.DatabaseError as e:
        assert e.sqlite_errorcode & 255 in (sqlite3.SQLITE_FULL, sqlite3.SQLITE_IOERR), e
    assert inject(-1, 0) > 0
    c.close()
    c = sqlite3.connect(path)
    assert c.execute('PRAGMA quick_check').fetchone()[0] == 'ok'
    assert c.execute('SELECT * FROM tracks').fetchall() == [(1, 'committed')]
    c.close()
os.close(fd)
'''
            subprocess.run([sys.executable, '-c', child, root, str(library)],
                           env={**os.environ, 'LD_PRELOAD': str(library)}, check=True, timeout=20)

    def test_abrupt_process_exit_recovers_wal_without_claiming_power_loss(self):
        with tempfile.TemporaryDirectory() as root:
            path = str(Path(root) / 'fault.sqlite')
            child = r'''
import os, sqlite3, sys
c = sqlite3.connect(sys.argv[1], isolation_level=None)
c.executescript('PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; CREATE TABLE t(value); INSERT INTO t VALUES(1); BEGIN; INSERT INTO t VALUES(2);')
os._exit(23)
'''
            result = subprocess.run([sys.executable, '-c', child, path], timeout=10)
            self.assertEqual(result.returncode, 23)
            import sqlite3
            with sqlite3.connect(path) as c:
                self.assertEqual(c.execute('PRAGMA quick_check').fetchone()[0], 'ok')
                self.assertEqual(c.execute('SELECT * FROM t').fetchall(), [(1,)])
            c.close()


if __name__ == '__main__':
    unittest.main()
