"""Independent semantic check of every allowed node/property in the first DTB."""
import struct
from tools.validation.formats import fdt
from tools.validation.d08 import RAM, RESERVED, require

BOOTARGS = 'rdinit=/init earlycon console=ttyS0,921600n8 loglevel=8 ignore_loglevel panic=0'


def cells(*values): return struct.pack('>' + 'I' * len(values), *values)
def strings(*values): return b''.join(v.encode() + b'\0' for v in values)


def check(data, r):
    nodes, reserved = fdt(data)
    require(tuple(reserved) == RESERVED, 'D08 FDT permanent reservation mismatch')
    handles = {}
    for path in ['/interrupt-controller@10200100', '/interrupt-controller@10211000',
                 '/clock-system', '/clock-rtc', '/clock-uart']:
        raw = nodes.get(path, {}).get('phandle', b'')
        require(len(raw) == 4, 'missing hardware phandle')
        handles[path] = struct.unpack('>I', raw)[0]
    require(len(set(handles.values())) == len(handles) and all(handles.values()), 'duplicate/zero phandle')
    def ph(path): return handles[path]
    expected = {
      '/': {'model': strings('Innioasis Y2 offline first-boot candidate'),
            'compatible': strings('innioasis,y2', 'mediatek,mt6582'),
            '#address-cells': cells(1), '#size-cells': cells(1),
            'interrupt-parent': cells(ph('/interrupt-controller@10200100'))},
      '/aliases': {'serial0': strings('/serial@11002000')},
      '/chosen': {'stdout-path': strings('serial0'), 'bootargs': strings(BOOTARGS),
                  'linux,initrd-start': cells(0x84000000), 'linux,initrd-end': cells(0x84000000+r)},
      '/memory@80000000': {'device_type': strings('memory'),
                           'reg': cells(*[v for a,b in RAM for v in (a,b-a)])},
      '/cpus': {'#address-cells': cells(1), '#size-cells': cells(0)},
      '/cpus/cpu@0': {'device_type': strings('cpu'), 'compatible': strings('arm,cortex-a7'), 'reg': cells(0)},
      '/timer@10008000': {'compatible': strings('mediatek,mt6577-timer'),
                         'reg': cells(0x10008000,0x80), 'interrupts': cells(0,112,8),
                         'clocks': cells(ph('/clock-system'),ph('/clock-rtc')),
                         'clock-names': strings('system-clk','rtc-clk')},
      '/interrupt-controller@10200100': {'compatible': strings('mediatek,mt6582-sysirq','mediatek,mt6577-sysirq'),
                         'reg': cells(0x10200100,0x1c), 'interrupt-controller': b'',
                         '#interrupt-cells': cells(3), '#address-cells': cells(0),
                         'interrupt-parent': cells(ph('/interrupt-controller@10211000'))},
      '/interrupt-controller@10211000': {'compatible': strings('arm,cortex-a7-gic'),
                         'reg': cells(0x10211000,0x1000,0x10212000,0x2000), 'interrupt-controller': b'',
                         '#interrupt-cells': cells(3), '#address-cells': cells(0),
                         'interrupt-parent': cells(ph('/interrupt-controller@10211000'))},
      '/serial@11002000': {'compatible': strings('mediatek,mt6582-uart','mediatek,mt6577-uart'),
                         'reg': cells(0x11002000,0x400), 'interrupts': cells(0,51,8),
                         'clocks': cells(ph('/clock-uart'))},
    }
    for path, rate in [('/clock-system',13000000),('/clock-rtc',32000),('/clock-uart',26000000)]:
        expected[path] = {'compatible': strings('fixed-clock'), '#clock-cells': cells(0), 'clock-frequency': cells(rate)}
    for path, handle in handles.items(): expected[path]['phandle'] = cells(handle)
    require(set(nodes) == set(expected), 'unexpected/missing hardware or memory node')
    for path, props in expected.items():
        require(nodes[path] == props, 'DT properties differ from reviewed policy: ' + path)
    return {'node_count': len(nodes), 'bootargs': BOOTARGS, 'stdout_path': 'serial0', 'memory': RAM}
