// SPDX-License-Identifier: GPL-2.0-only
/* Paint only the kernel-provided fb0 mapping; never map arbitrary physical RAM. */
#include <errno.h>
#include <fcntl.h>
#include <linux/fb.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <unistd.h>
static uint32_t component(unsigned v, struct fb_bitfield f)
{
    return f.length ? ((v >> (8-f.length)) << f.offset) : 0;
}
int main(void)
{
    struct fb_fix_screeninfo fix;
    struct fb_var_screeninfo var;
    unsigned x,y,bytes; uint32_t before=2166136261U,after=2166136261U,expected=2166136261U;
    int fd=open("/dev/fb0",O_RDWR|O_CLOEXEC);
    if(fd<0 || ioctl(fd,FBIOGET_FSCREENINFO,&fix)<0 || ioctl(fd,FBIOGET_VSCREENINFO,&var)<0) {
        perror("Y2PATTERN framebuffer query"); return 1;
    }
    bytes=var.bits_per_pixel/8;
    if(var.xres!=480 || var.yres!=360 || (bytes!=2 && bytes!=4) ||
       var.red.length>8 || var.green.length>8 || var.blue.length>8 || var.transp.length>8 ||
       fix.type!=FB_TYPE_PACKED_PIXELS || fix.visual!=FB_VISUAL_TRUECOLOR ||
       (uint64_t)(var.yoffset+var.yres)*fix.line_length>fix.smem_len ||
       (uint64_t)(var.xoffset+var.xres)*bytes>fix.line_length) {
        fprintf(stderr,"Y2PATTERN refused unsupported framebuffer layout\n");return 2;
    }
    volatile unsigned char *mem=mmap(NULL,fix.smem_len,PROT_READ|PROT_WRITE,MAP_SHARED,fd,0);
    if(mem==MAP_FAILED) {perror("Y2PATTERN mmap");return 1;}
    for(y=0;y<var.yres;y++) for(x=0;x<var.xres;x++) {
        unsigned r=0,g=0,b=0;
        if(y<270) {
            if(x<120) r=g=b=255;
            else if(x<240) r=255;
            else if(x<360) g=255;
            else b=255;
        } else if(((x/20)^(y/20))&1) r=g=b=255;
        uint32_t pixel=component(r,var.red)|component(g,var.green)|component(b,var.blue)|component(255,var.transp);
        size_t at=(y+var.yoffset)*fix.line_length+(x+var.xoffset)*bytes;
        for(unsigned k=0;k<bytes;k++) {before=(before^mem[at+k])*16777619U;mem[at+k]=pixel>>(8*k);expected=(expected^(uint8_t)(pixel>>(8*k)))*16777619U;}
    }
    /* This DMA-backed fb maps the scanout GEM; read back exactly the visible area. */
    for(y=0;y<var.yres;y++) for(x=0;x<var.xres*bytes;x++)
        after=(after^mem[(y+var.yoffset)*fix.line_length+var.xoffset*bytes+x])*16777619U;
    printf("Y2PATTERN white/red/green/blue + checkerboard: %ux%u bpp=%u pitch=%u DMA=%08lx allocation=%u FNV-before=%08x after=%08x expected=%08x match=%s; PHYSICAL VISIBILITY STILL REQUIRES OWNER OBSERVATION\n",
        var.xres,var.yres,var.bits_per_pixel,fix.line_length,fix.smem_start,fix.smem_len,before,after,expected,after==expected?"yes":"NO");
    munmap((void*)mem,fix.smem_len);close(fd);return after==expected?0:3;
}
