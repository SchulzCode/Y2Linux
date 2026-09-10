#!/usr/bin/env python3
"""Build one fresh DEV-01 candidate. Host files only; no deployment commands."""
import argparse, hashlib, json, os, shlex, shutil, subprocess, tarfile, urllib.request
from pathlib import Path
PROJECT=Path(__file__).resolve().parents[2]


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--output',default='out/y2linux-dev-02')
    p.add_argument('--candidate',default='Y2LINUX-DEV-02')
    p.add_argument('--public-key',type=Path,required=True)
    a=p.parse_args();out=(PROJECT/a.output).resolve();key=a.public_key.absolute()
    if not out.is_relative_to(PROJECT/'out') or (out/'kernel/vmlinux').exists():
        p.error('a fresh output directory below project/out is required')
    if key.suffix!='.pub' or key.is_symlink():p.error('a regular explicit .pub file is required')
    # This command reads the public file only; no SSH directory or private-file discovery.
    subprocess.run(['ssh-keygen','-lf',str(key)],check=True)
    out.mkdir(parents=True,exist_ok=True)
    if not (PROJECT/'.cache/environment/.y2-build-lock').exists():
        subprocess.run(['python3','tools/build/prepare.py'],cwd=PROJECT,check=True)
    lock=json.loads((PROJECT/'buildroot/inputs.lock.json').read_text())
    archive=PROJECT/'.cache/downloads'/('buildroot-'+lock['version']+'.tar.xz')
    if not archive.exists():
        archive.parent.mkdir(parents=True,exist_ok=True)
        temporary=archive.with_suffix('.partial')
        with urllib.request.urlopen(lock['url'],timeout=120) as src,temporary.open('wb') as dst:shutil.copyfileobj(src,dst)
        with temporary.open('rb') as f:
            if hashlib.file_digest(f,'sha256').hexdigest()!=lock['sha256']:raise ValueError('Buildroot download hash')
        temporary.replace(archive)
    with archive.open('rb') as f:
        if hashlib.file_digest(f,'sha256').hexdigest()!=lock['sha256']:raise ValueError('Buildroot archive hash')
    source=PROJECT/'.cache/sources'/('buildroot-'+lock['version'])
    if not source.exists():
        with tarfile.open(archive) as t:t.extractall(source.parent,filter='data')
    env=os.environ.copy();env.update(Y2_PUBLIC_KEY_FILE=str(key),Y2_ARTIFACT_DIR=str(out),
        BR2_DL_DIR=str(PROJECT/'.cache/buildroot-dl'),SOURCE_DATE_EPOCH=str(lock['source_date_epoch']),
        E2FSPROGS_FAKE_TIME=str(lock['source_date_epoch']),LC_ALL='C',TZ='UTC')
    if not shutil.which('bc'):
        hosttools=PROJECT/'.cache/host-tools';hosttools.mkdir(exist_ok=True)
        alpine=PROJECT/'.cache/environment'
        wrapper=hosttools/'bc'
        argv=[str(alpine/'lib/ld-musl-x86_64.so.1'),'--library-path',str(alpine/'usr/lib')+':'+str(alpine/'lib'),str(alpine/'usr/bin/bc')]
        wrapper.write_text('#!/bin/sh\nexec '+shlex.join(argv)+' "$@"\n');wrapper.chmod(0o755)
        env['PATH']=str(hosttools)+os.pathsep+env['PATH']
    def run(command,log):
        print(log,flush=True)
        with (out/log).open('w') as stream:subprocess.run(command,cwd=PROJECT,env=env,check=True,stdout=stream,stderr=subprocess.STDOUT)
    builder=['python3','tools/build/run.py','--output',str(out.relative_to(PROJECT)),'--']
    run(builder+['sh','-c','sh /project/tools/build/configure.sh && make -C /src O=/build/kernel -j12 Image zImage modules && sh /project/tools/build/dev_observer.sh'],'kernel-build.log')
    br=['make','-C',str(source),'O='+str(out/'buildroot'),'BR2_EXTERNAL='+str(PROJECT/'buildroot')]
    run(br+['y2_dev_defconfig'],'buildroot-configure.log')
    run(br+['-j12','toolchain'],'buildroot-toolchain.log')
    run([str(out/'buildroot/host/bin/arm-linux-gcc'),'-Os','-Wall','-Wextra','-Werror',
         'tools/development/fbtest.c','-o',str(out/'y2-fbtest')],'fbtest-build.log')
    run([str(out/'buildroot/host/bin/arm-linux-gcc'),'-mcpu=cortex-a7','-mfpu=neon-vfpv4','-mfloat-abi=hard','-marm','-Os','-Wall','-Wextra','-Werror','-pthread',
         'tools/development/abi-check.c','-o',str(out/'y2-abi-check')],'abi-check-build.log')
    run(br+['-j12','all'],'buildroot-build.log')
    run(builder+['sh','-c','cd /project && python3 -m tools.build.dev_initramfs && sh tools/build/dtb.sh && python3 -m tools.validation.dev_artifacts /build --package'],'artifact-validation.log')
    run(builder+['sh','/project/tools/build/dev_tests.sh'],'tests-subsystems.log')
    run(['python3','tools/validation/dev_rootfs.py',str(out),'--public-key',str(key)],'rootfs-validation.log')
    run(['python3','tools/build/dev_finalize.py',str(out),'--candidate',a.candidate],'finalize.log')
    print('Built and checked:',out,'— owner manual deployment remains pending.')

if __name__=='__main__':main()
