/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_TEXT_H
#define Y2_TEXT_H
#include "policy.h"
#define Y2_TEXT_MAGIC 0x59325431U
#define Y2_TEXT_MAJOR 120U /* upstream devices.txt: local/experimental char range */
#define Y2_COLS 40U
#define Y2_ROWS 20U
struct y2_text_frame { unsigned magic; char rows[Y2_ROWS][Y2_COLS]; };
/* Original compact glyphs. Lowercase is deliberately displayed as uppercase. */
static const unsigned char y2_font[64][7] = {
 ['!'-32]={4,4,4,4,4,0,4}, ['"'-32]={10,10,10,0,0,0,0},
 ['#'-32]={10,31,10,10,31,10,0}, ['%'-32]={17,2,4,4,8,17,0},
 ['&'-32]={12,18,20,8,21,18,13}, ['\''-32]={4,4,8,0,0,0,0},
 ['('-32]={2,4,8,8,8,4,2}, [')'-32]={8,4,2,2,2,4,8},
 ['*'-32]={0,21,14,31,14,21,0}, ['+'-32]={0,4,4,31,4,4,0},
 [','-32]={0,0,0,0,4,4,8}, ['-'-32]={0,0,0,31,0,0,0},
 ['.'-32]={0,0,0,0,0,4,4}, ['/'-32]={1,2,2,4,8,8,16},
 ['0'-32]={14,17,19,21,25,17,14}, ['1'-32]={4,12,4,4,4,4,14},
 ['2'-32]={14,17,1,2,4,8,31}, ['3'-32]={30,1,1,14,1,1,30},
 ['4'-32]={2,6,10,18,31,2,2}, ['5'-32]={31,16,16,30,1,1,30},
 ['6'-32]={14,16,16,30,17,17,14}, ['7'-32]={31,1,2,4,8,8,8},
 ['8'-32]={14,17,17,14,17,17,14}, ['9'-32]={14,17,17,15,1,1,14},
 [':'-32]={0,4,4,0,4,4,0}, [';'-32]={0,4,4,0,4,4,8},
 ['<'-32]={1,2,4,8,4,2,1}, ['='-32]={0,0,31,0,31,0,0},
 ['>'-32]={16,8,4,2,4,8,16}, ['?'-32]={14,17,1,2,4,0,4},
 ['@'-32]={14,17,23,21,23,16,14},
 ['A'-32]={14,17,17,31,17,17,17}, ['B'-32]={30,17,17,30,17,17,30},
 ['C'-32]={14,17,16,16,16,17,14}, ['D'-32]={30,17,17,17,17,17,30},
 ['E'-32]={31,16,16,30,16,16,31}, ['F'-32]={31,16,16,30,16,16,16},
 ['G'-32]={14,17,16,23,17,17,15}, ['H'-32]={17,17,17,31,17,17,17},
 ['I'-32]={14,4,4,4,4,4,14}, ['J'-32]={7,2,2,2,18,18,12},
 ['K'-32]={17,18,20,24,20,18,17}, ['L'-32]={16,16,16,16,16,16,31},
 ['M'-32]={17,27,21,21,17,17,17}, ['N'-32]={17,25,21,19,17,17,17},
 ['O'-32]={14,17,17,17,17,17,14}, ['P'-32]={30,17,17,30,16,16,16},
 ['Q'-32]={14,17,17,17,21,18,13}, ['R'-32]={30,17,17,30,20,18,17},
 ['S'-32]={15,16,16,14,1,1,30}, ['T'-32]={31,4,4,4,4,4,4},
 ['U'-32]={17,17,17,17,17,17,14}, ['V'-32]={17,17,17,17,17,10,4},
 ['W'-32]={17,17,17,21,21,27,17}, ['X'-32]={17,17,10,4,10,17,17},
 ['Y'-32]={17,17,10,4,4,4,4}, ['Z'-32]={31,1,2,4,8,16,31},
 ['['-32]={14,8,8,8,8,8,14}, ['\\'-32]={16,8,8,4,2,2,1},
 [']'-32]={14,2,2,2,2,2,14}, ['^'-32]={4,10,17,0,0,0,0},
 ['_'-32]={0,0,0,0,0,0,31},
};
static inline int y2_text_valid(const struct y2_text_frame *f)
{
    unsigned row,col;
    if (f->magic != Y2_TEXT_MAGIC) return 0;
    for(row=0;row<Y2_ROWS;++row) for(col=0;col<Y2_COLS;++col)
        if ((unsigned char)f->rows[row][col]<32 || (unsigned char)f->rows[row][col]>126) return 0;
    return 1;
}
static inline unsigned char y2_glyph(char c, unsigned row)
{
    unsigned char ch=(unsigned char)c;
    if(row>=7) return 0;
    if(ch=='~') return row==2 ? 9 : row==3 ? 22 : 0;
    if(ch>='a' && ch<='z') ch-=32;
    if(ch<32 || ch>95) ch='?';
    return y2_font[ch-32][row];
}
static inline unsigned short y2_text_pixel(const struct y2_text_frame *f, unsigned x, unsigned y)
{
    static const char title[]="Y2 LINUX / M2-PHYWAKE-01";
    static const char guard[]="WDT:STOPPED  FB:GUARD OK";
    unsigned row=y/16, col=x/12, gx=(x%12)/2, gy=(y%16)/2;
    char ch=' ';
    if(x>=Y2_WIDTH || y>=Y2_HEIGHT) return 0;
    if(row==0 && col<sizeof(title)-1) ch=title[col];
    else if(row==1 && col<sizeof(guard)-1) ch=guard[col];
    else if(row>=2 && row<Y2_ROWS+2) ch=f->rows[row-2][col];
    return gx<5 && (y2_glyph(ch,gy)&(1U<<(4-gx))) ? (row==1 ? 0x07e0 : 0xffff) : 0;
}
#endif
