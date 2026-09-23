"""Qualification collection never turns gaps, new processes or new sources into passes."""
import json
from pathlib import Path
import tempfile
import unittest
import sys
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'tools/platform'))
from qualify import analyze, identity, run
from y2_platform.common import Context
from y2_platform.observe import snapshot

class QualificationTests(unittest.TestCase):
    def fixture(self,root):
        ctx=Context(root)
        versions=dict(build_git_commit='1'*40,reborn_source_commit='2'*40,kernel_version='6.18-test',rootfs_version='root-v1')
        for name,content in [('etc/y2linux/versions.json',json.dumps(versions)),('proc/sys/kernel/random/boot_id','boot-a'),('proc/sys/kernel/osrelease','6.18-test')]:
            path=Path(root)/name;path.parent.mkdir(parents=True,exist_ok=True);path.write_text(content)
        return ctx,identity(versions)

    def test_transport_gap_retained_and_capture_never_declares_endurance(self):
        with tempfile.TemporaryDirectory() as directory:
            ctx,expected=self.fixture(Path(directory)/'fixture');clock=[0];calls=[]
            def runner(argv,**kwargs):
                calls.append(argv)
                if 'dmesg' in argv[-1]:return dict(ok=True,output='bounded kernel evidence',reason=None)
                if clock[0]==10:return dict(ok=False,reason='command_timeout',output=None)
                value=snapshot(ctx);value['record']['monotonic_ns']=int((clock[0]+1)*1e9)
                return dict(ok=True,reason=None,output=json.dumps(value))
            output=Path(directory)/'capture'
            receipt=run('usb-cycles',expected,output,31,10,['fake-ssh'],runner,
                        lambda n:clock.__setitem__(0,clock[0]+n),lambda:clock[0])
            self.assertEqual((receipt['samples'],receipt['transport_gaps']),(3,1))
            self.assertFalse(receipt['endurance_qualified'])
            self.assertEqual(receipt['physical_acceptance'],'PENDING_OWNER_REVIEW')
            result=analyze(output/'samples.jsonl')
            self.assertEqual((result['samples'],result['transport_gaps']),(3,1))
            self.assertEqual(result['counter_deltas'],{})
            self.assertEqual((output/'samples.jsonl').stat().st_mode&0o777,0o600)
            self.assertTrue(all('status' in call[-1] or 'dmesg' in call[-1] for call in calls))

    def test_source_mismatch_and_no_samples_fail_closed(self):
        with tempfile.TemporaryDirectory() as directory:
            ctx,expected=self.fixture(Path(directory)/'fixture');sample=snapshot(ctx)
            sample['record']['y2linux_commit']='3'*40
            receipt=run('wired-8h',expected,Path(directory)/'capture',1,10,[],
                        lambda *a,**k:dict(ok=True,reason=None,output=json.dumps(sample)))
            self.assertEqual(receipt['failure'],'candidate_identity_mismatch')
            self.assertEqual(receipt['state'],'CaptureIncomplete')
            with self.assertRaises(ValueError): identity({})

    def test_counter_reset_pid_restart_and_boot_do_not_create_negative_deltas(self):
        with tempfile.TemporaryDirectory() as directory:
            ctx,_=self.fixture(Path(directory)/'fixture');path=Path(directory)/'samples'
            values=[]
            for t,boot,pid,counter in [(1,'a',1,5),(61,'a',1,7),(121,'a',1,1),(181,'a',2,30),(1,'b',2,10)]:
                value=snapshot(ctx);value['record'].update(boot_id=boot,monotonic_ns=t*10**9)
                value['memory']['processes']=[dict(pid=pid,start_ticks=pid*100,rss_kib=1000+t)]
                value['reborn_metrics']={'audio_xruns':counter};values.append(value)
            path.write_text(''.join(json.dumps(v)+'\n' for v in values))
            result=analyze(path)
            self.assertEqual(result['counter_deltas']['reborn:audio_xruns'],2)
            self.assertEqual(result['counter_resets']['reborn:audio_xruns'],1)
            self.assertEqual(result['boot_ids'],['a','b'])
            self.assertEqual(len(result['memory_growth_after_60s']),2)
            self.assertNotIn('packet_loss',result['counter_deltas'])
            path.write_text(json.dumps(values[0])+'\n'+json.dumps(values[0])+'\n')
            with self.assertRaisesRegex(ValueError,'non_increasing'):analyze(path)

if __name__=='__main__':unittest.main()
