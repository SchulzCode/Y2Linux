"""umtprd patch 0005: music object formats and Reborn library tags.

Builds the pinned responder with every Y2 patch, then drives the real dataset
builders against a temporary library shaped like Reborn's (WAL, written by
another connection while umtprd reads it)."""
import hashlib
import shutil
import sqlite3
import subprocess
import tarfile
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
ARCHIVE = ROOT/'.cache/buildroot-dl/y2-mtp/y2-mtp-2241a803c27f1fff7537cb9504befeb810b880d3.tar.gz'
SCHEMA = '''
CREATE TABLE sources(id TEXT PRIMARY KEY, root TEXT NOT NULL, online INTEGER NOT NULL);
CREATE TABLE tracks(id INTEGER PRIMARY KEY, source_id TEXT NOT NULL REFERENCES sources(id), path TEXT NOT NULL,
  filename TEXT NOT NULL, size INTEGER NOT NULL, mtime INTEGER NOT NULL, title TEXT NOT NULL, artist TEXT NOT NULL,
  album TEXT NOT NULL, album_artist TEXT NOT NULL, track INTEGER NOT NULL, disc INTEGER NOT NULL,
  duration_ms INTEGER NOT NULL, codec TEXT NOT NULL, sample_rate INTEGER NOT NULL, channels INTEGER NOT NULL,
  bitrate INTEGER NOT NULL, artwork INTEGER NOT NULL, seen INTEGER NOT NULL, deleted INTEGER NOT NULL DEFAULT 0,
  genre TEXT NOT NULL DEFAULT '', UNIQUE(source_id,path));
'''
FILES = ('song.mp3', 'OLD.FLAC', 'both.m4a', 'gone.ogg', 'untitled.opus', 'voice.oga', 'take.wav', 'old.wma',
         'raw.aac', 'notes.txt', 'noext', 'cover.mp3.jpg')


def library(path, media):
    db = sqlite3.connect(path, isolation_level=None)
    db.execute('PRAGMA journal_mode=WAL')
    db.executescript(SCHEMA)
    db.executemany('INSERT INTO sources VALUES (?,?,?)', [('sd-old', str(media), 0), ('sd-new', str(media), 1)])

    def track(source, name, title, artist='', deleted=0, **tags):
        values = dict(album='', album_artist='', track=0, disc=0, duration_ms=0, sample_rate=0, channels=0,
                      bitrate=0, genre='')
        values.update(tags)
        db.execute('INSERT INTO tracks(source_id,path,filename,size,mtime,title,artist,album,album_artist,track,disc,'
                   'duration_ms,codec,sample_rate,channels,bitrate,artwork,seen,deleted,genre) '
                   'VALUES (?,?,?,1,1,?,?,?,?,?,?,?,"",?,?,?,0,1,?,?)',
                   (source, '%s/Music/%s' % (media, name), name, title, artist, values['album'],
                    values['album_artist'], values['track'], values['disc'], values['duration_ms'],
                    values['sample_rate'], values['channels'], values['bitrate'], deleted, values['genre']))
    track('sd-new', 'song.mp3', 'Real Title é', 'Artist A', album='Album A', album_artist='Album Artist',
          track=7, disc=1, duration_ms=245000, sample_rate=44100, channels=2, bitrate=320000, genre='Jazz')
    track('sd-old', 'song.mp3', 'Stale Song', 'Old Artist')
    track('sd-old', 'OLD.FLAC', 'Stale', 'Old Artist')
    track('sd-old', 'both.m4a', 'Stale Both')
    track('sd-new', 'both.m4a', 'Fresh Both')
    track('sd-new', 'gone.ogg', 'Deleted', deleted=1)
    track('sd-new', 'untitled.opus', '', 'Someone')
    return db


@unittest.skipUnless(shutil.which('cc') and shutil.which('make') and Path('/usr/include/sqlite3.h').exists(),
                     'cc, make and SQLite headers required')
class MusicTags(unittest.TestCase):
    def test_formats_and_library_tags(self):
        if not ARCHIVE.is_file():
            self.skipTest('pinned responder archive not prepared')
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary)
            with tarfile.open(ARCHIVE) as source:
                source.extractall(base, filter='data')
            source = next(base.glob('uMTP-Responder-*'))
            for patchfile in sorted((ROOT/'buildroot/package/y2-mtp').glob('*.patch')):
                subprocess.run(['patch', '--batch', '--fuzz=0', '-p1', '-i', str(patchfile)], cwd=source, check=True,
                               stdout=subprocess.DEVNULL)
            for name in ('h', 'c'):
                shutil.copyfile(ROOT/('tools/usb/mtp-safe.'+name),
                                source/('inc' if name == 'h' else 'src')/('mtp-safe.'+name))
            build = subprocess.run(['make', '-j4'], cwd=source, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                   text=True)
            self.assertEqual(build.returncode, 0, build.stdout)
            self.assertNotIn('warning:', build.stdout)
            flags = ['cc', '-I'+str(source/'inc')]
            main = base/'main.o'
            subprocess.run(flags+['-Dmain=umtprd_original_main', '-c', str(source/'src/umtprd.c'), '-o', str(main)],
                           check=True)
            binary = base/'music-tags-test'
            objects = [str(o) for o in (source/'obj').glob('*.o') if o.name != 'umtprd.o']
            subprocess.run(flags+['-Wall', '-Wextra', '-Werror', str(ROOT/'tests/usb_mtp_music_tags.c'), str(main),
                                  *objects, '-lsqlite3', '-lpthread', '-lrt', '-o', str(binary)], check=True)

            media = base/'media'
            (media/'Music').mkdir(parents=True)
            for name in FILES:
                (media/'Music'/name).write_bytes(b'x')
            (base/'data').mkdir()
            db_path = base/'data/library.db'
            writer = library(db_path, media)
            locked_path = base/'data/locked.db'
            locked = sqlite3.connect(locked_path, isolation_level=None)
            locked.executescript(SCHEMA)
            locked.execute('BEGIN EXCLUSIVE')
            before = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in (base/'data').iterdir()}
            try:
                run = subprocess.run([str(binary), str(media), str(db_path), str(locked_path)],
                                     stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, timeout=20)
                self.assertEqual(run.returncode, 0, run.stdout)
                self.assertIn('music tags ok', run.stdout)
                after = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in (base/'data').iterdir()
                         if not p.name.endswith('-shm')}
                # Read-only: the library, its WAL and the locked library are untouched, nothing new appears.
                self.assertEqual(after, {k: v for k, v in before.items() if not k.endswith('-shm')})
                self.assertEqual(writer.execute('SELECT count(*) FROM tracks').fetchone()[0], 7)
            finally:
                locked.execute('ROLLBACK')
                locked.close()
                writer.close()
            self.assertFalse(list(media.glob('**/.y2-mtp-*')))


if __name__ == '__main__':
    unittest.main()
