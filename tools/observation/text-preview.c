/* SPDX-License-Identifier: GPL-2.0-only
 * Host-only rendering of an exact D14 frame packet to PPM, never a device.
 */
#include <stdio.h>
#include "../../kernel/diagnostic/text.h"
int main(void)
{
    struct y2_text_frame f;
    if(fread(&f,1,sizeof(f),stdin)!=sizeof(f) || getchar()!=EOF || !y2_text_valid(&f)) return 1;
    printf("P6\n480 360\n255\n");
    for(unsigned y=0;y<Y2_HEIGHT;++y) for(unsigned x=0;x<Y2_WIDTH;++x) {
        unsigned p=y2_text_pixel(&f,x,y);
        putchar(((p>>11)&31)*255/31);
        putchar(((p>>5)&63)*255/63);
        putchar((p&31)*255/31);
    }
    return ferror(stdout) ? 1 : 0;
}
