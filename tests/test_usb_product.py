"""USB kernel overlay, responder build, host harness and file-ownership regressions. The platform's USB
service tests are Rust: platform/crates/y2-platform/src/commands/service/usb_tests.rs."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import tarfile
import shutil
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]


class USBProduct(unittest.TestCase):
    def test_musb_overlay_fixes_idle_queue_flush_and_stale_dma_mapping(self):
        patch_text = (ROOT / 'kernel/patches/0065-y2-musb-pio-progress.patch').read_text()
        # nuke() releases the channel first; the request must still be unmapped, or
        # FunctionFS's persistent ep->req keeps a stale request->dma (zero payloads).
        self.assertIn('+\tif (!is_buffer_mapped(request))', patch_text)
        self.assertNotIn('+\tif (!is_buffer_mapped(request) || !musb_ep->dma)', patch_text)
        # A request queued on an idle OUT endpoint must service, not flush, the FIFO.
        self.assertIn('+static void musb_ep_start(struct musb *musb, struct musb_request *req)', patch_text)
        self.assertIn('+\t\trxstate(musb, req, true);', patch_text)
        # Packets already ACKed before the request existed are read by the CPU.
        self.assertIn('+\t\t\tif (late && y2_musb_late_pio()) {', patch_text)
        # V1 code review B6: a multi-packet start that gives up stops the engine
        # first and hands the packet to the CPU only if nothing reached memory.
        start = patch_text[patch_text.index('+static int y2_rx_mode1_start('):
                           patch_text.index('+static bool y2_rx_mode1_end(')]
        fallback = start[start.index('+\tmoved = y2_hsdma_stop(channel);'):]
        self.assertLess(fallback.index('y2_hsdma_stop'), fallback.index('c->channel_abort(channel);'))
        self.assertLess(fallback.index('if (!moved)\n+\t\treturn Y2_M1_PIO;'),
                        fallback.index('musb_g_giveback(musb_ep, request, -EIO);'))
        self.assertIn('MUSB_RXCSR_FLUSHFIFO', fallback)
        hsdma = (ROOT / 'kernel/patches/0055-musb-abort-bus-error.patch').read_text()
        self.assertIn('+u32 y2_hsdma_stop(struct dma_channel *channel)', hsdma)
        manifest = json.loads((ROOT / 'kernel/patches/manifest.json').read_text())
        spec = next(o for o in manifest['overlays'] if o['path'] == 'drivers/usb/musb/musb_gadget.c')
        self.assertEqual(spec['patch'], '0065-y2-musb-pio-progress.patch')
        usb = (ROOT / 'kernel/platform/usb.c').read_text()
        # BC1.1 touches the PHY only while the controller's bus clock is running.
        begin = usb[usb.index('int y2_usb_bc11_begin(void)'):usb.index('int y2_usb_bc11_end')]
        self.assertLess(begin.index('pm_runtime_resume_and_get'), begin.index('readb(y2_usb_phy + 0x1a)'))
        end = usb[usb.index('int y2_usb_bc11_end'):usb.index('/* USB core calls set_power')]
        self.assertIn('y2_bc11_pm_release()', end)


class PhysicalHarness(unittest.TestCase):
    def setUp(self):
        spec=importlib.util.spec_from_file_location('usb_qualify',ROOT/'tools/usb/qualify.py')
        self.module=importlib.util.module_from_spec(spec); spec.loader.exec_module(self.module)

    def test_cpu_irq_context_switch_deltas_have_explicit_average_scope(self):
        value=self.module.delta({'stat':'cpu 10 0 20 100 0 0 0 0\nintr 100\nctxt 200'},
                                {'stat':'cpu 20 0 30 120 0 0 0 0\nintr 160\nctxt 280'},2)
        self.assertEqual(value['cpu_busy_percent'],50)
        self.assertEqual(value['irq_per_s'],30)
        self.assertEqual(value['context_switches_per_s'],40)

    def test_process_metrics_refuse_reused_pid_and_label_memory_scope(self):
        before={'stat':'','processes':{'9':{'start':'one','ticks':100},'10':{'start':'old','ticks':1}}}
        after={'stat':'','hz':100,'processes':{'9':{'start':'one','ticks':200,'name':'umtprd','memory_kib':{'VmRSS':1000}},'10':{'start':'new','ticks':200,'name':'dropbear','memory_kib':{}}}}
        value=self.module.delta(before,after,2)['processes']
        self.assertEqual(len(value),1)
        self.assertEqual(value[0]['cpu_percent_one_core'],50)
        self.assertEqual(value[0]['memory_kib']['VmRSS'],1000)
        self.assertIn('process lifetime',value[0]['memory_scope'])
        compile(self.module.SNAPSHOT,'remote-snapshot','exec')

    def test_boot_or_media_swap_refuses_following_transfer_or_cleanup(self):
        from types import SimpleNamespace
        harness=self.module.Harness(SimpleNamespace(ssh_config=None,host='fixture'))
        harness.identity={'boot':'one','mounts':'5 1 179:2 / /data rw - ext4 /dev/data rw'}
        with patch.object(harness,'snapshot',return_value={'boot':'two','mounts':harness.identity['mounts']}):
            with self.assertRaisesRegex(RuntimeError,'identity_changed'): harness.guard()
        with patch.object(harness,'snapshot',return_value={'boot':'one','mounts':'6 1 179:2 / /data rw - ext4 /dev/data rw'}):
            with self.assertRaisesRegex(RuntimeError,'identity_changed'): harness.guard()

    def test_benchmark_requires_the_exact_writable_user_partition(self):
        good={'state':'Ready','uuid':'79324c69-6e75-4801-8000-000000000103',
              'start':'3742720','sectors':'11460608','filesystem':'ext4'}
        self.assertEqual(self.module.validate_volume([{},{},{},good],'internal'),good)
        for changes in ({'uuid':'system'},{'start':'0'},{'state':'Degraded'},{'uuid':'79324c69-6e75-4801-8000-000000000102'}):
            with self.assertRaises(RuntimeError):self.module.validate_volume([{},{},{},dict(good,**changes)],'internal')

    def test_sd_benchmark_refuses_missing_dirty_or_readonly_media(self):
        for state in ('Unavailable','Failed','Degraded'):
            with self.assertRaises(RuntimeError):self.module.validate_volume([{}, {}, {'state':state,'filesystem':'exfat'}],'sd')
        for filesystem in ('vfat','exfat','ext4'):
            self.assertEqual(self.module.validate_volume([{}, {}, {'state':'Ready','filesystem':filesystem}],'sd')['filesystem'],filesystem)

    def test_deadline_prevents_copy_and_many_files_use_one_sftp_batch(self):
        import time
        from types import SimpleNamespace
        harness=self.module.Harness(SimpleNamespace(ssh_config=None,host='fixture',protocol='sftp'))
        harness.device_dir='/data/unique-fixture'; harness.deadline=time.monotonic()-1
        with patch.object(harness,'sftp_batch') as batch:
            with self.assertRaisesRegex(RuntimeError,'deadline'): harness.copies([Path('one')],Path('/tmp'),True)
            batch.assert_not_called()
            harness.deadline=time.monotonic()+60
            harness.copies([Path('one'),Path('two')],Path('/tmp'),True)
            self.assertEqual(len(batch.call_args.args[0]),2)
            self.assertEqual(batch.call_count,1)


class USBFiles(unittest.TestCase):
    def test_actual_responder_operations_and_descriptors(self):
        archive=ROOT/'.cache/buildroot-dl/y2-mtp/y2-mtp-2241a803c27f1fff7537cb9504befeb810b880d3.tar.gz'
        if not archive.is_file(): self.skipTest('pinned responder archive not prepared')
        self.assertEqual(hashlib.sha256(archive.read_bytes()).hexdigest(), '077ef6f4745d599bda7db0fff2993e21b969bbf414784c4ab3adf0f664177d2b')
        with tempfile.TemporaryDirectory() as temporary:
            base=Path(temporary)
            with tarfile.open(archive) as source: source.extractall(base,filter='data')
            source=next(base.glob('uMTP-Responder-*'))
            for patchfile in sorted((ROOT/'buildroot/package/y2-mtp').glob('*.patch')):
                subprocess.run(['patch','--batch','--fuzz=0','-p1','-i',str(patchfile)],cwd=source,check=True,stdout=subprocess.DEVNULL)
            for name in ('h','c'): shutil.copyfile(ROOT/('tools/usb/mtp-safe.'+name),source/('inc' if name=='h' else 'src')/('mtp-safe.'+name))
            # Patch 0005 reads tags through SQLite; the locked container has
            # no SQLite headers, so link the failing stub there (file-name fallback).
            sqlite=[]
            if not Path('/usr/include/sqlite3.h').exists():
                stub=ROOT/'tests/native/sqlite3-stub'; library=base/'sqlite-stub'; library.mkdir()
                subprocess.run(['cc','-c','-Wall','-Werror',str(stub/'sqlite3.c'),'-o',str(library/'sqlite3.o')],check=True)
                subprocess.run(['ar','rcs',str(library/'libsqlite3.a'),str(library/'sqlite3.o')],check=True)
                sqlite=['-I'+str(stub),'-L'+str(library)]
            env=dict(os.environ,CPPFLAGS=' '.join(f for f in sqlite if f.startswith('-I')),
                     LDFLAGS=' '.join(f for f in sqlite if f.startswith('-L')))
            build=subprocess.run(['make','-j4'],cwd=source,env=env,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True)
            self.assertEqual(build.returncode,0,build.stdout)
            receive_test=base/'receive-test'
            subprocess.run(['cc','-Wall','-Wextra','-Werror','-pthread','-I'+str(source/'inc'),
                            str(ROOT/'tests/native/mtp-rx-events.c'),str(source/'src/y2_rx.c'),
                            '-o',str(receive_test)],check=True)
            subprocess.run([str(receive_test)],check=True,timeout=10)
            flags=['cc','-I'+str(source/'inc'),*sqlite]
            main=base/'main.o'
            subprocess.run(flags+['-Dmain=umtprd_original_main','-c',str(source/'src/umtprd.c'),'-o',str(main)],check=True)
            binary=base/'protocol-test'
            objects=[str(o) for o in (source/'obj').glob('*.o') if o.name!='umtprd.o']
            subprocess.run(flags+[str(ROOT/'tests/usb_mtp_protocol.c'),str(main),*objects,'-lsqlite3','-lpthread','-lrt','-o',str(binary)],check=True)
            media=base/'media'; media.mkdir()
            subprocess.run([str(binary),str(media)],check=True,timeout=5)
            self.assertFalse(list(media.glob('.y2-mtp-*')))

    @unittest.skipUnless(shutil.which('cc') and Path('/usr/include/libmtp.h').exists(), 'libmtp headers required')
    def test_host_file_operations_tool_builds(self):
        """tools/usb/mtp-file-ops.c drives a connected player (copy, rename, move, copy, delete)."""
        with tempfile.TemporaryDirectory() as temporary:
            subprocess.run(['cc','-O2','-Wall','-Wextra','-Werror','-o',str(Path(temporary)/'ops'),
                            str(ROOT/'tools/usb/mtp-file-ops.c'),'-lmtp'],check=True)

    def test_real_atomic_files_and_fault_injection(self):
        with tempfile.TemporaryDirectory() as temporary:
            base=Path(temporary); media=base/'media'; media.mkdir()
            binary=base/'fs-test'
            subprocess.run(['cc','-Wall','-Wextra','-Werror','-I'+str(ROOT/'tools/usb'),str(ROOT/'tests/usb_mtp_fs.c'),'-o',str(binary)],check=True)
            subprocess.run([str(binary),str(media)],check=True)
            expected=hashlib.sha256(b'abcdef').hexdigest()
            for file in media.glob('*.mp3'):
                if file.name!='partial.mp3': self.assertEqual(hashlib.sha256(file.read_bytes()).hexdigest(),expected)
            self.assertEqual(hashlib.sha256((media/'one-MiB.flac').read_bytes()).hexdigest(),hashlib.sha256(b'a'*1024**2).hexdigest())
            self.assertFalse(list(media.glob('.y2-mtp-*')))


if __name__ == '__main__': unittest.main()
