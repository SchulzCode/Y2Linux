#!/usr/bin/env python3
"""One owner-operated cable cycle; host reads only, apart from DTR and LOG1."""
import argparse
import datetime
import json
import os
from pathlib import Path
import time
import termios
try:
    from .usb_log_capture import usb_identity, open_verified, receive
except ImportError:
    from usb_log_capture import usb_identity, open_verified, receive

MANUFACTURER='Linux 6.18.0-y2-m2-usbacm4 with musb-hdrc'


def find_y2():
    found=[]
    for device in sorted(Path('/dev').glob('ttyACM*')):
        try:
            identity=usb_identity(device)
        except (OSError,ValueError):
            continue
        if identity.get('manufacturer')==MANUFACTURER:
            found.append((device,identity))
    if len(found)>1:
        raise ValueError('Multiple USBACM-04 devices; refusing ambiguous capture')
    return found[0] if found else None


def run(output):
    output.mkdir(parents=True,exist_ok=False)
    started=time.monotonic_ns()
    summary={'hardware_qualified':False,'started_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),
             'events':[],'status':'waiting','exit_code':2}
    def event(name,**fields):
        summary['events'].append({'event':name,'elapsed_ns':time.monotonic_ns()-started,**fields})
        (output/'robustness.json').write_text(json.dumps(summary,indent=2)+'\n')
    def capture(name,device,identity,seconds,**kwargs):
        folder=output/name;folder.mkdir()
        fd=open_verified(device,identity)
        try:
            identity=dict(identity,dtr_asserted=True)
            event(name+'-open',device=str(device),identity=identity)
            return receive(fd,folder,seconds,1048576,identity,**kwargs)
        finally:
            os.close(fd)
    try:
        # A pre-existing candidate could already be past its safety deadline.
        if find_y2():
            raise ValueError('USBACM-04 already present. Start this reader before the owner boots unplugged.')
        print('Ready. Owner boots USBACM-04 UNPLUGGED, then attaches at its prompt. Restore within 60s of power-on.',flush=True)
        event('waiting-for-fresh-enumeration')
        wait_end=time.monotonic()+90
        found=None
        while time.monotonic()<wait_end and not found:
            found=find_y2()
            if not found: time.sleep(.1)
        if not found: raise TimeoutError('No USBACM-04 on the real host within 90s')
        device,identity=found
        enumerated=time.monotonic();deadline=enumerated+42
        event('enumerated',device=str(device),identity=identity)
        print(f'Identified {device} at {identity["sysfs_path"]}. Leaving tty unopened for 8s.',flush=True)
        while time.monotonic()-enumerated<8: time.sleep(.05)
        first=capture('before-disconnect',device,identity,20,pause_after=3,pause_seconds=6,disconnect_after=12)
        event('first-reader-ended',reason=first['status'])
        kinds=[e['event'] for e in first['events']]
        if not first['protocol_header'] or first['status']!='disconnected' or kinds!=[
            'read-pause-start','read-pause-end','disconnect-prompt']:
            raise ValueError('First phase incomplete; retain evidence and stop this trial')
        disconnected=time.monotonic()
        print('Disconnect observed. After five unplugged seconds, reconnect once; heartbeat must continue.',flush=True)
        reconnect_end=min(deadline,disconnected+10)
        reconnected=None
        while time.monotonic()<reconnect_end:
            match=find_y2()
            if match:
                new_device,new_identity=match
                if new_identity['sysfs_path']!=identity['sysfs_path']:
                    raise ValueError('Reconnect appeared on a different physical USB path')
                if new_identity.get('devnum')!=identity.get('devnum'):
                    reconnected=match;break
            time.sleep(.1)
        if not reconnected: raise TimeoutError('No fresh matching re-enumeration within 10s')
        event('re-enumerated',unplugged_observation_seconds=time.monotonic()-disconnected,
              device=str(reconnected[0]),identity=reconnected[1])
        remaining=min(12,deadline-time.monotonic())
        if remaining<=0: raise TimeoutError('42s host trial budget exhausted')
        second=capture('after-reconnect',*reconnected,remaining)
        if second['exit_code'] or not second['protocol_header']:
            raise ValueError('Reconnect LOG1 capture failed')
        summary.update(status='captured-review-required',exit_code=0)
        # A capture exit is never an automated hardware qualification verdict.
        event('complete-review-required')
        return 0
    except (OSError,ValueError,termios.error) as exc:
        summary.update(status='incomplete',error=str(exc))
        event('failure');print(str(exc),flush=True);return 2
    finally:
        print('Trial ended. Owner restores Android BOOTIMG within the original 60s power-on limit.',flush=True)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args()
    return run(args.output)


if __name__=='__main__':
    raise SystemExit(main())
