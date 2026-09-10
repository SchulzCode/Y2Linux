"""Safety and dependency contract for the integrated M2 DT (not hardware proof)."""
import struct
from tools.validation.formats import fdt
from tools.validation.d08 import require
from tools.validation.dev_memory import RAM, RESERVED
from tools.validation.dtb import cells, strings

# Reviewed controller address/size and GIC SPI/type (MT6582, not sibling IRQs).
REGS = {
 '/clock-controller@10000000': (0x10000000,0x1000,0x10003000,0x1000,0x10001000,0x1000,0x10209000,0x1000),
 '/pinctrl@10005000': (0x10005000,0x1000,0x1000b000,0x1000),
 '/pwrap@1000d000': (0x1000d000,0x1000),
 '/timer@10008000': (0x10008000,0x80),
 '/interrupt-controller@10200100': (0x10200100,0x1c),
 '/interrupt-controller@10211000': (0x10211000,0x1000,0x10212000,0x2000),
 '/serial@11002000': (0x11002000,0x400),
 '/usb@11200000': (0x11200000,0x280,0x11210800,0x70),
 '/i2c@11007000': (0x11007000,0x70,0x11000200,0x80),
 '/i2c@11008000': (0x11008000,0x70,0x11000280,0x80),
 '/keypad@10011000': (0x10011000,0x1000),
 '/mmc@11230000': (0x11230000,0x1000), '/mmc@11240000': (0x11240000,0x1000),
 '/syscon@14000000': (0x14000000,0x1000),
 '/ovl@14007000': (0x14007000,0x1000), '/rdma@14008000': (0x14008000,0x1000),
 '/color@1400b000': (0x1400b000,0x1000), '/mutex@1400e000': (0x1400e000,0x1000),
 '/mipi-tx@10010000': (0x10010000,0x1000), '/dsi@1400c000': (0x1400c000,0x1000),
}
IRQS = {'/timer@10008000':(112,8), '/serial@11002000':(51,8), '/usb@11200000':(32,8),
 '/pinctrl@10005000':(113,4), '/i2c@11007000':(44,8), '/i2c@11008000':(45,8),
 '/keypad@10011000':(116,2), '/mmc@11230000':(39,8), '/mmc@11240000':(40,8),
 '/ovl@14007000':(153,8), '/rdma@14008000':(152,8), '/color@1400b000':(156,8),
 '/mutex@1400e000':(161,8), '/dsi@1400c000':(157,8)}

