#!/usr/bin/env python3
"""Build an isolated production candidate. No physical devices or deployment."""
import argparse, hashlib, json, os, re, shutil, subprocess
from pathlib import Path
import sys
sys.path.insert(0,str(Path(__file__).resolve().parents[2]))
from tools.production.prepare import buildroot_source, environment
from tools.production.ffmpeg9 import apply as apply_ffmpeg9
from tools.production.application import (source_version as reborn_source_version,
                                         source_version_at_commit)
from tools.production.modernize_buildroot import apply as apply_buildroot_modernization
PROJECT=Path(__file__).resolve().parents[2]
REBORN=PROJECT.parent/'Y2Reborn'
JOBS=str(os.cpu_count() or 4)

def dirty_source(path):
    """Return reviewed source changes, excluding reference-only inputs."""
    reference_only = {
        'docs/architecture/bluetooth-codecs.md',
    }
    status=subprocess.check_output(
        ['git','status','--porcelain=v1'],cwd=path,text=True
    ).splitlines()
    return [
        line for line in status
        if not (
            line.startswith('?? ')
            and (
                line[3:].strip('"') == 'UI Examaples/'
                or line[3:].strip('"') in reference_only
                or line[3:].strip('"').startswith('docs/audit/')
                or line[3:].strip('"').startswith('docs/review/')
            )
        )
    ]

def kernel_release_base(project):
    """VERSION.PATCHLEVEL.SUBLEVEL of the locked 6.18.y source, e.g. 6.18.56."""
    makefile=(project/'.cache/sources/linux-6.18/Makefile').read_text()
    fields={k:re.search(r'^'+k+r' = (\d+)$',makefile,re.M).group(1) for k in ('VERSION','PATCHLEVEL','SUBLEVEL')}
    return '{VERSION}.{PATCHLEVEL}.{SUBLEVEL}'.format(**fields)


# The app's Last.fm key (owner's API account; never in the source). Copied
# 0600 into the build folder for post-build, which installs it as
# /usr/share/reborn/lastfm-api.json; without it the image has no Last.fm.
LASTFM_KEY = Path.home() / '.local/share/y2linux/lastfm-api.json'


def lastfm_key(out, source=None):
    source = LASTFM_KEY if source is None else source
    target = out / 'lastfm-api.json'
    target.unlink(missing_ok=True)
    if not source.is_file():
        print('lastfm: no app key (%s); the image has no Last.fm' % source, flush=True)
        return False
    if source.stat().st_mode & 0o077:
        raise SystemExit('lastfm: %s must be readable by its owner only (chmod 600)' % source)
    value = json.loads(source.read_text())
    if set(value) != {'api_key', 'shared_secret'} or not all(
            isinstance(v, str) and re.fullmatch(r'[0-9a-f]{32}', v) for v in value.values()):
        raise SystemExit('lastfm: %s must hold exactly api_key and shared_secret (32 hex digits)' % source)
    fd = os.open(target, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, 'w') as f:
        json.dump(value, f)
    print('lastfm: app key included', flush=True)
    return True



# The error report receiver's build-wide upload key (tools/reports/README.md:
# made by tools/reports/new_upload_key.py, also set as the Worker's UPLOAD_KEY).
# Installed as /usr/share/y2linux/report-upload.json; without it the image never
# uploads and Reborn offers only Save Report.
REPORT_KEY = Path.home() / '.local/share/y2linux/report-upload.json'


