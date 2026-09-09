"""Run the production GPIO access functions against bounded synthetic MMIO."""
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class GpioInput(unittest.TestCase):
    def test_banks_direction_and_no_writes(self):
        source = (ROOT/'kernel/gpio/gpio-mt6582-input.c').read_text()
        # Compile the actual callbacks, excluding Linux platform registration.
        callbacks = source[source.index('#define MT6582_GPIO_DIR'):
                           source.index('static int mt6582_input_request')]
        prefix = r'''
#include <assert.h>
#include <stdint.h>
#include <string.h>
#define __iomem
#define BIT(n) (1u << (n))
#define EINVAL 22
#define EBUSY 16
#define GPIO_LINE_DIRECTION_OUT 0
#define GPIO_LINE_DIRECTION_IN 1
struct gpio_chip { void *data; };
#define gpiochip_get_data(chip) ((chip)->data)
static uint32_t mmio[0x1000/4];
static unsigned reads;
static uint32_t readl(void *address) {
    uintptr_t offset = (char *)address - (char *)mmio;
    assert(offset < sizeof(mmio) && !(offset & 3));
    assert(offset < 0xb0 || (offset >= 0x500 && offset < 0x5b0));
    ++reads; return *(uint32_t *)address;
}
'''
        cases = r'''
int main(void) {
    struct mt6582_input_gpio g = {.base = mmio};
    g.chip.data = &g;
    for(unsigned pin=0; pin<169; ++pin) {
        unsigned bank=pin/16*4, bit=pin%16;
        memset(mmio,0,sizeof(mmio));
        assert(mt6582_input_get_direction(&g.chip,pin)==1);
        assert(!mt6582_input_direction_input(&g.chip,pin));
        mmio[0x500/4+bank]=1u<<bit;
        assert(mt6582_input_get(&g.chip,pin)==1);
        mmio[0x500/4+bank]=~(1u<<bit);
        assert(mt6582_input_get(&g.chip,pin)==0);
        mmio[bank]=1u<<bit;
        assert(mt6582_input_get_direction(&g.chip,pin)==0);
        assert(mt6582_input_direction_input(&g.chip,pin)==-16);
        unsigned before=reads;
        assert(mt6582_input_get(&g.chip,pin)==-16 && reads==before+1);
        assert(mmio[bank]==(1u<<bit));
    }
    unsigned before=reads;
    assert(mt6582_input_get(&g.chip,169)==-22);
    assert(mt6582_input_get(&g.chip,~0u)==-22);
    assert(mt6582_input_direction_input(&g.chip,169)==-22);
    assert(reads==before);
}
'''
        with tempfile.TemporaryDirectory() as directory:
            p = Path(directory)
            (p/'gpio.c').write_text(prefix+callbacks+cases)
            subprocess.run(['clang','-O2','-Wall','-Wextra','-Werror',
                            str(p/'gpio.c'),'-o',str(p/'gpio')],check=True)
            subprocess.run([str(p/'gpio')],check=True)
