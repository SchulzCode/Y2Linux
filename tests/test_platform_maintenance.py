import contextlib
import json
import os
from pathlib import Path
import sqlite3
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import patch
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'tools/platform'))
from y2_platform.common import Context
from y2_platform import maintenance as m
from tools.production.service_guards import bluetooth_maintenance_guard

class Maintenance(unittest.TestCase):
    def test_bluez_guard_distinguishes_start_from_restart_and_is_idempotent(self):
        source = '#!/bin/sh\nstart() {\n\tstart-stop-daemon --start\n}\nrestart() {\n\tstop\n\tstart\n}\n'
        result = bluetooth_maintenance_guard(source)
        self.assertIn('start() {\n\t[ ! -e /data/system/platform/maintenance-pending ] || return 1\n', result)
        self.assertIn('restart() {\n\tstop\n\tstart\n}', result)
        self.assertEqual(bluetooth_maintenance_guard(result), result)
        for unknown in ('restart() {\n}\n', source + 'start() {\n}\n'):
            with self.assertRaisesRegex(ValueError, 'structure'):
                bluetooth_maintenance_guard(unknown)

    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup)
        self.root=Path(self.tmp.name);self.ctx=Context(self.root)
        self.data=self.ctx.path('/data');self.data.mkdir()
        (self.data/'.y2data-schema').write_text('1');(self.data/'updates').mkdir(mode=0o700)
        self.volume={'path':'/data','state':'Ready','generation':'a:7:179:7','uuid':'data-uuid'}
        check=patch('y2_platform.transfer.storage',return_value={'volumes':[self.volume]})
        check.start();self.addCleanup(check.stop)
        for d in ('proc/sys/kernel/random','data/reborn/state','data/music','data/network','data/ssh/host-keys','data/factory'):
            self.ctx.path(d).mkdir(parents=True,exist_ok=True)
        self.ctx.path('/proc/sys/kernel/random/boot_id').write_text('boot')
        self.ctx.path('/data/music/song.flac').write_bytes(b'music')
        self.ctx.path('/data/ssh/host-keys/private').write_bytes(b'test-only-secret')
        self.ctx.path('/data/factory/preserve').write_bytes(b'factory-fixture')
        self.session={'schema':2,'model':{'settings':{'volume':99},'queue':[{'id':3}],'position_ms':42}}
        self.ctx.path('/data/'+m.SETTINGS).write_text(json.dumps(self.session))
        self.ctx.runner=lambda *a,**kw:{'ok':True,'output':json.dumps({'session_schema':2,'settings':{'volume':35}})}

    def test_settings_reset_preserves_queue_database_music_and_bounded_purge(self):
        plan=m.plan(self.ctx,'settings')
        with self.assertRaisesRegex(ValueError,'confirmation'):m.execute(self.ctx,'wrong')
        result=m.execute(self.ctx,plan['confirmation'])
        session=json.loads(self.ctx.path('/data/'+m.SETTINGS).read_text())
        self.assertEqual(session['model']['settings'],{'volume':35})
        self.assertEqual(session['model']['queue'],self.session['model']['queue'])
        self.assertEqual(session['model']['position_ms'],42)
        self.assertEqual(self.ctx.path('/data/music/song.flac').read_bytes(),b'music')
        self.assertEqual(m.purge(self.ctx,plan['id'],plan['confirmation'])['state'],'Complete')
        self.assertFalse(self.ctx.path('/data/system/platform/maintenance-pending').exists())

    def test_process_source_and_mount_guards_and_full_reset_explicitness(self):
        process=self.ctx.path('/proc/999');process.mkdir()
        (process/'cmdline').write_bytes(b'/usr/bin/reborn\0')
        self.assertEqual(m.plan(self.ctx,'settings')['state'],'NeedsQuiescence')
        (process/'cmdline').unlink();process.rmdir()
        plan=m.plan(self.ctx,'full-user')
        with self.assertRaisesRegex(ValueError,'erase_user_music'):m.execute(self.ctx,plan['confirmation'])
        self.volume['generation']='replacement'
        with self.assertRaisesRegex(ValueError,'generation'):m.execute(self.ctx,plan['confirmation'],True)
        self.volume['generation']='a:7:179:7'
        result=m.execute(self.ctx,plan['confirmation'],True)
        self.assertFalse(self.ctx.path('/data/music').exists())
        self.assertEqual(self.ctx.path('/data/factory/preserve').read_bytes(),b'factory-fixture')
        self.assertTrue(self.ctx.path('/data/ssh/host-keys/private').exists())
        self.assertEqual(result['state'],'Complete')

    def test_interrupted_reset_resumes_after_new_boot_without_overwriting_new_files(self):
        for suffix in ('','-wal','-shm'): self.ctx.path('/data/reborn/library.db'+suffix).write_bytes(b'test')
        plan=m.plan(self.ctx,'library');rename=os.rename;calls=[0]
        def interrupt(*args,**kwargs):
            calls[0]+=1
            if calls[0]==2:raise OSError('injected interruption')
            return rename(*args,**kwargs)
        with patch.object(m.os,'rename',side_effect=interrupt):
            with self.assertRaises(OSError):m.execute(self.ctx,plan['confirmation'])
        self.assertTrue(self.ctx.path('/data/system/platform/maintenance-pending').exists())
        self.ctx.path('/proc/sys/kernel/random/boot_id').write_text('next')
        self.volume['generation']='next:8:179:7'
        result=m.execute(self.ctx,plan['confirmation'],resume=True)
        self.assertEqual(result['state'],'Complete')
        self.assertFalse(self.ctx.path('/data/system/platform/maintenance-pending').exists())
        self.assertEqual(len(result['completed']),3)

    def test_private_export_uses_online_sqlite_backup_and_excludes_bonds_keys(self):
        database=self.ctx.path('/data/reborn/library.db')
        with contextlib.closing(sqlite3.connect(database)) as db:
            db.execute('PRAGMA journal_mode=WAL');db.execute('CREATE TABLE test(value)');db.execute('INSERT INTO test VALUES(42)');db.commit()
            self.ctx.path('/data/network/wpa_supplicant.conf').write_text('secret test fixture')
            result=m.export(self.ctx,include_database=True)
            self.assertFalse(result['contains_network_credentials'])
            archive=self.ctx.path(result['path'])
            self.assertEqual(archive.stat().st_mode & 0o777,0o600)
            with tarfile.open(archive) as tar:
                names=tar.getnames()
                self.assertEqual(set(names),{'manifest.json','reborn--state--session.json','library.db'})
                restored=self.root/'restored.db'
                restored.write_bytes(tar.extractfile('library.db').read())
            with contextlib.closing(sqlite3.connect(restored)) as check:
                self.assertEqual(check.execute('SELECT value FROM test').fetchone(),(42,))
            result=m.export(self.ctx,include_network=True)
            self.assertTrue(result['contains_network_credentials'])
            with tarfile.open(self.ctx.path(result['path'])) as tar:
                self.assertIn('network--wpa_supplicant.conf',tar.getnames())

    def test_symlink_never_escapes_purge_and_source_reset_refuses_symlink(self):
        outside=self.root/'outside';outside.mkdir();(outside/'keep').write_bytes(b'keep')
        self.ctx.path('/data/music/link').symlink_to(outside,target_is_directory=True)
        plan=m.plan(self.ctx,'full-user');m.execute(self.ctx,plan['confirmation'],True)
        while m.purge(self.ctx,plan['id'],plan['confirmation'],limit=2)['state']!='Complete':pass
        self.assertTrue((outside/'keep').exists())
        self.ctx.path('/data/reborn').symlink_to(outside,target_is_directory=True)
        with self.assertRaises(ValueError):m.plan(self.ctx,'full-user')

if __name__=='__main__':unittest.main()