def report_upload_key(out, source=None):
    source = REPORT_KEY if source is None else source
    target = out / 'report-upload.json'
    target.unlink(missing_ok=True)
    if not source.is_file():
        print('reports: no upload key (%s); the image saves reports but never uploads them' % source, flush=True)
        return False
    if source.stat().st_mode & 0o077:
        raise SystemExit('reports: %s must be readable by its owner only (chmod 600)' % source)
    value = json.loads(source.read_text())
    if set(value) != {'upload_key'} or not isinstance(value['upload_key'], str) or \
            not re.fullmatch(r'[A-Za-z0-9_-]{32,128}', value['upload_key']):
        raise SystemExit('reports: %s must hold exactly upload_key (32-128 URL-safe characters)' % source)
    fd = os.open(target, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, 'w') as f:
        json.dump(value, f)
    print('reports: upload key included', flush=True)
    return True

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--output',default='out/y2linux-platform-v1-build')
    p.add_argument('--resume',choices=['kernel','buildroot','artifacts'])
    p.add_argument('--public',action='store_true',help='RELEASE-49 public image: no aptX/aptX HD (owner decision 2026-10-16)')
    p.add_argument('--reuse-userspace',type=Path,help='verified production package whose root/data and rescue binaries are retained')
    p.add_argument('--owner-firmware',type=Path,help='ignored since RELEASE-49 (images carry no radio firmware)')
    a=p.parse_args();out=(PROJECT/a.output).resolve()
    if not out.is_relative_to(PROJECT/'out') or (out.exists() and not a.resume):p.error('fresh directory inside out required')
    out.mkdir(parents=True,exist_ok=True)
    commit=subprocess.check_output(['git','rev-parse','HEAD'],cwd=PROJECT,text=True).strip()
    if dirty_source(PROJECT):p.error('commit the reviewed source before building a release candidate')
    if not REBORN.is_dir() or dirty_source(REBORN):p.error('commit the reviewed Y2Reborn source before building a release candidate')
    reborn_commit=subprocess.check_output(['git','rev-parse','HEAD'],cwd=REBORN,text=True).strip()
    reborn_version=reborn_source_version((REBORN/'Cargo.toml').read_text())
    localversion=re.search(r'^CONFIG_LOCALVERSION="([^"]+)"$',(PROJECT/'kernel/config/production.config').read_text(),re.M).group(1)
    versions={**json.loads((PROJECT/'tools/production/release.json').read_text()),'layout_version':1,'kernel_version':kernel_release_base(PROJECT)+localversion,'data_schema_version':1,'reborn_version':reborn_version,'build_git_commit':commit,'reborn_source_commit':reborn_commit}
    # RELEASE-49: the MediaTek radio firmware never enters an image; each
    # player takes it from its owner's copy (y2_platform.radiofw).
    if a.owner_firmware:
        print('note: --owner-firmware is ignored since RELEASE-49; images carry no radio firmware')
    if a.reuse_userspace:
        from tools.production.validate import validate_manifest
        base=a.reuse_userspace.resolve()
        if not base.is_relative_to(PROJECT/'out'):p.error('userspace package must be inside out')
        previous=validate_manifest(base)
        if previous['minimum_compatible_components']['rootfs_contract']!='y2-platform-v1':p.error('incompatible existing rootfs')
        versions['rootfs_version']=previous['rootfs_version']
        # A reused root keeps its actual build identity and API metadata.
        for field in ('build_id','platform_api_version'):
            versions.pop(field,None)
            if field in previous: versions[field]=previous[field]
        versions['rootfs_build_git_commit']=previous.get('rootfs_build_git_commit', previous['build_git_commit'])
        installed_reborn_version=previous.get('reborn_version', previous.get('application',{}).get('version'))
        installed_reborn_commit=previous.get('reborn_source_commit')
        if not installed_reborn_version and installed_reborn_commit:
            try:
                installed_reborn_version=source_version_at_commit(REBORN, installed_reborn_commit)
            except (ValueError, subprocess.CalledProcessError) as error:
                p.error(f'cannot resolve retained Reborn identity from its recorded source: {error}')
        if not installed_reborn_version:
            p.error('cannot package a retained rootfs without an identifiable Reborn version')
        versions['reborn_version']=installed_reborn_version
        versions['reborn_source_commit']=installed_reborn_commit
    config=out/'buildroot/.config'
    if a.resume and not a.reuse_userspace and config.exists() and 'BR2_PER_PACKAGE_DIRECTORIES=y' not in config.read_text():
        p.error('this build folder predates per-package directories; start a clean build (no --resume)')
    if a.resume in ('buildroot','artifacts') and (not (out/'kernel-source-commit').exists() or (out/'kernel-source-commit').read_text().strip()!=commit):p.error('kernel source commit changed; resume kernel first')
    (out/'versions.json').write_text(json.dumps(versions,indent=2)+'\n')
    environment(PROJECT)
    if not a.reuse_userspace:source,lock=buildroot_source(PROJECT)
    else:lock=json.loads((PROJECT/'buildroot/inputs.lock.json').read_text())
    if not a.reuse_userspace:
        apply_buildroot_modernization(source)
        apply_ffmpeg9(source, PROJECT/'.cache/buildroot-dl')
    lastfm_key(out)
    report_upload_key(out)
    env=os.environ.copy();env.pop('Y2_PUBLIC_KEY_FILE',None)
    env.pop('Y2_OWNER_FIRMWARE',None)
    build_id = versions.get('build_id', 'Y2LINUX-REBORN-PREMIUM-' + versions['rootfs_version'].rsplit('.', 1)[-1].zfill(2))
    env.update(Y2_ARTIFACT_DIR=str(out),BR2_DL_DIR=str(PROJECT/'.cache/buildroot-dl'),SOURCE_DATE_EPOCH=str(lock['source_date_epoch']),E2FSPROGS_FAKE_TIME=str(lock['source_date_epoch']),Y2_BUILD_ID=build_id,LC_ALL='C',TZ='UTC')
    if not shutil.which('bc'):
        env['PATH']=str(PROJECT/'.cache/host-tools')+os.pathsep+env['PATH']
    def run(argv,log):
        print(log,flush=True)
        with (out/log).open('a') as f:subprocess.run(argv,cwd=PROJECT,env=env,stdout=f,stderr=subprocess.STDOUT,check=True)
    builder=['python3','tools/build/run.py','--output',str(out.relative_to(PROJECT)),'--']
    stage=['kernel','buildroot','artifacts'].index(a.resume or 'kernel')
    if stage<=0:
        run(builder+['sh','-c','make -C /src O=/build/kernel KCONFIG_ALLCONFIG=/project/kernel/config/production.config allnoconfig && python3 /project/tools/build/check_config.py /build/kernel/.config /project/kernel/config/production.config && make -C /src O=/build/kernel -j'+JOBS+' Image zImage modules && sh /project/tools/build/dev_observer.sh'],'kernel-build.log')
        (out/'kernel-source-commit').write_text(commit+'\n')
    if stage<=1:
        if a.reuse_userspace:
            run(builder+['python3','/project/tools/production/boot_update.py','--stage-userspace',
                         '/project/'+str(base.relative_to(PROJECT)),'/build'],'userspace-reuse.log')
            from tools.production.rescue_tools import rebuild
            rebuild(PROJECT, out, run)
        else:
            build_userspace(PROJECT,out,source,run,a.public)
    if stage<=2:
        run(builder+['sh','-c','cd /project && python3 -m tools.build.dev_initramfs --production && Y2_DTS=innioasis-y2-production.dts sh tools/build/dtb.sh && Y2_DTS=innioasis-y2-production.dts Y2_DTB_OUT=/build/y2-recovery.dtb Y2_DTS_DEFINES=-DY2_RECOVERY sh tools/build/dtb.sh && python3 -m tools.validation.dev_artifacts /build --package --production'],'artifact-validation.log')
    print('Built production candidate:',out,flush=True)

