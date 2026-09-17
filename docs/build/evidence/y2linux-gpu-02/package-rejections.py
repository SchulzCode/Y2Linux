from pathlib import Path
import copy,json,shutil,tempfile,os
from tools.production.validate import validate_manifest

source=Path('out/y2linux-gpu-02').resolve()
original=json.loads((source/'manifest.json').read_text())

def altered_data(m):
    next(x for x in m['installed_components'] if x['target_partition']=='USRDATA')['raw']['sha256']='0'*64

cases=[
 ('added data payload',lambda m:m['payloads'].append(copy.deepcopy(next(x for x in m['installed_components'] if x['target_partition']=='USRDATA'))),'exact payload allowlist'),
 ('protected kernel write',lambda m:m['runtime_kernel_write_allowlist'].append('NVRAM'),'runtime allowlist'),
 ('data schema reset',lambda m:m.update(data_schema_version=2),'build version receipt'),
 ('source mismatch',lambda m:m.update(rootfs_build_git_commit='0'*40),'kernel/root source receipt'),
 ('changed data identity',altered_data,'new root plus unchanged data contract'),
 ('fallback identity lie',lambda m:m['fallback']['images'][0].update(sha256='0'*64),'fallback identity receipt'),
 ('firmware redistribution claim',lambda m:m['owner_firmware'].update(redistribution_permission_established=True),'owner-only provisioning'),
 ('data scatter selection',lambda m:None,'exact preserving scatter'),
 ('unexpected data image',lambda m:None,'no data image in a system update'),
]
for name,change,expected in cases:
 with tempfile.TemporaryDirectory(dir='out',prefix='gpu-package-test-') as temporary:
  out=Path(temporary)/'package'
  def copy_file(src,dst):
   if Path(src).suffix=='.img':os.link(src,dst)
   else:shutil.copyfile(src,dst)
  shutil.copytree(source,out,copy_function=copy_file)
  m=copy.deepcopy(original);change(m)
  (out/'manifest.json').write_text(json.dumps(m))
  if name=='data scatter selection':
   p=out/'MT6582_preserve_data_scatter.txt';s=p.read_text();s=s.replace('partition_name: USRDATA\n  file_name: NONE\n  is_download: false','partition_name: USRDATA\n  file_name: NONE\n  is_download: true');p.write_text(s)
  if name=='unexpected data image':(out/'Y2DATA.img').write_bytes(b'bad')
  try:validate_manifest(out)
  except ValueError as error:
   assert expected in str(error),(name,str(error));print('PASS rejected',name,':',error)
  else:raise AssertionError('Accepted '+name)
print('PASS 9 isolated preserving-package rejection cases; image targets were hard links opened only for reading')
