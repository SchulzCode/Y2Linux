#!/usr/bin/env python3
"""Build an isolated production candidate. No physical devices or deployment."""
import argparse, json, os, re, shutil, subprocess
from pathlib import Path
import sys
sys.path.insert(0,str(Path(__file__).resolve().parents[2]))
from tools.production.prepare import buildroot_source, environment
PROJECT=Path(__file__).resolve().parents[2]

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--output',default='out/y2linux-m5-connectivity-03-build')
    p.add_argument('--resume',choices=['kernel','buildroot','artifacts'])
    p.add_argument('--reuse-userspace',type=Path,help='verified production package whose root/data and rescue binaries are retained')
    p.add_argument('--owner-firmware',type=Path,help='explicit local provision.py output; no downloads or calibration inputs')
    a=p.parse_args();out=(PROJECT/a.output).resolve()
    if not out.is_relative_to(PROJECT/'out') or (out.exists() and not a.resume):p.error('fresh directory inside out required')
    out.mkdir(parents=True,exist_ok=True)
    commit=subprocess.check_output(['git','rev-parse','HEAD'],cwd=PROJECT,text=True).strip()
    if subprocess.check_output(['git','status','--porcelain'],cwd=PROJECT,text=True).strip():p.error('commit the reviewed source before building a release candidate')
    localversion=re.search(r'^CONFIG_LOCALVERSION="([^"]+)"$',(PROJECT/'kernel/config/production.config').read_text(),re.M).group(1)
    versions={'release_version':'0.1.0-m5.connectivity.3','layout_version':1,'kernel_version':'6.18.0'+localversion,'rootfs_version':'2025.02.17-connectivity.3','data_schema_version':1,'y2player_version':None,'build_git_commit':commit}
    if not a.reuse_userspace:
        if not a.owner_firmware:p.error('M5 requires explicit --owner-firmware local provisioning')
        from tools.connectivity.provision import verify_provision
        provisioned=verify_provision(a.owner_firmware.resolve())
        (out/'owner-firmware.json').write_text(json.dumps({'schema':'org.schulzcode.y2linux.owner-firmware/v1',
            'redistribution_permission_established':False,'files':{
                n:{'size_bytes':v[0],'sha256':v[1]} for n,v in provisioned.items()}},indent=2)+'\n')
    if a.reuse_userspace:
        from tools.production.validate import validate_manifest
        base=a.reuse_userspace.resolve()
        if not base.is_relative_to(PROJECT/'out'):p.error('userspace package must be inside out')
        previous=validate_manifest(base)
        if previous['minimum_compatible_components']['rootfs_contract']!='y2-platform-v1':p.error('incompatible existing rootfs')
        versions['rootfs_version']=previous['rootfs_version']
        versions['rootfs_build_git_commit']=previous.get('rootfs_build_git_commit', previous['build_git_commit'])
    if a.resume in ('buildroot','artifacts') and (not (out/'kernel-source-commit').exists() or (out/'kernel-source-commit').read_text().strip()!=commit):p.error('kernel source commit changed; resume kernel first')
    (out/'versions.json').write_text(json.dumps(versions,indent=2)+'\n')
    environment(PROJECT)
    if not a.reuse_userspace:source,lock=buildroot_source(PROJECT)
    else:lock=json.loads((PROJECT/'buildroot/inputs.lock.json').read_text())
    env=os.environ.copy();env.pop('Y2_PUBLIC_KEY_FILE',None)
    if a.owner_firmware:env['Y2_OWNER_FIRMWARE']=str(a.owner_firmware.resolve())
    else:env.pop('Y2_OWNER_FIRMWARE',None)
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
        if a.reuse_userspace:
            run(builder+['python3','/project/tools/production/boot_update.py','--stage-userspace',
                         '/project/'+str(base.relative_to(PROJECT)),'/build'],'userspace-reuse.log')
            from tools.production.rescue_tools import rebuild
            rebuild(PROJECT, out, run)
        else:
            build_userspace(PROJECT,out,source,run)
    if stage<=2:
        run(builder+['sh','-c','cd /project && python3 -m tools.build.dev_initramfs --production && Y2_DTS=innioasis-y2-production.dts sh tools/build/dtb.sh && python3 -m tools.validation.dev_artifacts /build --package --production'],'artifact-validation.log')
    print('Built production candidate:',out,flush=True)

def build_userspace(PROJECT,out,source,run):
    br=['make','-C',str(source),'O='+str(out/'buildroot'),'BR2_EXTERNAL='+str(PROJECT/'buildroot')]
    run(br+['y2_production_defconfig'],'buildroot-configure.log')
    run(br+['-j12','toolchain'],'buildroot-toolchain.log')
    cc=str(out/'buildroot/host/bin/arm-linux-gcc')
    for name,extra in [('fbtest',[]),('abi-check',['-mcpu=cortex-a7','-mfpu=neon-vfpv4','-mfloat-abi=hard','-marm','-pthread'])]:
        run([cc,'-Os','-Wall','-Wextra','-Werror',*extra,'tools/development/'+name+'.c','-o',str(out/('y2-'+name))],name+'-build.log')
    run([cc,'-Os','-Wall','-Wextra','-Werror','tools/production/usb-status.c','-o',str(out/'y2-usb-status')],'usb-status-build.log')
    run([cc,'-Os','-Wall','-Wextra','-Werror','tools/production/platform-start.c','-o',str(out/'y2-platform-start')],'platform-start-build.log')
    run([cc,'-Os','-Wall','-Wextra','-Werror','tools/production/offline-charge.c','-o',str(out/'y2-offline-charge')],'offline-charge-build.log')
    run(['python3','tools/development/audio-fixtures.py',str(out/'audio')],'audio-fixtures.log')
    run(br+['-j12','all'],'buildroot-build.log')
if __name__=='__main__':main()
