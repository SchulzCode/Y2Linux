import hashlib
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]/'tools/platform'))
from y2_platform.common import Context
from y2_platform.ssh import entropy_ready, prerequisites
from y2_platform.transfer import begin, commit, discard


class Transfer(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(); self.addCleanup(self.tmp.cleanup)
        self.ctx = Context(self.tmp.name)
        self.volume = {'path':'/data', 'state':'Ready', 'space_state':'Normal',
                       'generation':'boot:1:179:7', 'available_bytes':1024**3}
        p = patch('y2_platform.transfer.storage', side_effect=lambda _: {'volumes':[self.volume]})
        p.start(); self.addCleanup(p.stop)
        self.payload = b'owner music test\0' * 4000

    def begin(self, name='Album/track.flac'):
        stage = begin(self.ctx, name, len(self.payload), hashlib.sha256(self.payload).hexdigest())
        return stage, self.ctx.path(stage['upload_path'])

    def test_interrupted_upload_bad_hash_and_reserve_never_publish(self):
        stage,path=self.begin(); path.write_bytes(self.payload[:100])
        with self.assertRaisesRegex(ValueError,'incomplete'): commit(self.ctx,stage['id'])
        self.assertFalse(self.ctx.path('/data/music').exists())
        path.write_bytes(b'x'*len(self.payload))
        with self.assertRaisesRegex(ValueError,'hash'): commit(self.ctx,stage['id'])
        self.assertFalse(self.ctx.path('/data/music').exists())
        discard(self.ctx,stage['id'])
        self.volume['available_bytes']=97*1024**2
        with self.assertRaisesRegex(ValueError,'insufficient_space'):
            begin(self.ctx,'large.flac',1024**2,'a'*64)

    def test_verified_copy_does_not_change_with_old_upload_fd_and_never_overwrites(self):
        stage,path=self.begin(); path.write_bytes(self.payload)
        with path.open('r+b') as old:
            value=commit(self.ctx,stage['id'])
            self.assertEqual(value['state'],'Committed')
            old.seek(0); old.write(b'changed'); old.flush()
        target=self.ctx.path('/data/music/Album/track.flac')
        self.assertEqual(target.read_bytes(),self.payload)
        self.assertEqual(commit(self.ctx,stage['id'])['state'],'Committed')
        discard(self.ctx,stage['id'])
        stage,path=self.begin(); path.write_bytes(self.payload)
        with self.assertRaises(FileExistsError): commit(self.ctx,stage['id'])
        self.assertEqual(target.read_bytes(),self.payload)

    def test_mount_replacement_and_symlink_escape_fail_closed(self):
        stage,path=self.begin(); path.write_bytes(self.payload)
        self.volume['generation']='boot:2:179:7'
        with self.assertRaisesRegex(ValueError,'generation_changed'): commit(self.ctx,stage['id'])
        discard(self.ctx,stage['id'])
        outside=Path(self.tmp.name)/'outside';outside.mkdir()
        self.ctx.path('/data/music').symlink_to(outside)
        stage,path=self.begin();path.write_bytes(self.payload)
        with self.assertRaises(OSError):commit(self.ctx,stage['id'])
        self.assertEqual(list(outside.iterdir()),[])
        for name in ('../state/db','/root/key','ok/../bad','ok\nput secret'):
            with self.assertRaises(ValueError):self.begin(name)

    def test_ssh_waits_for_crng_and_usb_not_only_a_running_daemon(self):
        def not_ready(*_):raise BlockingIOError()
        self.assertFalse(entropy_ready(not_ready))
        self.assertTrue(entropy_ready(lambda *_:b'x'))
        key=self.ctx.path('/root/.ssh/authorized_keys');key.parent.mkdir(parents=True);key.write_text('fixture-public-key')
        self.assertEqual(prerequisites(self.ctx,False)[1],'kernel_crng_not_ready')
        self.ctx.runner=lambda *a,**k:{'ok':True,'output':json.dumps([{'ifname':'wlan0','addr_info':[{'local':'10.42.0.1','prefixlen':24}]}])}
        self.assertEqual(prerequisites(self.ctx,True)[1],'usb_address_not_ready')
        self.ctx.path('/data').mkdir(exist_ok=True)
        self.ctx.path('/data/.y2data-schema').write_text('1')
        self.ctx.runner=lambda *a,**k:{'ok':True,'output':json.dumps([{'ifname':'usb0','addr_info':[{'local':'10.42.0.1','prefixlen':24}]}])}
        self.assertEqual(prerequisites(self.ctx,True)[0],'Ready')


if __name__=='__main__': unittest.main()
