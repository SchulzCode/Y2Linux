#!/usr/bin/env python3
"""Wait for the guarded Y2 CDC ACM trial; request LOG1 and retain bounded bytes."""
import argparse
import datetime
import fcntl
import hashlib
import json
import os
from pathlib import Path
import select
import stat
import struct
import termios
import time

HEADER=b'Y2LOG1 M2-USBACM-01\n'


def usb_identity(device, sysfs=Path('/sys')):
    if not device.name.startswith('ttyACM'):
        raise ValueError('Expected a ttyACM device')
    path=(sysfs/'class/tty'/device.name/'device').resolve(strict=True)
    interface=None
    for parent in (path,*path.parents):
        if (parent/'bInterfaceClass').exists():
            interface={k:(parent/k).read_text().strip() for k in
                       ('bInterfaceClass','bInterfaceSubClass','bInterfaceProtocol')}
        if (parent/'idVendor').exists():
            identity={k:(parent/k).read_text().strip() for k in ('idVendor','idProduct')}
            if identity != {'idVendor':'0525','idProduct':'a4a7'} or not interface or \
               (interface['bInterfaceClass'],interface['bInterfaceSubClass'])!=('02','02'):
                raise ValueError('USB identity is not the expected g_serial CDC ACM function')
            for k in ('manufacturer','product','serial','speed','busnum','devnum'):
                if (parent/k).exists(): identity[k]=(parent/k).read_text().strip()
            identity.update(sysfs_path=str(parent),interface=interface)
            identity['descriptors_hex']=(parent/'descriptors').read_bytes().hex()
            return identity
    raise ValueError('No USB device parent')


def receive(fd, output, seconds, max_bytes, identity):
    """fd is opened nonblocking; tests supply only a PTY, never a device."""
    started=time.monotonic_ns()
    report={'status':'capturing','identity':identity,'bytes':0,
            'started_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),
            'seconds':seconds,'max_bytes':max_bytes,'requested_protocol':'LOG1',
            'hardware_qualified':False}
    report_path=output/'capture.json'
    def save():
        temporary=output/'capture.json.tmp'
        temporary.write_text(json.dumps(report,indent=2)+'\n');temporary.replace(report_path)
    original=termios.tcgetattr(fd)
    raw=list(original);raw[6]=list(original[6])
    raw[0]=raw[1]=raw[3]=0
    raw[2]=termios.CS8|termios.CREAD|termios.CLOCAL
    raw[4]=raw[5]=termios.B115200
    raw[6][termios.VMIN]=raw[6][termios.VTIME]=0
    termios.tcsetattr(fd,termios.TCSANOW,raw)
    sha=hashlib.sha256();prefix=bytearray();command=b'LOG1\n';reason='timeout'
    save()
    try:
        with (output/'raw.bin').open('xb') as data, (output/'chunks.jsonl').open('x') as chunks:
            while (time.monotonic_ns()-started)/1e9 < seconds:
                readable,writable,_=select.select([fd],[fd] if command else [],[],0.1)
                if writable:
                    try: command=command[os.write(fd,command):]
                    except BlockingIOError: pass
                if not readable: continue
                try: block=os.read(fd,min(16384,max_bytes-report['bytes']))
                except BlockingIOError: continue
                except OSError as exc:
                    report['read_errno']=exc.errno;reason='disconnected';break
                if not block: reason='disconnected';break
                chunks.write(json.dumps({'elapsed_ns':time.monotonic_ns()-started,
                    'offset':report['bytes'],'bytes':len(block)})+'\n')
                data.write(block);data.flush();sha.update(block)
                if len(prefix)<len(HEADER): prefix.extend(block[:len(HEADER)-len(prefix)])
                report['bytes']+=len(block)
                if report['bytes']>=max_bytes: reason='byte-limit';break
    except OSError as exc:
        report['io_errno']=exc.errno;reason='io-error'
    finally:
        try: termios.tcsetattr(fd,termios.TCSANOW,original)
        except (OSError,termios.error): pass
        report.update(status=reason,sha256=sha.hexdigest(),protocol_header=bytes(prefix)==HEADER,
            command_sent=not command,elapsed_ns=time.monotonic_ns()-started)
        report['exit_code']=0 if report['protocol_header'] and reason in ('timeout','disconnected') else 2
        save()
    return report


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--device',type=Path,help='Optional explicit /dev/ttyACM device')
    parser.add_argument('--output',type=Path,required=True,help='New evidence directory')
    parser.add_argument('--wait-seconds',type=float,default=90)
    parser.add_argument('--seconds',type=float,default=45)
    parser.add_argument('--max-bytes',type=int,default=1048576)
    args=parser.parse_args()
    if not (0<args.wait_seconds<=120 and 0<args.seconds<=60 and 0<args.max_bytes<=4194304):
        parser.error('Wait <=120s, capture <=60s, bytes <=4MiB; all positive')
    args.output.mkdir(parents=True,exist_ok=False)
    print('Waiting for Y2 CDC ACM. Boot UNPLUGGED; attach at the screen prompt.',flush=True)
    deadline=time.monotonic()+args.wait_seconds
    try:
        while time.monotonic()<deadline:
            candidates=[]
            for device in ([args.device] if args.device else sorted(Path('/dev').glob('ttyACM*'))):
                if not device.exists(): continue
                try: identity=usb_identity(device)
                except (ValueError,OSError):
                    if args.device: raise
                    continue
                candidates.append((device,identity))
            if len(candidates)>1: raise ValueError('Multiple matching devices; specify --device')
            if candidates:
                device,identity=candidates[0]
                fd=os.open(device,os.O_RDWR|os.O_NOCTTY|os.O_NONBLOCK|os.O_NOFOLLOW)
                try:
                    st=os.fstat(fd)
                    if not stat.S_ISCHR(st.st_mode) or st.st_rdev!=device.stat().st_rdev:
                        raise ValueError('TTY changed during open')
                    if usb_identity(device)!=identity: raise ValueError('USB changed during open')
                    fcntl.ioctl(fd,termios.TIOCMBIS,struct.pack('I',termios.TIOCM_DTR))
                    identity['dtr_asserted']=True
                    print(f'Connected: {device}; requesting logs.',flush=True)
                    report=receive(fd,args.output,args.seconds,args.max_bytes,identity)
                finally: os.close(fd)
                print(json.dumps(report,indent=2));return report['exit_code']
            time.sleep(.2)
        raise TimeoutError('No matching CDC ACM device appeared')
    except (OSError,ValueError,termios.error) as exc:
        (args.output/'failure.json').write_text(json.dumps({'error':str(exc),
            'type':type(exc).__name__,'hardware_qualified':False},indent=2)+'\n')
        print(str(exc));return 2


if __name__=='__main__': raise SystemExit(main())
