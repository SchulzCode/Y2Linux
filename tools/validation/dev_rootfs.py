"""Validate built Y2ROOT files and filesystem without mounting or writing a device."""
import argparse, hashlib, json, posixpath, struct, subprocess, tarfile
from pathlib import Path


def require(ok, message):
    if not ok: raise ValueError(message)


def check(root, public_key, rootfs_kernel=None):
    module_root=rootfs_kernel or root
    require(public_key.name.endswith('.pub'), 'explicit .pub key required')
    key=public_key.read_bytes()
    require(key.startswith(b'ssh-ed25519 ') and len(key.splitlines())==1, 'expected one owner ED25519 public key')
    images=root/'buildroot/images'; image=images/'rootfs.ext4'
    def command(*args):
        return subprocess.run(args,check=True,text=True,stdout=subprocess.PIPE,stderr=subprocess.STDOUT).stdout
    fsck=command('e2fsck','-f','-n',str(image))
    identity=command('blkid','-p','-o','export',str(image))
    require('LABEL=Y2ROOT\n' in identity and 'TYPE=ext4\n' in identity and
            'UUID=79324c69-6e75-4801-8000-000000000001\n' in identity,'filesystem identity')
    require(image.stat().st_size==512*1024*1024,'filesystem size')
    with tarfile.open(images/'rootfs.tar') as tar:
        members={m.name.removeprefix('./').rstrip('/'):m for m in tar.getmembers()}
        def entry(name):
            for _ in range(16):
                require(name in members,'missing rootfs path '+name);m=members[name]
                if not m.issym() and not m.islnk():return m
                name=posixpath.normpath(m.linkname.lstrip('/') if m.linkname.startswith('/') or m.islnk()
                                        else posixpath.join(posixpath.dirname(name),m.linkname)).removeprefix('./')
            raise ValueError('rootfs symlink loop')
        def read(name):return tar.extractfile(entry(name)).read()
        for name,m in members.items():
            require((m.uid,m.gid)==((33,33) if name=='var/www' else (0,0)),'unexpected packaged ownership '+name)
            require(not name.startswith('/') and '..' not in name.split('/'),'unsafe tar path')
            if name.startswith('root/.ssh/'):
                require(name=='root/.ssh/authorized_keys','unexpected SSH material '+name)
            if name.startswith('etc/dropbear/'):
                require(False,'host keys must be generated on the Y2, not packaged')
        require(read('root/.ssh/authorized_keys')==key,'authorized public key mismatch')
        require(entry('root/.ssh').mode==0o700 and entry('root/.ssh/authorized_keys').mode==0o600,'SSH permissions')
        require(read('etc/y2linux/build-id')==b'Y2LINUX-M3-AUDIO-01\n','rootfs identity')
        shadow=next(s for s in read('etc/shadow').splitlines() if s.startswith(b'root:'))
        require(shadow.split(b':')[1]==b'$6$y2linux$disabled','unusable password without locked public-key account')
        require(b'-s -g -j -k -p 10.42.0.1:22' in read('etc/default/dropbear'),'key-only SSH policy')
        require(b'LABEL=Y2ROOT / ext4' in read('etc/fstab'),'root mount identity')
        require(read('display.ko')==(module_root/'display.ko').read_bytes(),'rootfs/rescue DRM module mismatch')
        for name in ('y2-observer','y2-fbtest'):
            require(read('usr/sbin/'+name)==(root/name).read_bytes(),'stale rootfs '+name)
        for name in ('bin/busybox','sbin/init','sbin/ip','bin/ps','bin/lsblk','usr/sbin/i2cdetect',
                     'usr/sbin/ethtool','usr/bin/strace','sbin/e2fsck','sbin/fsck.fat','usr/sbin/dropbear',
                     'usr/bin/aplay','usr/bin/amixer','usr/bin/speaker-test','usr/bin/evtest',
                     'sbin/modprobe','usr/bin/memtester','sbin/blkid','sbin/switch_root'):
            raw=read(name)
            require(raw[:6]==b'\x7fELF\x01\x01' and struct.unpack_from('<H',raw,18)[0]==40,'ARM ELF '+name)
            require(struct.unpack_from('<I',raw,36)[0]&0x400,'ARM hard-float ABI '+name)
        import io, wave
        for rate in (44100,48000):
            raw=read(f'usr/share/y2linux/audio/headphone-{rate}.wav')
            with wave.open(io.BytesIO(raw)) as w:
                require(w.getparams()[:4]==(2,2,rate,rate*3),'WAV format/duration')
                samples=struct.unpack('<'+'h'*(w.getnframes()*2),w.readframes(w.getnframes()))
                require(max(map(abs,samples))<=1037,'WAV peak above -30 dBFS')
        require('etc/init.d/S30alsactl' not in members,'unexpected mixer autorestoration')
        for name in ('lib/ld-linux-armhf.so.3','lib/libc.so.6'):require(read(name)[:4]==b'\x7fELF','glibc runtime')
        release=(module_root/'kernel/include/config/kernel.release').read_text().strip()
        require(read('lib/modules/'+release+'/modules.dep').startswith(b'kernel/drivers/gpu/drm/mediatek/mediatek-drm.ko:'),'module index')
        for name in ('S01y2-observer','S20y2-usb','S25y2-pattern','S50dropbear'):
            m=entry('etc/init.d/'+name);require(m.mode&0o111,'non-executable init service '+name)
        # Read the filesystem image independently, so tar checks do not stand in for ext4 contents.
        for name in ('root/.ssh/authorized_keys','etc/shadow','display.ko','usr/sbin/y2-observer','usr/sbin/y2-fbtest','usr/bin/aplay','usr/bin/amixer','usr/share/y2linux/audio/headphone-44100.wav','usr/share/y2linux/audio/headphone-48000.wav'):
            result=subprocess.run(['debugfs','-R','cat /'+name,str(image)],check=True,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
            require(result.stdout==read(name),'ext4/tar disagreement '+name)
    result={'status':'PASS offline; SD boot and SSH login require physical qualification',
            'sd_module_kernel':release,'module_policy':'rescue supplies running kernel module; SD module tree refresh pending' if rootfs_kernel else 'rootfs and rescue modules match',
            'public_key_sha256':hashlib.sha256(key).hexdigest(),'filesystem':identity,'e2fsck':fsck,
            'rootfs_tar_bytes':(images/'rootfs.tar').stat().st_size,'rootfs_ext4_bytes':image.stat().st_size}
    (root/'rootfs-validation.json').write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps(result,indent=2))
    return result

if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('root',type=Path);p.add_argument('--public-key',type=Path,required=True)
    p.add_argument('--rootfs-kernel',type=Path,help='explicit existing rootfs kernel when the new BOOTIMG supplies its module from rescue')
    a=p.parse_args();check(a.root,a.public_key,a.rootfs_kernel)
