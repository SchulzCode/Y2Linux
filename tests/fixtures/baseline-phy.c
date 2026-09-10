/* Host fixture: actual PHY code is inserted at DRIVER. No hardware access. */
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
#include <stdarg.h>
#include <errno.h>
#include <string.h>
typedef uint32_t u32;
#define __iomem
#define BIT(n) (1u << (n))
#define GENMASK(h,l) ((~0u << (l)) & (~0u >> (31-(h))))
#define FIELD_PREP(m,v) (((u32)(v) << __builtin_ctz(m)) & (m))
#define THIS_MODULE NULL
struct clk_hw { void *clk; };
struct phy { void *data; };
struct phy_ops { int (*power_on)(struct phy *); int (*power_off)(struct phy *); void *owner; };
struct clk_ops {
 int (*prepare)(struct clk_hw *); void (*unprepare)(struct clk_hw *);
 long (*round_rate)(struct clk_hw *, unsigned long, unsigned long *);
 int (*set_rate)(struct clk_hw *, unsigned long, unsigned long);
 unsigned long (*recalc_rate)(struct clk_hw *, unsigned long);
};
struct mtk_mipitx_data {
 u32 mppll_preserve; const struct phy_ops *phy_ops; const struct clk_ops *mipi_tx_clk_ops;
 void (*mipi_tx_enable_signal)(struct phy *); void (*mipi_tx_disable_signal)(struct phy *);
};
struct mtk_mipi_tx { void *regs, *dev; u32 data_rate; bool inherited_power; struct clk_hw pll_hw; };
static u32 regs[64];
static unsigned writes, prepares, disables;
static int prepare_fault;
static struct mtk_mipi_tx *mtk_mipi_tx_from_clk_hw(struct clk_hw *h) {
 return (void *)((char *)h - offsetof(struct mtk_mipi_tx, pll_hw));
}
static void *phy_get_drvdata(struct phy *p) { return p->data; }
static u32 readl(void *p) { assert(p != (void *)((char *)regs+0x64)); return *(u32 *)p; }
static void writel(u32 v,void *p) {
 unsigned off=(char *)p-(char *)regs;
 if (off==0x64 || (off==0x50 && (v&1))) {
  assert(regs[0x04/4]&regs[0x08/4]&regs[0x0c/4]&1); /* lanes before PLL/TOP */
 }
 writes++; *(u32 *)p=v;
}
static void mtk_phy_update_bits(void *p,u32 m,u32 v) { writel((readl(p)&~m)|(v&m),p); }
static void mtk_phy_set_bits(void *p,u32 v) { mtk_phy_update_bits(p,v,v); }
static void mtk_phy_clear_bits(void *p,u32 v) { mtk_phy_update_bits(p,v,0); }
static void mdelay(unsigned n) { (void)n; }
static int dev_err_probe(void *d,int e,const char *fmt,...) { return e; }
static void dev_info(void *d,const char *fmt,...) { }
static void dev_dbg(void *d,const char *fmt,...) { }
static int mtk_mipi_tx_pll_prepare(struct clk_hw *hw);
static void mtk_mipi_tx_pll_unprepare(struct clk_hw *hw);
static int mtk_mipi_tx_pll_set_rate(struct clk_hw *h,unsigned long r,unsigned long p) {
 mtk_mipi_tx_from_clk_hw(h)->data_rate=r;return 0;
}
static unsigned long mtk_mipi_tx_pll_recalc_rate(struct clk_hw *h,unsigned long p) {
 return mtk_mipi_tx_from_clk_hw(h)->data_rate;
}
static int clk_set_rate(void *c,unsigned long r) { return mtk_mipi_tx_pll_set_rate(c,r,26000000); }
static int clk_prepare_enable(void *c) { prepares++;return prepare_fault ?: mtk_mipi_tx_pll_prepare(c); }
static void clk_disable_unprepare(void *c) { disables++;mtk_mipi_tx_pll_unprepare(c); }
/* DRIVER */
int main(void) {
 struct mtk_mipi_tx tx={.regs=regs};struct phy phy={.data=&tx};u32 saved[64];
 tx.pll_hw.clk=&tx.pll_hw;
 /* Arbitrary live LK PCW/dividers: no rate assumption, writes or CCF refs. */
 regs[0x50/4]=0x123;regs[0x54/4]=1;regs[0x58/4]=0xdeadbeef;
 regs[0x68/4]=1;regs[0]=3;regs[1]=regs[2]=regs[3]=1;
 memcpy(saved,regs,sizeof(regs));
 assert(!mt6582_phy_power_on(&phy));assert(tx.inherited_power);
 assert(!memcmp(saved,regs,sizeof(regs)) && !writes && !prepares && !tx.data_rate);
 assert(!mt6582_phy_power_off(&phy));assert(!tx.inherited_power && !disables);
 assert(!(regs[0x50/4]&1));
 /* Normal modeset owns a balanced CCF reference and programs the fixed rate. */
 assert(!mt6582_phy_power_on(&phy));assert(prepares==1 && tx.data_rate==338000000);
 assert(regs[0x58/4]==(52u<<24) && regs[0x64/4]==0x300);
 assert(!mt6582_phy_power_off(&phy));assert(disables==1);
 /* Incomplete inherited power is refused without attempting to repair it. */
 regs[0x50/4]=1;regs[0x68/4]=2;writes=0;
 assert(mt6582_phy_power_on(&phy)==-EBUSY && !writes && prepares==1);
 regs[0x68/4]=1;regs[0]=3;regs[1]=regs[2]=1;regs[3]=0;
 assert(mt6582_phy_power_on(&phy)==-EBUSY && !writes);
 /* CCF failure must not enable signals or manufacture a reference. */
 regs[0x50/4]=0;prepare_fault=-EIO;
 assert(mt6582_phy_power_on(&phy)==-EIO && !writes && disables==1);
 /* Direct CCF prepare never retunes a live PLL. */
 regs[0x50/4]=1;
 assert(mtk_mipi_tx_pll_prepare(&tx.pll_hw)==-EBUSY && !writes);
 return 0;
}
