python3 - <<'PY'
# RETIRED for the player: its device side runs Python, and the player has none since test build 169 (docs/planning/PLATFORM-RUST-REFACTOR-PLAN.md §2.6). Kept for its host-side logic and history.
import json,struct,sys,time,subprocess
from pathlib import Path
sys.path.insert(0,'/usr/lib/y2-platform')
from y2_platform.common import Context
from y2_platform.suspend_record import application_rpc
ctx=Context()
def state(label):
    s=application_rpc(ctx,'status')
    bl=open('/sys/class/backlight/y2-backlight/actual_brightness').read().strip()
    sl=subprocess.run(['/usr/sbin/y2-platform','sleep','status'],capture_output=True,text=True).stdout.strip()[:260]
    print(label,'screen_off=',s.get('screen_off'),'playback=',s.get('playback',{}).get('state'),'backlight=',bl); print('   sleep:',sl)
def press(hold=0.025):
    paths=[p for p in Path('/sys/class/input').glob('event*') if (p/'device/name').read_text().strip()=='mtk-pmic-keys']
    with (Path('/dev/input')/paths[0].name).open('wb',buffering=0) as f:
        for v in (1,0):
            f.write(struct.pack('llHHi',0,0,1,116,v)); f.write(struct.pack('llHHi',0,0,0,0,0))
            if v: time.sleep(hold)
import os
def receipt():
    d=json.load(open('/data/system/platform/suspend-last.json')); return d.get('stage'), round(time.time()-d.get('updated',0))
state('initial'); print('receipt',receipt())
press(); time.sleep(3); state('after press A'); print('receipt',receipt())
press(); time.sleep(3); state('after press B'); print('receipt',receipt())
PY
