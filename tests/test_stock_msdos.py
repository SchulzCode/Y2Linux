"""Execute Linux's actual DOS/EBR parser, without emulating its algorithm."""
import re, subprocess, tempfile, unittest
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
class StockParser(unittest.TestCase):
    def test_upstream_parser_and_malformed_tables(self):
        linux=ROOT/'.cache/sources/linux-6.18'
        source=(linux/'block/partitions/msdos.c').read_text()
        header=(linux/'include/linux/msdos_partition.h').read_text()
        strip=lambda s: re.sub(r'^#include .*\n','',s,flags=re.M)
        fixtures=ROOT/'tests/fixtures/production'
        tail=r'''
int main(int argc,char **argv) {
 assert(argc==4);
 for(int i=0;i<3;i++) {FILE *f=fopen(argv[i+1],"rb");assert(f);assert(fread(sectors[i],1,512,f)==512);fclose(f);}
 struct disk disk={0};struct parsed_partitions s={.disk=&disk,.limit=256};
 assert(msdos_partition(&s)==1);
 assert(s.parts[1].from==1024 && s.parts[1].size==2);
 assert(s.parts[2].from==18432 && s.parts[3].from==38912 && s.parts[4].from==125952);
 assert(s.parts[5].from==166912 && s.parts[5].size==1679360);
 assert(s.parts[6].from==1846272 && s.parts[6].size==258048);
 assert(s.parts[7].from==2104320 && s.parts[7].size==1638400);
 /* The block core subsequently clips this oversized legacy FAT, not root/data. */
 assert(s.parts[8].from==3742720 && s.parts[8].size==4291225599ULL);
 sectors[0][510]=0; assert(msdos_partition(&s)==0);sectors[0][510]=0x55;
 sectors[0][446]=1; assert(msdos_partition(&s)==0);sectors[0][446]=0;
 s=(struct parsed_partitions){.disk=&disk,.limit=256};sectors[1][510]=0;
 assert(msdos_partition(&s)==1 && !s.parts[5].size && !s.parts[7].size);
 return 0;
}
'''
        with tempfile.TemporaryDirectory() as d:
            p=Path(d);(p/'test.c').write_text((fixtures/'msdos-harness.h').read_text()+strip(header)+strip(source)+tail)
            subprocess.run(['cc','-Werror=implicit-function-declaration','-I'+str(ROOT/'kernel/platform'),str(p/'test.c'),'-o',str(p/'test')],check=True)
            subprocess.run([str(p/'test'),*[str(fixtures/n) for n in ['MBR','EBR1','EBR2']]],check=True)
