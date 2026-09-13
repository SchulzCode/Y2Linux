#!/usr/bin/env python3
"""Build an isolated production candidate. No physical devices or deployment."""
import argparse, json, os, shlex, shutil, subprocess
from pathlib import Path
PROJECT=Path(__file__).resolve().parents[2]

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--output',default='out/y2linux-production-build-v1-r3')
    p.add_argument('--resume',choices=['kernel','buildroot','artifacts'])
    a=p.parse_args();out=(PROJECT/a.output).resolve()
    if not out.is_relative_to(PROJECT/'out') or (out.exists() and not a.resume):p.error('fresh directory inside out required')
    out.mkdir(parents=True,exist_ok=True)
    commit=subprocess.check_output(['git','rev-parse','HEAD'],cwd=PROJECT,text=True).strip()
    if subprocess.check_output(['git','status','--porcelain'],cwd=PROJECT,text=True).strip():p.error('commit the reviewed source before building a release candidate')
    versions={'release_version':'0.1.0-storage.3','layout_version':1,'kernel_version':'6.18.0-y2linux-storage03','rootfs_version':'2025.02.17-storage.3','data_schema_version':1,'y2player_version':None,'build_git_commit':commit}
    if a.resume in ('buildroot','artifacts') and (not (out/'kernel-source-commit').exists() or (out/'kernel-source-commit').read_text().strip()!=commit):p.error('kernel source commit changed; resume kernel first')
    (out/'versions.json').write_text(json.dumps(versions,indent=2)+'\n')
    lock=json.loads((PROJECT/'buildroot/inputs.lock.json').read_text())
    source=PROJECT/'.cache/sources'/('buildroot-'+lock['version'])
    if not source.is_dir():p.error('prepare the locked Buildroot source using the canonical development builder first')
    env=os.environ.copy();env.pop('Y2_PUBLIC_KEY_FILE',None)
    env.update(Y2_ARTIFACT_DIR=str(out),BR2_DL_DIR=str(PROJECT/'.cache/buildroot-dl'),SOURCE_DATE_EPOCH=str(lock['source_date_epoch']),E2FSPROGS_FAKE_TIME=str(lock['source_date_epoch']),LC_ALL='C',TZ='UTC')
    if not shutil.which('bc'):
        env['PATH']=str(PROJECT/'.cache/host-tools')+os.pathsep+env['PATH']
    def run(argv,log):
        print(log,flush=True)
        with (out/log).open('a') as f:subprocess.run(argv,cwd=PROJECT,env=env,stdout=f,stderr=subprocess.STDOUT,check=True)
    builder=['python3','tools/build/run.py','--output',str(out.relative_to(PROJECT)),'--']
    stage=['kernel','buildroot','artifacts'].index(a.resume or 'kernel')
    if stage<=0:
        run(builder+['sh','-c','make -C /src O=/build/kernel KCONFIG_ALLCONFIG=/project/kernel/config/production.config allnoconfig && python3 /project/tools/build/check_config.py /build/kernel/.config /project/kernel/config/production.config && make -C /src O=/build/kernel -j12 Image zImage modules && sh /project/tools/build/dev_observer.sh'],'kernel-build.log')
        (out/'kernel-source-commit').write_text(commit+'\n')
    if stage<=1:
        br=['make','-C',str(source),'O='+str(out/'buildroot'),'BR2_EXTERNAL='+str(PROJECT/'buildroot')]
        run(br+['y2_production_defconfig'],'buildroot-configure.log')
        run(br+['-j12','toolchain'],'buildroot-toolchain.log')
        cc=str(out/'buildroot/host/bin/arm-linux-gcc')
        for name,extra in [('fbtest',[]),('abi-check',['-mcpu=cortex-a7','-mfpu=neon-vfpv4','-mfloat-abi=hard','-marm','-pthread'])]:
            run([cc,'-Os','-Wall','-Wextra','-Werror',*extra,'tools/development/'+name+'.c','-o',str(out/('y2-'+name))],name+'-build.log')
        run([cc,'-Os','-Wall','-Wextra','-Werror','tools/production/usb-status.c','-o',str(out/'y2-usb-status')],'usb-status-build.log')
        run(['python3','tools/development/audio-fixtures.py',str(out/'audio')],'audio-fixtures.log')
        run(br+['-j12','all'],'buildroot-build.log')
    if stage<=2:
        run(builder+['sh','-c','cd /project && python3 -m tools.build.dev_initramfs --production && Y2_DTS=innioasis-y2-production.dts sh tools/build/dtb.sh && python3 -m tools.validation.dev_artifacts /build --package --production'],'artifact-validation.log')
    print('Built production candidate:',out,flush=True)
if __name__=='__main__':main()
