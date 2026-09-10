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

HEADER=b'Y2LOG1 M2-USBACM-04\n'


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


def receive(fd, output, seconds, max_bytes, identity, *, pause_after=None,
            pause_seconds=0, disconnect_after=None, expected_header=HEADER):
    """fd is opened nonblocking; tests supply only a PTY, never a device."""
    started=time.monotonic_ns()
    report={'status':'capturing','identity':identity,'bytes':0,
            'started_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),
            'seconds':seconds,'max_bytes':max_bytes,'requested_protocol':'LOG1',
            'hardware_qualified':False,'events':[]}
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
            paused=False;resumed=False;prompted=False
            while (time.monotonic_ns()-started)/1e9 < seconds:
                elapsed=(time.monotonic_ns()-started)/1e9
                no_read=pause_after is not None and pause_after<=elapsed<pause_after+pause_seconds
                if no_read and not paused:
                    paused=True
                    report['events'].append({'event':'read-pause-start','elapsed_ns':time.monotonic_ns()-started})
                    print('Host reads paused; PID1 must continue.',flush=True);save()
                if paused and not no_read and not resumed:
                    resumed=True
                    report['events'].append({'event':'read-pause-end','elapsed_ns':time.monotonic_ns()-started})
                    print('Host reads resumed.',flush=True);save()
                if disconnect_after is not None and elapsed>=disconnect_after and not prompted:
                    prompted=True
                    report['events'].append({'event':'disconnect-prompt','elapsed_ns':time.monotonic_ns()-started})
                    print('UNPLUG Y2 USB NOW. Keep it unplugged for five seconds, then reconnect.',flush=True);save()
                readable,writable,_=select.select([] if no_read else [fd],[fd] if command else [],[],0.1)
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
                if len(prefix)<len(expected_header): prefix.extend(block[:len(expected_header)-len(prefix)])
                report['bytes']+=len(block)
                if report['bytes']>=max_bytes: reason='byte-limit';break
    except OSError as exc:
        report['io_errno']=exc.errno;reason='io-error'
    finally:
        try: termios.tcsetattr(fd,termios.TCSANOW,original)
        except (OSError,termios.error): pass
        report.update(status=reason,sha256=sha.hexdigest(),protocol_header=bytes(prefix)==expected_header,
            command_sent=not command,elapsed_ns=time.monotonic_ns()-started)
        report['exit_code']=0 if report['protocol_header'] and reason in ('timeout','disconnected') else 2
        save()
    return report


def open_verified(device, identity):
    """Open only the same verified character device and assert ACM DTR."""
    fd=os.open(device,os.O_RDWR|os.O_NOCTTY|os.O_NONBLOCK|os.O_NOFOLLOW)
    try:
        st=os.fstat(fd)
        if not stat.S_ISCHR(st.st_mode) or st.st_rdev!=device.stat().st_rdev:
            raise ValueError('TTY changed during open')
        if usb_identity(device)!=identity: raise ValueError('USB changed during open')
        fcntl.ioctl(fd,termios.TIOCMBIS,struct.pack('I',termios.TIOCM_DTR))
    except BaseException:
        os.close(fd);raise
    return fd


def parse_args(argv=None):
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--device',type=Path,help='Optional explicit /dev/ttyACM device')
    parser.add_argument('--output',type=Path,required=True,help='New evidence directory')
    parser.add_argument('--wait-seconds',type=float,default=90)
    parser.add_argument('--seconds',type=float,help='Capture duration (baseline: 180s; earlier builds: 45s)')
    parser.add_argument('--max-bytes',type=int,default=1048576)
    parser.add_argument('--build',choices=('M2-USBACM-03','M2-USBACM-04','M2-INPUT-01','M2-BASELINE-01','M2-BASELINE-02'),default='M2-USBACM-04')
    args=parser.parse_args(argv)
    baseline=args.build.startswith('M2-BASELINE-')
    if args.seconds is None: args.seconds=180 if baseline else 45
    limit=295 if baseline else 60
    if not (0<args.wait_seconds<=120 and 0<args.seconds<=limit and 0<args.max_bytes<=4194304):
        parser.error(f'Wait <=120s, capture <={limit}s, bytes <=4MiB; all positive')
    return args


def main():
    args=parse_args()
    baseline=args.build.startswith('M2-BASELINE-')
    args.output.mkdir(parents=True,exist_ok=False)
    print('Waiting for Y2 CDC ACM. Boot UNPLUGGED; attach after ten seconds.' if baseline else 'Waiting for Y2 CDC ACM. Boot UNPLUGGED; attach at the screen prompt.',flush=True)
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
                fd=open_verified(device,identity)
                try:
                    identity['dtr_asserted']=True
                    print(f'Connected: {device}; requesting logs.',flush=True)
                    report=receive(fd,args.output,args.seconds,args.max_bytes,identity,
                                   expected_header=('Y2LOG1 '+args.build+'\n').encode('ascii'))
                finally: os.close(fd)
                print(json.dumps(report,indent=2));return report['exit_code']
            time.sleep(.2)
        raise TimeoutError('No matching CDC ACM device appeared')
    except (OSError,ValueError,termios.error) as exc:
        (args.output/'failure.json').write_text(json.dumps({'error':str(exc),
            'type':type(exc).__name__,'hardware_qualified':False},indent=2)+'\n')
        print(str(exc));return 2


if __name__=='__main__': raise SystemExit(main())
