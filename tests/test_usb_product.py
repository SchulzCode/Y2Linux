"""Behavioral product, descriptor readiness and file-ownership regressions."""
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import tarfile
import shutil
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools/platform'))
from y2_platform.common import Context
from y2_platform.usb import USBService, SCHEMA, GADGET, config, selection, media_views, release_sd


class Child:
    pid = 123
    def __init__(self): self.result = None
    def poll(self): return self.result
    def terminate(self): self.result = 0
    def kill(self): self.result = -9
    def wait(self, **kw): return self.result


class USBProduct(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.ctx = Context(self.temp.name)
        self.put('/proc/sys/kernel/random/boot_id', 'boot-one')
        self.put('/sys/class/udc/musb/state', 'not attached')
        self.ctx.path('/sys/kernel/config/usb_gadget').mkdir(parents=True)
        self.ctx.path('/run/y2').mkdir(parents=True)
        self.views = [{'path':'/media/internal/music', 'name':'Internal Storage', 'access':'rw', 'mount_id':5}]
        self.service = USBService(self.ctx, launch=Child)
        self.mock = patch('y2_platform.usb.media_views', return_value=self.views)
        self.mock.start(); self.addCleanup(self.mock.stop)
        self.ctx.runner = lambda *_a, **_k: {'ok':True, 'reason':None, 'output':''}

    def put(self, name, value):
        path = self.ctx.path(name); path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(value); return path

    def ready(self):
        self.service.tick()
        for i in (1,2,3): self.put('/dev/ffs-mtp/ep%s' % i, '')
        self.put(GADGET + '/functions/ffs.mtp/ready', '1')
        self.put('/sys/class/udc/musb/state', 'configured')
        return self.service.tick()

    def test_no_udc_bind_before_all_functionfs_descriptors(self):
        state = self.service.tick()
        self.assertEqual(state['reason'], 'waiting_for_descriptors')
        self.assertFalse(self.ctx.path(GADGET+'/UDC').exists())
        self.assertEqual(self.ready()['state'], 'Ready')
        self.assertEqual(self.ctx.read(GADGET+'/UDC'), 'musb')
        self.assertEqual(self.ctx.read(GADGET+'/os_desc/use'),'1')
        self.assertTrue(self.ctx.path(GADGET+'/os_desc/c.1').is_symlink())
        self.assertEqual(list(selection('mtp')), ['ffs.mtp','ecm.usb0'])

    def test_no_block_export_or_triple_function_selection(self):
        for invalid in ('msc','mass_storage','mtp+acm+ecm','/dev/mmcblk0','Y2DATA','BOOTIMG'):
            with self.assertRaises(ValueError): selection(invalid)
        self.assertEqual(selection('developer'), ('acm.usb0','ecm.usb0'))
        for path in ('/','/data','/dev/mmcblk0','/data/reborn','/data/ssh'):
            with self.assertRaises(ValueError): config([{'path':path,'name':'Internal Storage','access':'rw'}])
        self.assertIn('storage "/media/internal/music" "Internal Storage" "rw"', config(self.views))

    def test_disconnect_refresh_keeps_linux_ownership_and_reconnect(self):
        self.ready()
        self.put('/sys/class/udc/musb/state','not attached')
        disconnected = self.service.tick()
        self.assertEqual(disconnected['state'],'Disconnected')
        self.assertGreater(disconnected['refresh_generation'],0)
        self.assertEqual(disconnected['filesystem_owner'],'linux')
        self.assertFalse(disconnected['block_export'])
        self.put('/sys/class/udc/musb/state','configured')
        self.assertEqual(self.service.tick()['state'],'Ready')

    def test_only_durable_generation_not_activity_requests_library_refresh(self):
        self.ready()
        self.put('/run/y2/mtp-events.json',json.dumps({'pid':123,'active':True,'generation':0}))
        value=self.service.tick(); self.assertEqual(value['state'],'Copying'); self.assertEqual(value['refresh_generation'],0)
        self.put('/run/y2/mtp-events.json',json.dumps({'pid':123,'active':False,'generation':5}))
        first=self.service.tick()['refresh_generation']; self.assertGreater(first,0)
        self.assertEqual(first,self.service.tick()['refresh_generation'])

    def test_failed_daemon_and_failed_bind_have_bounded_retry(self):
        self.ready()
        for _ in range(5):
            if self.service.child: self.service.child.result=1
            self.service.tick(); self.service.tick()
        self.assertTrue(self.service.terminal)
        self.assertEqual(self.service.tick()['state'],'Failed')

    def test_failure_budget_decays_after_a_stable_responder(self):
        self.ready()
        self.service.failures = 4
        self.service.started -= 120
        self.service.child.result = 1
        self.service.tick()
        self.assertEqual(self.service.failures, 1)
        self.assertFalse(self.service.terminal)

    def test_bind_failure_never_reports_host_ownership(self):
        self.service.tick()
        for i in (1,2,3): self.put('/dev/ffs-mtp/ep%s' % i,'')
        self.put(GADGET+'/functions/ffs.mtp/ready','1')
        original=self.service.write
        def failed(relative,value):
            if relative=='UDC' and value: raise OSError('bind_failed')
            original(relative,value)
        with patch.object(self.service,'write',side_effect=failed):
            value=self.service.tick()
        self.assertEqual(value['state'],'Failed')
        self.assertEqual(value['filesystem_owner'],'linux')
        self.assertFalse(self.service.bound)
        self.assertIsNone(self.service.child)

    def test_readiness_restart_unbinds_its_existing_gadget_before_setup(self):
        self.ready()
        restarted=USBService(self.ctx,launch=Child)
        seen=[]; original=restarted.write
        def observed(relative,value):
            if relative=='UDC': seen.append(value)
            original(relative,value)
        with patch.object(restarted,'write',side_effect=observed): restarted.tick()
        self.assertEqual(seen[0],'')
        self.assertEqual(seen[-1],'musb')
        self.assertEqual(restarted.tick()['filesystem_owner'],'linux')

    def test_gadget_requests_validated_bus_powered_500ma_allocation(self):
        # The charger grants 450 mA to a *configured* 500 mA request only; a
        # self-powered/0 mA configuration left a PC port "Not charging".
        self.ready()
        self.assertEqual(self.ctx.read(GADGET+'/configs/c.1/MaxPower'), '500')
        self.assertEqual(self.ctx.read(GADGET+'/configs/c.1/bmAttributes'), '0x80')

    def test_both_views_declared_in_fixed_slots_missing_sd_is_notmounted(self):
        text = config(self.views)
        self.assertIn('storage "/media/internal/music" "Internal Storage" "rw"', text)
        self.assertIn('storage "/media/sd" "SD Card" "rw,removable,notmounted"', text)
        sd = {'path':'/media/sd','name':'SD Card','access':'ro','mount_id':9}
        text = config(self.views + [sd])
        self.assertIn('storage "/media/sd" "SD Card" "ro,removable"', text)
        self.assertLess(text.index('Internal Storage'), text.index('SD Card'))

    def test_sd_hotplug_changes_the_live_view_without_usb_reenumeration(self):
        self.ready()
        binds = []; original = self.service.write
        def watch(relative, value):
            if relative == 'UDC': binds.append(value)
            original(relative, value)
        sent = []
        def command(message):
            sent.append(message)
            name = message.split(':', 1)[1]
            roots = {'Internal Storage': '/media/internal/music', 'SD Card': '/media/sd'}
            current = set(json.loads(self.ctx.read('/run/y2/mtp-roots.json') or '{"roots":[]}')['roots'])
            (current.add if message.startswith('mount') else current.discard)(roots[name])
            self.put('/run/y2/mtp-roots.json', json.dumps({'pid':123, 'roots':sorted(current)}))
            return True
        self.put('/run/y2/mtp-roots.json', json.dumps({'pid':123, 'roots':['/media/internal/music']}))
        sd = {'path':'/media/sd','name':'SD Card','access':'rw','mount_id':9,'device_id':'179:9','uuid':'u'}
        with patch.object(self.service, 'write', side_effect=watch), patch.object(self.service, 'command', side_effect=command):
            self.mock.stop(); self.mock = patch('y2_platform.usb.media_views', return_value=self.views + [sd]); self.mock.start()
            self.put('/proc/self/mountinfo', 'sd inserted')
            first = self.service.tick(); generation = first['refresh_generation']
            self.assertEqual(sent, ['mount:SD Card'])
            self.assertEqual([v['path'] for v in first['views']], ['/media/internal/music', '/media/sd'])
            self.mock.stop(); self.mock = patch('y2_platform.usb.media_views', return_value=list(self.views)); self.mock.start()
            self.put('/proc/self/mountinfo', 'sd removed')
            second = self.service.tick()
        self.assertEqual(sent, ['mount:SD Card', 'unmount:SD Card'])
        self.assertEqual([v['path'] for v in second['views']], ['/media/internal/music'])
        self.assertEqual(binds, [])
        self.assertGreater(second['refresh_generation'], 0)

    def test_unacknowledged_unmount_falls_back_to_stopping_the_responder(self):
        self.ready(); old = self.service.child
        sd = {'path':'/media/sd','name':'SD Card','access':'rw','mount_id':9,'device_id':'179:9','uuid':'u'}
        self.service.mounted['/media/sd'] = ('9', '179:9', 'u', 'rw')
        self.service.views = self.views + [sd]
        with patch.object(self.service, 'command', return_value=True), \
             patch.object(self.service, 'await_root', return_value=False):
            self.put('/proc/self/mountinfo', 'sd gone')
            self.service.tick()
        self.assertIsNot(self.service.child, old)  # old responder released every pin

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

    def test_media_identity_change_stops_old_responder_before_new_view(self):
        self.ready(); old=self.service.child
        self.views[0]['mount_id'] = 6
        self.mock.stop()
        self.mock = patch('y2_platform.usb.media_views', return_value=[dict(self.views[0])])
        self.mock.start()
        self.service.views = [{'path':'/media/internal/music', 'name':'Internal Storage', 'access':'rw', 'mount_id':5}]
        self.put('/proc/self/mountinfo','new mount generation')
        self.service.tick()
        self.assertEqual(old.result,0)
        self.assertIsNot(self.service.child,old)

    def test_developer_change_waits_for_disconnect(self):
        self.ready(); self.put('/data/system/platform/usb-mode.json','{"mode":"developer"}')
        self.assertEqual(self.service.tick()['mode'],'mtp')
        self.put('/sys/class/udc/musb/state','not attached')
        self.assertEqual(self.service.tick()['mode'],'developer')

    def test_sd_eject_refuses_connected_host_and_preserves_mount(self):
        record={'schema':SCHEMA,'boot_id':'boot-one','cable':'configured','views':[{'path':'/media/sd'}]}
        self.put('/run/y2/usb-transfer.json',json.dumps(record))
        self.assertFalse(release_sd(self.ctx,42))
        self.assertFalse(self.ctx.path('/run/y2/usb-release-sd.json').exists())

    def test_responder_roots_are_exactly_the_platform_export_paths(self):
        """The responder patches name the roots it pins; a stale path leaves a storage declared but never mounted."""
        from y2_platform.usb import NAMES
        patches = ''.join((ROOT/'buildroot/package/y2-mtp'/name).read_text()
                          for name in ('0001-y2-safe-media-transfers.patch', '0002-y2-mtp-pipeline-and-live-storage.patch'))
        added = [line[1:] for line in patches.splitlines() if line.startswith('+')]
        for path in NAMES:
            self.assertTrue(any('strcmp(root, "%s")' % path in line for line in added), path)
        self.assertTrue(any('y2_fs_init(has_internal ? "%s" : NULL' % [p for p in NAMES if p != '/media/sd'][0] in line
                            for line in added))
        self.assertNotIn('/data/music', ''.join(added))


class MediaViews(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory(); self.addCleanup(self.temp.cleanup)
        self.ctx=Context(self.temp.name)
        self.ctx.path('/media/internal/music').mkdir(parents=True)
        self.ctx.path('/media/sd').mkdir(parents=True)
        device=self.ctx.path('/sys/dev/block/179:2'); device.mkdir(parents=True)
        (device/'start').write_text('3742720'); (device/'size').write_text('11460608')
        self.volumes=[{}, {'path':'/data','state':'Ready','uuid':'79324c69-6e75-4801-8000-000000000102'},
                         {'path':'/media/sd','state':'Unavailable'},
                         {'path':'/media/internal','state':'Ready','uuid':'79324c69-6e75-4801-8000-000000000103'}]
        self.mount='/proc/self/mountinfo'; self.ctx.path('/proc/self').mkdir(parents=True)
        self.ctx.path(self.mount).write_text('5 1 179:2 / /media/internal rw - ext4 /dev/mmcblk0p8 rw')
        mock=patch('y2_platform.usb.storage',return_value={'volumes':self.volumes})
        mock.start(); self.addCleanup(mock.stop)

    def test_missing_sd_never_exposes_empty_mountpoint(self):
        self.assertEqual([v['path'] for v in media_views(self.ctx)],['/media/internal/music'])

    def test_dirty_or_failed_media_and_remount_failure_are_not_exposed(self):
        self.volumes[2].update(state='Failed',reason='filesystem_check_required')
        self.ctx.path(self.mount).write_text(self.ctx.path(self.mount).read_text()+'\n6 1 179:9 / /media/sd rw - exfat /dev/mmcblk1p1 rw')
        self.assertEqual(len(media_views(self.ctx)),1)
        self.ctx.path(self.mount).write_text('')
        self.assertEqual(media_views(self.ctx),[])

    def test_readonly_superblock_is_never_advertised_writable(self):
        self.volumes[3]['state']='Degraded'
        self.ctx.path(self.mount).write_text('5 1 179:2 / /media/internal rw - ext4 /dev/mmcblk0p8 ro')
        self.assertEqual(media_views(self.ctx)[0]['access'],'ro')

    def test_wrong_internal_geometry_uuid_or_nested_mount_is_excluded(self):
        self.ctx.path('/sys/dev/block/179:2/start').write_text('0')
        self.assertEqual(media_views(self.ctx),[])
        self.ctx.path('/sys/dev/block/179:2/start').write_text('3742720')
        self.volumes[3]['uuid']='system-root'
        self.assertEqual(media_views(self.ctx),[])
        self.volumes[3]['uuid']='79324c69-6e75-4801-8000-000000000103'
        # The old Y2DATA geometry/uuid is no longer an acceptable internal export.
        self.ctx.path('/sys/dev/block/179:2/start').write_text('2104320')
        self.ctx.path('/sys/dev/block/179:2/size').write_text('1638400')
        self.volumes[3]['uuid']='79324c69-6e75-4801-8000-000000000102'
        self.assertEqual(media_views(self.ctx),[])
        self.ctx.path('/sys/dev/block/179:2/start').write_text('3742720')
        self.ctx.path('/sys/dev/block/179:2/size').write_text('11460608')
        self.volumes[3]['uuid']='79324c69-6e75-4801-8000-000000000103'
        self.ctx.path(self.mount).write_text(self.ctx.path(self.mount).read_text()+'\n8 5 179:1 / /media/internal/music rw - ext4 /dev/mmcblk0p1 rw')
        self.assertEqual(media_views(self.ctx),[])


    def test_old_storage_is_read_only_and_only_while_the_setup_waits(self):
        """STORAGE-SETUP case 3: the stock storage can be copied off, never written."""
        from types import SimpleNamespace
        self.ctx.path(self.mount).write_text('5 1 179:2 / /run/y2/old-storage ro - vfat /dev/mmcblk0p8 ro')
        self.ctx.path('/run/y2/old-storage').mkdir(parents=True)
        state={'state':'Waiting'}
        with patch('y2_platform.internal_media.partition',return_value=SimpleNamespace(name='mmcblk0p8')), \
             patch('y2_platform.internal_media.setup_status',side_effect=lambda ctx: dict(state)):
            views=media_views(self.ctx)
            self.assertEqual([(v['path'],v['name'],v['access']) for v in views],
                             [('/run/y2/old-storage','Old Storage','ro')])
            self.assertIn('storage "/run/y2/old-storage" "Old Storage" "ro"', config(views))
            state['state']='Running'                       # Set Up Now: it leaves the computer first
            self.assertEqual(media_views(self.ctx),[])
            self.assertIn('storage "/run/y2/old-storage" "Old Storage" "rw,notmounted"', config([]))

    def test_with_internal_storage_there_is_no_separate_firmware_drive(self):
        """#39: the copy goes into Internal Storage's Firmware folder instead of Y2DATA."""
        self.ctx.path(self.mount).write_text('5 1 179:2 / /media/internal rw - ext4 /dev/mmcblk0p8 rw\n'
                                             '7 1 179:7 / /data rw - ext4 /dev/mmcblk0p7 rw')
        with patch('y2_platform.internal_media.ready',return_value=False):
            self.assertIn('/data/firmware/inbox',[v['path'] for v in media_views(self.ctx)])
        with patch('y2_platform.internal_media.ready',return_value=True):
            views=[v['path'] for v in media_views(self.ctx)]
            self.assertNotIn('/data/firmware/inbox',views)
            self.assertIn('/media/internal/music',views)
            self.assertTrue(self.ctx.path('/media/internal/music/Firmware/README.txt').exists())


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
            build=subprocess.run(['make','-j4'],cwd=source,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True)
            self.assertEqual(build.returncode,0,build.stdout)
            receive_test=base/'receive-test'
            subprocess.run(['cc','-Wall','-Wextra','-Werror','-pthread','-I'+str(source/'inc'),
                            str(ROOT/'tests/native/mtp-rx-events.c'),str(source/'src/y2_rx.c'),
                            '-o',str(receive_test)],check=True)
            subprocess.run([str(receive_test)],check=True,timeout=10)
            flags=['cc','-I'+str(source/'inc')]
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