def check(data, initrd_size):
    nodes,reserved=fdt(data)
    require(tuple(reserved)==RESERVED[:1], 'DEV low boot reservation changed')
    require(nodes['/']['model']==strings('Innioasis Y2 Y2LINUX-DEV-01'),'DEV identity')
    expected={'/reserved-memory/loader@81800000':(0x81800000,0x02800000), '/reserved-memory/high-owned@bdf00000':(0xbdf00000,0x02100000)}
    require({p for p in nodes if p.startswith('/reserved-memory/')}==set(expected),'unreviewed reserved region')
    for path,region in expected.items():
        require(nodes[path]=={'reg':cells(*region),'no-map':b''},'reservation '+path)
    require(nodes['/memory@80000000']['reg']==cells(0x80000000,0x3e000000), 'DEV physical bank changed')
    chosen=nodes['/chosen']
    require(chosen['linux,initrd-start']==cells(0x84000000) and
            chosen['linux,initrd-end']==cells(0x84000000+initrd_size), 'initrd bounds')
    require(chosen['stdout-path']==strings('serial0') and nodes['/aliases']['serial0']==strings('/serial@11002000'), 'UART observation path')
    args=chosen['bootargs'].decode().rstrip('\0')
    require(args=='rdinit=/init earlycon console=ttyS0,921600n8 console=tty0 loglevel=8 ignore_loglevel panic=0 log_buf_len=1M g_cdc.dev_addr=02:42:00:00:00:01 g_cdc.host_addr=02:42:00:00:00:02 g_cdc.iSerialNumber=Y2LINUX-DEV-01', 'baseline command line')
    for path,reg in REGS.items(): require(nodes[path]['reg']==cells(*reg),'MMIO mapping '+path)
    for path,(irq,flags) in IRQS.items(): require(nodes[path]['interrupts']==cells(0,irq,flags),'IRQ mapping '+path)
    require({p for p,v in nodes.items() if p.count('/')==1 and 'reg' in v}==set(REGS)|{'/memory@80000000'},'unreviewed MMIO controller')
    handles={}
    for path,props in nodes.items():
        if 'phandle' in props:
            h=struct.unpack('>I',props['phandle'])[0]
            require(h and h not in handles,'duplicate phandle');handles[h]=(path,props)
        require(not any(k.startswith('regulator-') or k in ('iommus','memory-region','assigned-clock-rates','assigned-clocks') for k in props), 'unreviewed rail/DMA/clock policy')
        if path.startswith('/i2c@11008000/'): raise ValueError('M3/M5 I2C clients activated')
    def handle(path): return struct.unpack('>I',nodes[path]['phandle'])[0]
    for path,props in nodes.items():
        # Validate variable-length clock specifiers, provider arity and clock IDs.
        raw=props.get('clocks',b'');words=struct.unpack('>'+str(len(raw)//4)+'I',raw);i=0
        while i<len(words):
            require(words[i] in handles,'missing clock provider '+path)
            provider,v=handles[words[i]];n=struct.unpack('>I',v['#clock-cells'])[0]
            require(i+1+n<=len(words),'short clock specifier')
            if n: require(n==1 and words[i+1]<(15 if provider.startswith('/clock-controller') else 23),'invalid clock ID')
            i+=1+n
        for prop in ('pinctrl-0','pinctrl-1','backlight','remote-endpoint','interrupt-parent'):
            if prop in props:
                for h in struct.unpack('>'+str(len(props[prop])//4)+'I',props[prop]): require(h in handles,'missing '+prop)
    require(nodes['/cpus']['enable-method']==strings('mediatek,mt6589-smp'),'SMP release method')
    require({p for p in nodes if p.startswith('/cpus/cpu@')}=={'/cpus/cpu@'+str(i) for i in range(4)},'CPU count')
    for i in range(4): require(nodes['/cpus/cpu@'+str(i)]['reg']==cells(i),'CPU index')
    require(nodes['/usb@11200000']['compatible']==strings('innioasis,y2-usb-experiment') and nodes['/usb@11200000']['dr_mode']==strings('peripheral'),'known USB glue/role')
    for path in ('/mmc@11230000','/mmc@11240000'):
        v=nodes[path]
        require(v['bus-width']==cells(1),'SD conservative width')
        if path=='/mmc@11230000':
            require(v['status']==strings('disabled') and v['compatible']==strings('innioasis,y2-msdc-readonly'),'internal eMMC firewall')
        else:
            require(v['compatible']==strings('innioasis,y2-msdc-sd') and v['max-frequency']==cells(13000000),'removable SD contract')
            require('non-removable' not in v and 'no-mmc' in v,'SD removable only')
        require('no-sdio' in v and not any(k.endswith('-supply') for k in v),'MMC rail/radio activation')
    for path in ('/i2c@11007000','/i2c@11008000'):
        require(nodes[path]['clock-div']==cells(16) and nodes[path]['clock-frequency']==cells(100000),'I2C clock contract')
    for path,irq in (('/pwrap@1000d000/pmic',(25,4)),('/i2c@11007000/wheel@51',(55,2))):
        require(nodes[path]['interrupt-parent']==cells(handle('/pinctrl@10005000')) and nodes[path]['interrupts']==cells(*irq),'PMIC/wheel EINT')
    panel=nodes['/dsi@1400c000/panel@0']
    require(panel['resets']==cells(handle('/syscon@14000000'),0) and 'innioasis,lk-powered' in panel,'evidenced panel reset/power')
    return {'node_count':len(nodes),'bootargs':args,'memory':RAM,'storage':'internal eMMC disabled; removable SD writable','evidence':'offline dependencies only'}
