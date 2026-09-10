#!/usr/bin/env python3
"""Retain the validated development deliverables and bounded source metadata."""
import argparse, base64, hashlib, json, shutil, subprocess
from pathlib import Path
PROJECT=Path(__file__).resolve().parents[2]

def digest(path):
    with path.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('root',type=Path);p.add_argument('--candidate',default='Y2LINUX-DEV-01');a=p.parse_args();root=a.root.resolve()
    if not root.is_relative_to(PROJECT/'out'):p.error('output must be inside project/out')
    layout=json.loads((root/'layout.json').read_text());fs=json.loads((root/'rootfs-validation.json').read_text())
    if not fs['status'].startswith('PASS') or layout['policy']!='DEV-01':raise ValueError('validation missing')
    for src,dst in [('kernel/arch/arm/boot/Image','Image'),('kernel/arch/arm/boot/zImage','zImage'),
                    ('kernel/.config','kernel.config'),('buildroot/.config','buildroot.config'),
                    ('buildroot/images/rootfs.ext4','rootfs.ext4'),('buildroot/images/rootfs.tar','rootfs.tar')]:
        shutil.copyfile(root/src,root/dst)
    shutil.copyfile(PROJECT/('docs/build/'+a.candidate.lower()+'-deployment.md'),root/'DEPLOYMENT.md')
    # M3 retains the qualified baseline by Git identity and hashes only its
    # changed source delta, not another complete kernel/donor inventory.
    if a.candidate == 'Y2LINUX-M3-AUDIO-01':
        files=subprocess.check_output(['git','diff','--name-only','d76e57f','--',
            'kernel','initramfs','buildroot','tools','tests'],cwd=PROJECT,text=True).splitlines()
        files+=subprocess.check_output(['git','ls-files','--others','--exclude-standard',
            'kernel','buildroot','tools','tests'],cwd=PROJECT,text=True).splitlines()
    else:
        files=subprocess.check_output(['git','ls-files','--cached','--others','--exclude-standard','-z',
            'kernel','initramfs','buildroot','tools/build','tools/validation','tools/development','tools/observation','tests'],cwd=PROJECT).decode().split('\0')
    sources={name:digest(PROJECT/name) for name in sorted(set(files)) if name and (PROJECT/name).is_file()}
    (root/'source-manifest.json').write_text(json.dumps(sources,indent=2)+'\n')
    metadata={'candidate':a.candidate,'analysis_checkpoint':'2efcdc1',
        'git_head_at_packaging':subprocess.check_output(['git','rev-parse','HEAD'],cwd=PROJECT,text=True).strip(),
        'source_state':'reviewed working tree; exact project source hashes in source-manifest.json',
        'kernel_release':(root/'kernel/include/config/kernel.release').read_text().strip(),
        'buildroot':json.loads((PROJECT/'buildroot/inputs.lock.json').read_text()),
        'public_key_fingerprint':'SHA256:'+base64.b64encode(hashlib.sha256(base64.b64decode((root/'buildroot/target/root/.ssh/authorized_keys').read_bytes().split()[1],validate=True)).digest()).decode().rstrip('='),
        'public_key_file_sha256':fs['public_key_sha256'],
        'ram_before_kernel_bytes':layout['allocator_before_kernel_bytes'],
        'physical_status':'UNTESTED; latest physical result remains BASELINE-03 capture-02',
        'deployment':'owner manual removable-SD partition and BOOTIMG only',
        'validation':'one clean kernel tree, one Buildroot tree; corrections in place; targeted tests, ELF/DT/layout/BOOTIMG, ARM userspace checks, e2fsck and independent ext4/tar content comparison',
        'known_limits':['visible output and wheel recovery unproved','large RAM/SD/ACM+ECM/reconnect/SSH need Y2 qualification','initial SD: 1-bit, at most 13 MHz; root filesystem 512 MiB','boot USB unplugged, then attach; inherited power-state guard retained']}
    if a.candidate=='Y2LINUX-DEV-02':
        metadata.update(analysis_checkpoint='a34a360',physical_status='UNTESTED revision; DEV-01 has visible console, four CPUs/large RAM, SD ext4 mount followed by PID1 SIGILL',deployment='owner BOOTIMG-only; retain existing DEV-01 SD rootfs',validation='one clean revised kernel; reused pinned Buildroot binaries; ARM ABI probe, controller dispatch tests, exact layout/BOOTIMG and unchanged rootfs validation')
        metadata['known_limits']=['root handoff correction requires physical test; no captured original SIGILL PC','wheel WRRD correction requires hardware test','SD module tree has DEV-01; active DRM loads matching DEV-02 module from rescue; refresh SD over SSH later','LOG1 and USB serial retain DEV-01 userspace identity; kernel release identifies DEV-02','boot USB unplugged, then attach; inherited power-state guard retained']
    if a.candidate == 'Y2LINUX-M3-AUDIO-01':
        metadata.update(analysis_checkpoint='d76e57f',
            source_state='qualified DEV-02 Git baseline plus changed-source hashes in source-manifest.json',
            physical_status='M3 audio UNTESTED; DEV-02 core physically qualified, reconnect deferred #27',
            deployment='owner offline SD rootfs content update, then BOOTIMG-only manual flash',
            audio_architecture='ALSA DPCM DL1 -> I05/I06 -> O00/O01 -> I2S_CON3 -> upstream CS43131 -> headphone; amp held low',
            alsa_card='Y2Audio',pcm='hw:CARD=Y2Audio,DEV=0',
            initial_formats=['S16_LE stereo 44100 Hz','S16_LE stereo 48000 Hz'])
        metadata['known_limits']=['M3 playback/analog sequencing/pops/rate accuracy require physical acceptance',
            'speaker identity/output and jack reporting deferred; codec uses ready-status polling',
            'VGP2 retained on at 1.8 V pending shared-rail power qualification',
            'I2S pads retain LK mux; no guessed mux rewrite',
            'USB reconnect remains #27; boot unplugged then attach once',
            'USB serial/observer identity remains DEV-01; kernel and rootfs build-id identify M3']
    names=['BOOTIMG.img','rootfs.ext4','rootfs.tar','kernel.config','buildroot.config','y2.dtb','Image','zImage',
           'zImage-dtb','initramfs.cpio.gz','display.ko','y2-observer','y2-fbtest','layout.json','rescue-manifest.json',
           'rootfs-validation.json','source-manifest.json','DEPLOYMENT.md']
    for name in ('y2-abi-check','sd-module-update.tar'):
        if (root/name).exists():names.append(name)
    metadata['artifacts']={name:{'bytes':(root/name).stat().st_size,'sha256':digest(root/name)} for name in names}
    (root/'result.json').write_text(json.dumps(metadata,indent=2)+'\n');names.append('result.json')
    (root/'SHA256SUMS').write_text(''.join(digest(root/name)+'  '+name+'\n' for name in names))
    print(json.dumps(metadata,indent=2))

if __name__=='__main__':main()
