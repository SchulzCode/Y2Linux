"""Readback expectations follow installation intent, not the bundled data template."""
import hashlib,json,subprocess,tempfile,unittest
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
class Readback(unittest.TestCase):
    def test_preserve_data_and_first_install_have_distinct_expectations(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d);package=p/'pkg';before=p/'before';after=p/'after'
            for folder in (package/'metadata',before,after):folder.mkdir(parents=True)
            rows=[]
            for name,data in [('BOOTIMG',b'boot'),('ANDROID',b'root'),('USRDATA',b'seed')]:
                rows.append({'target_partition':name,'raw':{'size_bytes':4,'sha256':hashlib.sha256(data).hexdigest()},'maximum_size_bytes':8,'absolute_start_bytes':0})
                (after/(name+'.bin')).write_bytes(data+b'0000')
            (before/'USRDATA.bin').write_bytes(b'userdata');(after/'USRDATA.bin').write_bytes(b'userdata')
            (before/'NVRAM.bin').write_bytes(b'keep');(after/'NVRAM.bin').write_bytes(b'keep')
            (package/'manifest.json').write_text(json.dumps({'payloads':rows}))
            (package/'metadata/readback-plan.json').write_text(json.dumps({'preservation_ranges':[{'file':'NVRAM.bin','size_bytes':4}]}))
            cmd=['python3',str(ROOT/'tools/production/verify_readback.py'),'--package',str(package),'--before',str(before),'--after',str(after),'--profile']
            def run(profile):return subprocess.run(cmd+[profile],capture_output=True,text=True).returncode
            self.assertEqual(run('preserve-data'),0);self.assertNotEqual(run('first-install'),0)
            (after/'USRDATA.bin').write_bytes(b'seed0000')
            self.assertEqual(run('first-install'),0);self.assertNotEqual(run('preserve-data'),0)
            (after/'NVRAM.bin').write_bytes(b'bad!');self.assertNotEqual(run('first-install'),0)
            (after/'NVRAM.bin').write_bytes(b'keep');(after/'ANDROID.bin').write_bytes(b'bad!0000');self.assertNotEqual(run('first-install'),0)