def build_userspace(PROJECT,out,source,run,public=False):
    br=['make','-C',str(source),'O='+str(out/'buildroot'),'BR2_EXTERNAL='+str(PROJECT/'buildroot')]
    run(br+['y2_production_defconfig'],'buildroot-configure.log')
    if public:
        # Public codec set: SBC, AAC (fdk-aac-free), LDAC; no aptX/aptX HD.
        # Kconfig keeps a library once selected; turn both off explicitly.
        run([str(source/'utils/config'),'--file',str(out/'buildroot/.config'),
             '--disable','BR2_PACKAGE_Y2_CODEC_APTX','--disable','BR2_PACKAGE_LIBFREEAPTX'],'buildroot-public-codecs.log')
        run(br+['olddefconfig'],'buildroot-public-codecs.log')
        config=(out/'buildroot/.config').read_text()
        if 'BR2_PACKAGE_LIBFREEAPTX=y' in config or 'BR2_PACKAGE_Y2_CODEC_APTX=y' in config:
            raise SystemExit('public build: aptX still configured')
    (out/'codec-variant').write_text(('public' if public else 'private')+'\n')
    # Buildroot never removes files of dropped or replaced packages from its
    # target directory (a reused workspace kept libfreeaptx and the full FDK
    # in a public image). With per-package directories the image is assembled
    # afresh from the current packages' own target views, so an empty target
    # suffices; the legacy shared target re-installs every package instead.
    ppd='BR2_PER_PACKAGE_DIRECTORIES=y' in (out/'buildroot/.config').read_text()
    target=out/'buildroot/target'
    if target.exists():
        shutil.rmtree(target)
        if not ppd:
            for stamp in (out/'buildroot/build').glob('*/.stamp_target_installed'):
                stamp.unlink()
    # Buildroot's local-package stamps do not track edits in this repository.
    # Our small native helpers are always rebuilt (under a second each). The
    # three expensive ones (Reborn ≈13 s, bluez-alsa ≈7 s, fdk-aac ≈4 s) are
    # rebuilt only when their recorded inputs changed; the codec packages
    # follow the public/private variant and the AAC source.
    inputs=expensive_package_inputs(PROJECT,out,public)
    stamp=out/'buildroot/.y2-package-inputs.json'
    try: previous=json.loads(stamp.read_text())
    except (OSError,ValueError): previous={}
    stale=[name for name,key in inputs.items() if previous.get(name)!=key or not any((out/'buildroot/build').glob(name+'-*'))]
    stamp.unlink(missing_ok=True) # A failed build below must not look current.
    rebuild=['y2-connectivity','y2-gpu-check','y2-update','y2-mtp',*stale]
    if ppd and (out/'buildroot/build').is_dir():
        # Each package's view holds copies of its dependencies: a rebuilt
        # package must also rebuild everything that depends on it.
        shown=subprocess.run(br+[name+'-show-recursive-rdepends' for name in rebuild],cwd=PROJECT,
                             check=True,capture_output=True,text=True).stdout.split()
        rebuild=sorted(set(rebuild)|{name for name in shown if (out/'buildroot/per-package'/name).exists()})
    run(br+[name+'-dirclean' for name in rebuild],'local-package-rebuild.log')
    with (out/'local-package-rebuild.log').open('a') as f:
        f.write('rebuilt: '+' '.join(rebuild)+'\n')
    run(br+['-j'+JOBS,'toolchain'],'buildroot-toolchain.log')
    # Until host-finalize assembles host/, the cross compiler lives in the
    # toolchain package's own view.
    cc=next((str(path) for path in (out/'buildroot/per-package/toolchain-external-bootlin/host/bin/arm-linux-gcc',
                                    out/'buildroot/host/bin/arm-linux-gcc') if path.exists()),'arm-linux-gcc')
    for name,extra in [('fbtest',[]),('abi-check',['-mcpu=cortex-a7','-mfpu=neon-vfpv4','-mfloat-abi=hard','-marm','-pthread'])]:
        run([cc,'-Os','-Wall','-Wextra','-Werror',*extra,'tools/development/'+name+'.c','-o',str(out/('y2-'+name))],name+'-build.log')
    run([cc,'-Os','-Wall','-Wextra','-Werror','tools/production/usb-status.c','-o',str(out/'y2-usb-status')],'usb-status-build.log')
    run([cc,'-Os','-Wall','-Wextra','-Werror','tools/production/platform-start.c','-o',str(out/'y2-platform-start')],'platform-start-build.log')
    run([cc,'-Os','-Wall','-Wextra','-Werror','tools/production/offline-charge.c','-o',str(out/'y2-offline-charge')],'offline-charge-build.log')
    run(['python3','tools/development/audio-fixtures.py',str(out/'audio')],'audio-fixtures.log')
    run(br+['-j'+JOBS,'all'],'buildroot-build.log')
    run(['python3','tools/production/verify-ffmpeg.py','--buildroot-output',str(out/'buildroot')],
        'ffmpeg-verify.log')
    stamp.write_text(json.dumps(inputs,indent=2,sort_keys=True)+'\n')

