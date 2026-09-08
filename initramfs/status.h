/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_STATUS_H
#define Y2_STATUS_H
#include "../kernel/diagnostic/text.h"
static unsigned text_length(const char *s) { unsigned n=0; while(s[n]) ++n; return n; }
static int starts(const char *s,const char *prefix)
{ while(*prefix) if(*s++!=*prefix++) return 0; return 1; }
static void row_clear(char *row) { unsigned i; for(i=0;i<Y2_COLS;++i) row[i]=' '; }
static unsigned row_add(char *row,unsigned col,const char *s)
{
    while(*s) {
        unsigned char c=(unsigned char)*s++;
        if(col>=Y2_COLS) { row[Y2_COLS-1]='~'; return Y2_COLS; }
        row[col++]=(c>=32 && c<=126) ? c : '?';
    }
    return col;
}
static unsigned row_number(char *row,unsigned col,long value)
{
    char reversed[11],buf[12];unsigned n=0,k=0;
    unsigned v=(unsigned)value;
    if(value<0) { buf[k++]='-';v=0U-v; }
    do { reversed[n++]='0'+v%10U;v/=10U; } while(v);
    while(n) buf[k++]=reversed[--n];
    buf[k]=0;return row_add(row,col,buf);
}
static void row_pair(char *row,const char *key,const char *value)
{ row_clear(row);row_add(row,row_add(row,0,key),value); }
static void row_code(char *row,const char *key,long value)
{ row_clear(row);row_number(row,row_add(row,0,key),value); }
/* Copy just one field, never a full proc dump; input is bounded/NUL-terminated. */
static int field(char *out,unsigned capacity,const char *input,const char *key)
{
    unsigned n=0;
    while(*input) {
        if(starts(input,key)) {
            input+=text_length(key);
            while(*input==' ' || *input=='\t') ++input;
            if(*input!=':') goto next;
            ++input;while(*input==' ' || *input=='\t') ++input;
            while(*input && *input!='\n' && *input!='\r') {
                if(n+1>=capacity) return -75;
                out[n++]=*input++;
            }
            out[n]=0;return n ? 0 : -61;
        }
next:  while(*input && *input!='\n') ++input;
        if(*input) ++input;
    }
    return -61;
}
static int irq_count(char *out,unsigned capacity,const char *input)
{
    while(*input) {
        const char *line=input,*colon=0,*timer=0;
        unsigned n=0;
        while(*input && *input!='\n') {
            if(*input==':') colon=input;
            if(starts(input,"mtk-clkevt")) timer=input;
            ++input;
        }
        if(timer && colon && colon<timer && colon>=line) {
            ++colon;while(*colon==' ' || *colon=='\t') ++colon;
            while(*colon>='0' && *colon<='9') {
                if(n+1>=capacity) return -75;
                out[n++]=*colon++;
            }
            out[n]=0;return n ? 0 : -61;
        }
        if(*input) ++input;
    }
    return -61;
}
static unsigned be32(const unsigned char *s)
{ return (unsigned)s[0]<<24 | (unsigned)s[1]<<16 | (unsigned)s[2]<<8 | s[3]; }
#endif
