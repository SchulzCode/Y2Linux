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
        const char *line=input,*end=input,*count,*number_end,*token;
        unsigned n=0;
        int matched=0;
        while(*end && *end!='\n') ++end;
        input=*end ? end+1 : end;
        while(line<end && (*line==' ' || *line=='\t')) ++line;
        if(line==end || *line<'0' || *line>'9') continue;
        while(line<end && *line>='0' && *line<='9') ++line;
        if(line==end || *line++!=':') continue;
        while(line<end && (*line==' ' || *line=='\t')) ++line;
        count=number_end=line;
        while(number_end<end && *number_end>='0' && *number_end<='9') ++number_end;
        if(number_end==count || number_end==end ||
           (*number_end!=' ' && *number_end!='\t')) continue;
        token=number_end;
        while(token<end) {
            const char *next;
            while(token<end && (*token==' ' || *token=='\t')) ++token;
            next=token;
            while(next<end && *next!=' ' && *next!='\t') ++next;
            /* timer-of.c passes np->full_name, not the clock-event name. */
            if(token<next && *token=='/') ++token;
            if(next-token==14 && starts(token,"timer@10008000")) matched=1;
            token=next;
        }
        if(matched) {
            while(count<number_end) {
                if(n+1>=capacity) return -75;
                out[n++]=*count++;
            }
            out[n]=0;return 0;
        }
    }
    return -61;
}
static unsigned be32(const unsigned char *s)
{ return (unsigned)s[0]<<24 | (unsigned)s[1]<<16 | (unsigned)s[2]<<8 | s[3]; }
#endif