def expensive_package_inputs(PROJECT,out,public):
    """Identity of everything the expensive packages are built from."""
    def git(repo,spec):
        return subprocess.check_output(['git','rev-parse',spec],cwd=repo,text=True).strip()
    # Any configuration, pinned input or Buildroot modernisation change
    # rebuilds them all. The git-describe lines change with every commit.
    config=''.join(line for line in (out/'buildroot/.config').read_text().splitlines(True)
                   if not line.startswith(('# Buildroot ','BR2_EXTERNAL_Y2LINUX_VERSION=')))
    common=[hashlib.sha256(config.encode()).hexdigest()]+[
        hashlib.sha256((PROJECT/name).read_bytes()).hexdigest() for name in
        ('buildroot/inputs.lock.json','tools/production/modernize_buildroot.py','tools/production/ffmpeg9.py','tools/production/build.py')]
    variant='public' if public else 'private'
    def key(*parts):
        return hashlib.sha256('\n'.join([*common,*parts]).encode()).hexdigest()
    return {
        # REBORN_BUILD_ID is the Reborn commit; reborn-splash.c and the
        # package's install files live in this repository.
        'reborn':key(git(REBORN,'HEAD'),git(PROJECT,'HEAD:buildroot/package/reborn'),
                     git(PROJECT,'HEAD:tools/graphics/reborn-splash.c')),
        'fdk-aac':key(variant),
        'bluez-alsa':key(variant,git(PROJECT,'HEAD:buildroot/patches/bluez-alsa')),
        # Buildroot's stamps do not see a changed component list (the
        # lowshelf/highshelf filters were never compiled into a resumed
        # workspace); only FFmpeg's own options and pins rebuild it.
        'ffmpeg':hashlib.sha256(''.join(
            [line for line in config.splitlines(True) if line.startswith(('BR2_PACKAGE_FFMPEG','# BR2_PACKAGE_FFMPEG'))]
            +[hashlib.sha256((PROJECT/name).read_bytes()).hexdigest() for name in
              ('buildroot/inputs.lock.json','tools/production/ffmpeg9.py')]).encode()).hexdigest(),
    }
if __name__=='__main__':main()
