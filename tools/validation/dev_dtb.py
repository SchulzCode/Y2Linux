"""Safety and dependency contract for the integrated Y2 DT (not hardware proof)."""
import struct
from tools.validation.formats import fdt
from tools.validation.d08 import require
from tools.validation.dev_memory import RAM, RESERVED
from tools.validation.dtb import cells, strings

# Reviewed controller address/size and GIC SPI/type (MT6582, not sibling IRQs).
REGS = {
 '/audio-controller@11220000': (0x11220000,0x1000),
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
IRQS = {'/audio-controller@11220000':(104,8), '/timer@10008000':(112,8), '/serial@11002000':(51,8), '/usb@11200000':(32,8),
 '/pinctrl@10005000':(113,4), '/i2c@11007000':(44,8), '/i2c@11008000':(45,8),
 '/keypad@10011000':(116,2), '/mmc@11230000':(39,8), '/mmc@11240000':(40,8),
 '/ovl@14007000':(153,8), '/rdma@14008000':(152,8), '/color@1400b000':(156,8),
 '/mutex@1400e000':(161,8), '/dsi@1400c000':(157,8)}
POWER_REGS = {
 '/clock-controller@10000000': (0x10000000,0x1000,0x10003000,0x1000,0x10001000,0x1000,0x10209000,0x600),
 '/efuse@10206100': (0x10206100,8),
 '/thermal@1100b000': (0x1100b000,0x100,0x11001000,0x100,0x10209600,8),
 '/watchdog@10007000': (0x10007000,0x100),
 '/power-controller@10006000': (0x10006000,0x1000,0x10208000,4),
}
GPU = '/gpu@13010000'
GPU_REGS = {GPU:(0x13010000,0x10000), '/clock-controller@13000000':(0x13000000,0x10)}
CONN = '/connectivity@18070000'
CONN_REGS = {
 '/clock-controller@10000000': (0x10000000,0x1000,0x10003000,0x1000,0x10001000,0x2000,0x10209000,0x600),
 CONN: (0x18070000,0x1000,0xbdf00000,0x100000,
        0x1100c000,0x100,0x11000780,0x80,0x11000800,0x80,
        0x1020a000,0x200,0xbe000000,0x1600000,0xbf600000,0x1c4000,
        0x20050000,4,0x2019379c,4,0x20190000,4,0x20195488,4,0x180f0000,0x5c),
}
CONN_RAILS = ('ldo_vcn18','ldo_vcn28','ldo_vcn33_bt','ldo_vcn33_wifi')

def check(data, initrd_size, production=True):
    nodes,reserved=fdt(data)
    connectivity=CONN in nodes
    gpu=GPU in nodes
    require(tuple(reserved)==RESERVED[:1], 'DEV low boot reservation changed')
    require(nodes['/']['model']==strings('Innioasis Y2'),'DEV identity')
    expected={'/reserved-memory/loader@81800000':(0x81800000,0x02800000), '/reserved-memory/high-owned@bdf00000':(0xbdf00000,0x02100000)}
    require({p for p in nodes if p.startswith('/reserved-memory/')}==set(expected),'unreviewed reserved region')
    for path,region in expected.items():
        props=dict(nodes[path])
        if connectivity and path.endswith('/high-owned@bdf00000'):
            require(len(props.pop('phandle',b''))==4,'owned radio reservation handle')
        require(props=={'reg':cells(*region),'no-map':b''},'reservation '+path)
    require(nodes['/memory@80000000']['reg']==cells(0x80000000,0x3e000000), 'DEV physical bank changed')
    chosen=nodes['/chosen']
    require(chosen['linux,initrd-start']==cells(0x84000000) and
            chosen['linux,initrd-end']==cells(0x84000000+initrd_size), 'initrd bounds')
    require(chosen['stdout-path']==strings('serial0') and nodes['/aliases']['serial0']==strings('/serial@11002000'), 'UART observation path')
    args=chosen['bootargs'].decode().rstrip('\0')
    expected_args='rdinit=/init earlycon console=ttyS0,921600n8 console=tty0 loglevel=3 panic=0 log_buf_len=1M user_debug=31'
    require(args==expected_args, 'profile command line')
    power='/thermal@1100b000' in nodes
    regs=REGS|POWER_REGS if power else REGS
    if connectivity:
        require(power, 'connectivity requires the accepted power platform')
        regs=regs|CONN_REGS
    if gpu:
        require(power and connectivity, 'GPU retains M4/M5 platform')
        regs=regs|GPU_REGS
    for path,reg in regs.items(): require(nodes[path]['reg']==cells(*reg),'MMIO mapping '+path)
    for path,(irq,flags) in IRQS.items(): require(nodes[path]['interrupts']==cells(0,irq,flags),'IRQ mapping '+path)
    require({p for p,v in nodes.items() if p.count('/')==1 and 'reg' in v}==set(regs)|{'/memory@80000000'},'unreviewed MMIO controller')
    handles={}
    for path,props in nodes.items():
        if 'phandle' in props:
            h=struct.unpack('>I',props['phandle'])[0]
            require(h and h not in handles,'duplicate phandle');handles[h]=(path,props)
        require(not any(k in ('iommus','assigned-clock-rates','assigned-clocks') for k in props), 'unreviewed DMA/clock policy')
        require('memory-region' not in props or (connectivity and path==CONN), 'unreviewed memory owner')
        if any(k.startswith('regulator-') for k in props):
            require(path in ('/regulator-dac20','/regulator-dac18','/regulator-dac15',
                            '/pwrap@1000d000/pmic/regulators/ldo_vgp2',
                            '/pwrap@1000d000/pmic/regulators/buck_vproc') or
                    (connectivity and path in tuple('/pwrap@1000d000/pmic/regulators/'+n for n in CONN_RAILS)), 'unreviewed rail')
        if path.startswith('/i2c@11008000/'):
            require(path == '/i2c@11008000/codec@30', 'unreviewed audio/radio I2C client')
    def handle(path): return struct.unpack('>I',nodes[path]['phandle'])[0]
    for path,props in nodes.items():
        # Validate variable-length clock specifiers, provider arity and clock IDs.
        raw=props.get('clocks',b'');words=struct.unpack('>'+str(len(raw)//4)+'I',raw);i=0
        while i<len(words):
            require(words[i] in handles,'missing clock provider '+path)
            provider,v=handles[words[i]];n=struct.unpack('>I',v['#clock-cells'])[0]
            require(i+1+n<=len(words),'short clock specifier')
            if n: require(n==1 and words[i+1]<((25 if gpu else 24 if connectivity else 22 if power else 18) if provider.startswith('/clock-controller') else 23),'invalid clock ID')
            i+=1+n
        for prop in ('pinctrl-0','pinctrl-1','backlight','remote-endpoint','interrupt-parent'):
            if prop in props:
                for h in struct.unpack('>'+str(len(props[prop])//4)+'I',props[prop]): require(h in handles,'missing '+prop)
    if gpu:
        props=nodes[GPU]
        require(props['compatible']==strings('mediatek,mt6582-mali','arm,mali-400'), 'GPU exact binding')
        require(props['interrupts']==cells(*[x for irq in range(170,176) for x in (0,irq,8)]), 'GPU active-low IRQ contract')
        require(props['interrupt-names']==strings('gp','gpmmu','pp0','ppmmu0','pp1','ppmmu1'), 'GPU IRQ ordering')
        require(props['clocks']==cells(handle('/syscon@14000000'),0,handle('/clock-controller@13000000')), 'GPU clock owners')
        require(props['clock-names']==strings('bus','core'), 'Lima bus/core clock naming')
        require(nodes['/clock-controller@13000000']['clocks']==cells(handle('/clock-controller@10000000'),24), 'MFG source owner')
        require(props['power-domains']==cells(handle('/power-controller@10006000')), 'GPU shared SPM domain')
        require(nodes['/power-controller@10006000']['#power-domain-cells']==cells(0), 'MFG genpd arity')
        require(nodes['/power-controller@10006000']['clocks']==cells(handle('/clock-controller@10000000'),24) and
                nodes['/power-controller@10006000']['clock-names']==strings('mfg'), 'MFG source held throughout domain transition')
        require(not any(k in props for k in ('resets','mali-supply','operating-points-v2','dma-coherent','memory-region')), 'no guessed GPU resource')
    require(nodes['/cpus']['enable-method']==strings('innioasis,y2-smp'),'SMP release and hotplug method')
    require({p for p in nodes if p.startswith('/cpus/cpu@')}=={'/cpus/cpu@'+str(i) for i in range(4)},'CPU count')
    for i in range(4): require(nodes['/cpus/cpu@'+str(i)]['reg']==cells(i),'CPU index')
    require(nodes['/usb@11200000']['compatible']==strings('innioasis,y2-usb') and nodes['/usb@11200000']['dr_mode']==strings('peripheral'),'known USB glue/role')
    for path in ('/mmc@11230000','/mmc@11240000'):
        v=nodes[path]
        require(v['bus-width']==cells(1),'SD conservative width')
        if path=='/mmc@11230000':
            require(v['status']==strings('okay') and v['compatible']==strings('innioasis,y2-mmc'),'internal eMMC firewall')
            require(v['max-frequency']==cells(13000000),'internal legacy frequency')
        else:
            require(v['compatible']==strings('innioasis,y2-sd') and v['max-frequency']==cells(13000000),'removable SD contract')
            require('non-removable' not in v and 'no-mmc' in v,'SD removable only')
        require('no-sdio' in v and not any(k.endswith('-supply') for k in v),'MMC rail/radio activation')
    for path in ('/i2c@11007000','/i2c@11008000'):
        require(nodes[path]['clock-div']==cells(16) and nodes[path]['clock-frequency']==cells(100000),'I2C clock contract')
    for path,irq in (('/pwrap@1000d000/pmic',(25,4)),('/i2c@11007000/wheel@51',(55,2))):
        require(nodes[path]['interrupt-parent']==cells(handle('/pinctrl@10005000')) and nodes[path]['interrupts']==cells(*irq),'PMIC/wheel EINT')
    rail='/pwrap@1000d000/pmic/regulators/ldo_vgp2'
    require(nodes[rail]['regulator-min-microvolt']==cells(1800000) and
            nodes[rail]['regulator-max-microvolt']==cells(1800000) and
            'regulator-always-on' in nodes[rail], 'VGP2 1.8V/shared retention')
    parent=rail
    for pin in (20,18,15):
        path='/regulator-dac'+str(pin); v=nodes[path]
        require(v['compatible']==strings('regulator-fixed') and v['gpio']==cells(handle('/pinctrl@10005000'),pin,0), 'DAC enable GPIO')
        require(v['vin-supply']==cells(handle(parent)) and v['startup-delay-us']==cells(50000), 'DAC supply ordering/delay')
        require(v['regulator-min-microvolt']==cells(1800000) and v['regulator-max-microvolt']==cells(1800000), 'nominal DAC rails')
        parent=path
    codec=nodes['/i2c@11008000/codec@30']
    require(codec['compatible']==strings('cirrus,cs43131') and codec['reg']==cells(0x30), 'CS43131 identity')
    for supply in ('VA','VP','VCP'):
        require(codec[supply+'-supply']==cells(handle(rail)), 'DAC analog VGP2 ownership')
    require(codec['VD-supply']==cells(handle('/regulator-dac18')) and codec['VL-supply']==cells(handle('/regulator-dac15')), 'DAC digital controls')
    require(codec['cirrus,xtal-ibias']==cells(2) and 'interrupts' not in codec, 'codec crystal/polling')
    require(nodes['/sound']['mediatek,platform']==cells(handle('/audio-controller@11220000')) and
            nodes['/sound']['audio-codec']==cells(handle('/i2c@11008000/codec@30')), 'ASoC topology')
    require(nodes['/pinctrl@10005000/speaker-disable']['gpios']==cells(8,0) and
            'output-low' in nodes['/pinctrl@10005000/speaker-disable'], 'speaker stays disabled')
    panel=nodes['/dsi@1400c000/panel@0']
    require(panel['resets']==cells(handle('/syscon@14000000'),0) and 'innioasis,lk-powered' in panel,'evidenced panel reset/power')
    if power:
        require(nodes['/power-controller@10006000']['interrupts']==cells(0,117,8), 'actual Y2 SPM IRQ')
        require(nodes['/power-controller@10006000']['compatible']==strings('innioasis,y2-spm'), 'sole SPM owner')
        check_power(nodes,handle)
    if connectivity:
        check_connectivity(nodes,handle)
    return {'node_count':len(nodes),'bootargs':args,'memory':RAM,'storage':'internal eMMC: guarded root/data; removable SD optional','evidence':'offline dependencies only'}

def check_connectivity(nodes, handle):
    p=nodes[CONN]; pmic='/pwrap@1000d000/pmic'
    require(p['compatible']==strings('innioasis,y2-mt6582-connectivity'), 'single connectivity owner')
    require(p['memory-region']==cells(handle('/reserved-memory/high-owned@bdf00000')), 'existing radio exclusion ownership')
    require(p['reg-names']==strings('mcu','conn-emi','btif','tx-dma','rx-dma','ccif','md-rom','md-smem',
                                    'md-wdt','md-key','md-vector','md-enable','wifi'), 'connectivity resource order')
    require(p['interrupts']==cells(*sum(([0,n,8] for n in (50,71,72,185,100,184)),[])), 'actual connectivity IRQs')
    require(p['interrupt-names']==strings('btif','tx-dma','rx-dma','wake','ccif','wifi'), 'connectivity IRQ roles')
    require(p['clocks']==cells(*sum(([handle('/clock-controller@10000000'),n] for n in (22,23,8)),[])) and
            p['clock-names']==strings('connmcu','btif','dma'), 'shared CCF and AP_DMA ownership')
    require(p['resets']==cells(handle('/watchdog@10007000'),12) and p['reset-names']==strings('conn') and
            nodes['/watchdog@10007000']['#reset-cells']==cells(1), 'CONN-scoped reset')
    require(p['innioasis,pwrap']==cells(handle('/pwrap@1000d000')), 'existing PWRAP owner')
    for node,supply,voltage in zip(CONN_RAILS,('vcn18','vcn28','vcn33-bt','vcn33-wifi'),(1800000,2800000,3300000,3300000)):
        path=pmic+'/regulators/'+node; rail=nodes[path]
        require(p[supply+'-supply']==cells(handle(path)), 'connectivity regulator binding')
        require(rail['regulator-min-microvolt']==rail['regulator-max-microvolt']==cells(voltage), 'evidenced connectivity rail voltage')
        require('regulator-always-on' not in rail and 'regulator-boot-on' not in rail, 'radio rails can turn off')

def check_power(nodes, handle):
    pmic='/pwrap@1000d000/pmic'
    require(nodes['/opp-table']['compatible']==strings('operating-points-v2') and 'opp-shared' in nodes['/opp-table'],'shared OPPs')
    rates={598000000,747500000,1040000000}
    require({p for p in nodes if p.startswith('/opp-table/')}=={'/opp-table/opp-'+str(r) for r in rates},'only evidenced CPU OPPs')
    for rate in rates:
        p=nodes['/opp-table/opp-'+str(rate)]
        require(p['opp-hz']==struct.pack('>Q',rate) and p['opp-microvolt']==cells(1150000),'OPP frequency/voltage')
        require(('opp-suspend' in p)==(rate==598000000),'lowest suspend OPP')
    for i in range(4):
        p=nodes['/cpus/cpu@'+str(i)]
        require(p['clocks']==cells(handle('/clock-controller@10000000'),18) and
                p['cpu-supply']==cells(handle(pmic+'/regulators/buck_vproc')) and
                p['operating-points-v2']==cells(handle('/opp-table')),'shared CPU supply/clock/OPPs')
        require('cpu-idle-states' not in p,'architectural WFI only')
    p=nodes[pmic+'/regulators/buck_vproc']
    require(p['regulator-min-microvolt']==p['regulator-max-microvolt']==cells(1150000) and
            'regulator-always-on' in p,'retain inherited VPROC')
    require(nodes[pmic+'/charger']['io-channels']==cells(*sum(([handle(pmic+'/adc'),ch] for ch in (7,5,6,3)),[])), 'charger IIO sources')
    require(nodes[pmic+'/charger']['io-channel-names']==strings('battery-voltage','baton','isense','pmic-temperature'), 'charger channel meanings')
    require(nodes[pmic+'/charger']['power-supplies']==cells(handle('/usb@11200000')), 'negotiated USB input supply')
    require(nodes[pmic+'/backlight']['compatible']==strings('innioasis,y2-backlight') and
            '/pwrap@1000d000/backlight' not in nodes,'backlight MFD ownership')
    require(nodes['/efuse@10206100/calibration@0']['reg']==cells(0,8) and
            'read-only' in nodes['/efuse@10206100'],'own bounded read-only calibration')
    require(nodes['/thermal@1100b000']['nvmem-cells']==cells(handle('/efuse@10206100/calibration@0')),'SoC calibration dependency')
    for zone,sensor,trip,temp in [('cpu-thermal','/thermal@1100b000','cpu-critical',120000),
                                  ('pmic-thermal',pmic+'/adc','pmic-critical',150000)]:
        path='/thermal-zones/'+zone
        require(nodes[path]['thermal-sensors']==cells(handle(sensor)),'thermal sensor ownership')
        require(nodes[path+'/trips/'+trip]['temperature']==cells(temp) and
                nodes[path+'/trips/'+trip]['type']==strings('critical'),'BSP critical temperature')
    require(nodes['/thermal-zones/cpu-thermal/trips/cpu-hot']['temperature']==cells(110000),'BSP passive trip')
    require({p for p,v in nodes.items() if 'wakeup-source' in v}=={pmic+'/keys/power',pmic+'/rtc',pmic+'/charger'},'explicit Power/RTC/CHRDET wake sources')
    require(nodes[pmic+'/rtc']['compatible']==strings('mediatek,mt6323-rtc') and
            nodes[pmic+'/power-controller']['compatible']==strings('mediatek,mt6323-pwrc'),'upstream RTC/poweroff')
