/* SPDX-License-Identifier: GPL-3.0-or-later */
#include "buildconf.h"
#include <assert.h>
#include <inttypes.h>
#include <pthread.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/stat.h>
#include <poll.h>
#include <time.h>
#include "mtp.h"
#include "mtp_helpers.h"
#include "mtp_constant.h"
#include "mtp_operations.h"
#include "usb_gadget.h"
#include "mtp_ops_helpers.h"
extern mtp_ctx *mtp_context;
extern usb_gadget *init_usb_mtp_gadget(mtp_ctx *);
extern void deinit_usb_mtp_gadget(usb_gadget *);
extern void fill_if_descriptor(mtp_ctx *, usb_gadget *, struct usb_interface_descriptor *);
extern void fill_ep_descriptor(mtp_ctx *, usb_gadget *, struct usb_endpoint_descriptor_no_audio *, int, unsigned);
extern const unsigned short supported_op[];
extern const int supported_op_size;
#include "hash_utils.h"

/* MoveObject/CopyObject exactly as a host sends them: three parameters. */
static uint32_t transfer(mtp_ctx *ctx, uint16_t code, uint32_t handle, uint32_t storage, uint32_t parent, uint32_t *created)
{
    unsigned char command[32]={0}; MTP_PACKET_HEADER *h=(void *)command;
    h->code=code; h->operation=MTP_CONTAINER_TYPE_COMMAND; h->length=sizeof *h+12;
    memcpy(command+sizeof *h,&handle,4); memcpy(command+sizeof *h+4,&storage,4); memcpy(command+sizeof *h+8,&parent,4);
    int size=sizeof *h+12, params_size=0; uint32_t params[4]={0};
    uint32_t response=code==MTP_OPERATION_MOVE_OBJECT ? mtp_op_MoveObject(ctx,h,&size,params,&params_size)
                                                      : mtp_op_CopyObject(ctx,h,&size,params,&params_size);
    if(created) *created=params_size==4 ? params[0] : 0xFFFFFFFF;
    return response;
}
static void put(const char *path, const char *data, time_t mtime)
{
    int file=open(path,O_WRONLY|O_CREAT|O_EXCL,0600); assert(file>=0);
    assert(write(file,data,strlen(data))==(ssize_t)strlen(data)); close(file);
    struct timespec times[2]={{mtime,0},{mtime,0}}; assert(!utimensat(AT_FDCWD,path,times,0));
}
static void same(const char *path, const char *data, time_t mtime)
{
    char bytes[256]={0}; int file=open(path,O_RDONLY); assert(file>=0);
    assert(read(file,bytes,sizeof bytes-1)==(ssize_t)strlen(data) && !strcmp(bytes,data)); close(file);
    struct stat st; assert(!stat(path,&st) && st.st_mtime==mtime);
}
static fs_entry *child(fs_handles_db *db, uint32_t parent, uint32_t storage, const char *name)
{
    for(fs_entry *e=db->entry_list;e;e=e->next)
        if(e->parent==parent && e->storage_id==storage && e->name && !strcmp(e->name,name) && !(e->flags&ENTRY_IS_DELETED)) return e;
    return NULL;
}
static int leftovers(const char *folder)
{
    char command[4200]; snprintf(command,sizeof command,"find '%s' -name '.y2-mtp-*' | grep -q .",folder);
    return !system(command);
}
/* Link the actual operation/parser/descriptor code with isolated media and a
 * pipe for host OUT packets. No device, gadget bind or raw sector operation. */
int main(int argc, char **argv)
{
    assert(argc==2);
    mtp_ctx ctx={0}; usb_gadget usb={0}; int channel[2];
    assert(!pipe(channel)); mtp_context=&ctx;
    ctx.usb_ctx=&usb; usb.ep_handles[EP_DESCRIPTOR_OUT]=channel[0];
    ctx.uid=getuid(); ctx.euid=geteuid(); ctx.gid=getgid(); ctx.egid=getegid();
    ctx.default_uid=ctx.default_gid=-1;
    ctx.usb_rd_buffer_max_size=8192;
    ctx.rdbuffer2=calloc(1,8192);
    ctx.wrbuffer=calloc(1,8192); ctx.usb_wr_buffer_max_size=8192; ctx.max_packet_size=512;
    ctx.usb_cfg.usb_class=6; ctx.usb_cfg.usb_subclass=1; ctx.usb_cfg.usb_protocol=1;
    struct usb_interface_descriptor interface;
    struct usb_endpoint_descriptor_no_audio endpoint;
    fill_if_descriptor(&ctx,&usb,&interface);
    assert(interface.bInterfaceClass==6 && interface.bNumEndpoints==3);
    fill_ep_descriptor(&ctx,&usb,&endpoint,1,EP_BULK_MODE|EP_IN_DIR|EP_HS_MODE);
    assert(endpoint.wMaxPacketSize==512 && endpoint.bEndpointAddress==0x81);
    fill_ep_descriptor(&ctx,&usb,&endpoint,2,EP_BULK_MODE|EP_OUT_DIR);
    assert(endpoint.wMaxPacketSize==64 && endpoint.bEndpointAddress==2);
    fill_ep_descriptor(&ctx,&usb,&endpoint,3,EP_INT_MODE|EP_IN_DIR);
    assert(endpoint.wMaxPacketSize==16 && endpoint.bInterval>0);
    /* Capture exactly the descriptor block the daemon writes before UDC bind,
     * including Windows MTP compatible-ID descriptors (no live gadget). */
    char ep0[4096]; snprintf(ep0,sizeof ep0,"%s/ep0.fixture",argv[1]);
    int descriptor=open(ep0,O_RDWR|O_CREAT|O_EXCL,0600); assert(descriptor>=0);
    ctx.usb_cfg.usb_functionfs_mode=1;
    snprintf(ctx.usb_cfg.usb_device_path,sizeof ctx.usb_cfg.usb_device_path,"%s",ep0);
    strcpy(ctx.usb_cfg.usb_string_manufacturer,"Y2Linux");
    strcpy(ctx.usb_cfg.usb_string_product,"Y2");
    strcpy(ctx.usb_cfg.usb_string_serial,"Y2LINUX-DEV-01");
    strcpy(ctx.usb_cfg.usb_string_interface,"MTP");
    usb_gadget *captured=init_usb_mtp_gadget(&ctx); assert(captured);
    usb_ffs_cfg emitted; assert(pread(descriptor,&emitted,sizeof emitted,0)==sizeof emitted);
    assert(emitted.magic==FUNCTIONFS_DESCRIPTORS_MAGIC_V2 && emitted.length==sizeof emitted);
    assert((emitted.flags & (FUNCTIONFS_HAS_FS_DESC|FUNCTIONFS_HAS_HS_DESC|FUNCTIONFS_HAS_MS_OS_DESC)) ==
           (FUNCTIONFS_HAS_FS_DESC|FUNCTIONFS_HAS_HS_DESC|FUNCTIONFS_HAS_MS_OS_DESC));
    assert(emitted.fs_count==4 && emitted.hs_count==4 && emitted.os_count==1);
    assert(emitted.os_header.interface==0 && emitted.os_header.bcdVersion==0x0100 && emitted.os_header.wIndex==4);
    assert(emitted.os_header.dwLength==sizeof(struct usb_os_desc_header)+sizeof(struct usb_ext_compat_desc));
    assert(emitted.os_header.bCount==1 && emitted.os_compat.Reserved1==1 && !memcmp(emitted.os_compat.CompatibleID,"MTP",3));
    assert(emitted.ep_desc_hs.ep_desc_int_in.wMaxPacketSize==16 && emitted.ep_desc_hs.if_desc.bInterfaceClass==6);
    deinit_usb_mtp_gadget(captured); close(descriptor); assert(!unlink(ep0));
    ctx.usb_ctx=&usb; ctx.rdbuffer2=calloc(1,8192); ctx.wrbuffer=calloc(1,8192);
    pthread_mutex_init(&ctx.inotify_mutex,NULL);
    assert(!y2_fs_init(argv[1],NULL,NULL));
    uint32_t store=mtp_add_storage(&ctx,argv[1],"Internal Storage",-1,-1,0);
    assert(store && store!=UINT32_MAX);
    ctx.fs_db=init_fs_db(&ctx); assert(ctx.fs_db);
    assert(alloc_root_entry(ctx.fs_db,store));
    for(unsigned iteration=0;iteration<5;iteration++) {
        unsigned char info[256]={0}, command[32]={0}, data[32]={0};
        char name[64], path[4096]; snprintf(name,sizeof name,"copy-%u.flac",iteration);
        snprintf(path,sizeof path,"%s/%s",argv[1],name);
        MTP_PACKET_HEADER *ih=(void *)info, *ch=(void *)command, *dh=(void *)data;
        ih->code=MTP_OPERATION_SEND_OBJECT_INFO; ih->operation=MTP_CONTAINER_TYPE_DATA;
        unsigned char *dataset=info+sizeof *ih;
        dataset[4]=0; dataset[5]=0x30; /* undefined file format */
        dataset[8]=6; dataset[0x34]=strlen(name)+1;
        for(unsigned i=0;i<strlen(name);i++) dataset[0x35+2*i]=name[i];
        int length=sizeof *ih+0x35+2*(strlen(name)+1);
        ih->length=length;
        ch->code=MTP_OPERATION_SEND_OBJECT_INFO; ch->operation=MTP_CONTAINER_TYPE_COMMAND;
        memcpy(command+sizeof *ch,&store,4);
        assert(write(channel[1],info,length)==length);
        int size=sizeof *ch+8, params_size=0; uint32_t params[4]={0};
        assert(mtp_op_SendObjectInfo(&ctx,ch,&size,params,&params_size)==MTP_RESPONSE_OK);
        assert(params_size==12 && params[2]!=UINT32_MAX);
        assert(access(path,F_OK));
        dh->code=MTP_OPERATION_SEND_OBJECT; dh->operation=MTP_CONTAINER_TYPE_DATA;
        memcpy(data+sizeof *dh,"abcdef",6); size=sizeof *dh+6;
        if(iteration==1) size--; /* truncated host copy */
        if(iteration==2) ctx.cancel_req=1;
        if(iteration==3) { deinit_fs_db(ctx.fs_db); ctx.fs_db=NULL; } /* disconnect */
        uint32_t response=mtp_op_SendObject(&ctx,dh,&size,params,&params_size);
        if(iteration==0 || iteration==4) {
            assert(response==MTP_RESPONSE_OK);
            int file=y2_open(path,O_RDONLY,0); char bytes[7]={0}; assert(file>=0);
            assert(read(file,bytes,6)==6 && !strcmp(bytes,"abcdef")); close(file);
            int output[2]; assert(!pipe(output)); usb.ep_handles[EP_DESCRIPTOR_IN]=output[1];
            fs_entry *entry=get_entry_by_handle(ctx.fs_db,params[2]); assert(entry);
            assert(send_file_data(&ctx,entry,0,6)==6);
            unsigned char result[18]={0}; assert(read(output[0],result,sizeof result)==sizeof result);
            assert(!memcmp(result+sizeof(MTP_PACKET_HEADER),"abcdef",6));
            /* Concurrent local truncation is an incomplete transfer, never
             * zero-filled/stale bytes with an OK response. */
            file=open(path,O_TRUNC|O_WRONLY); assert(file>=0); close(file);
            assert(send_file_data(&ctx,entry,0,6)<0);
            close(output[0]); close(output[1]);
        } else assert(access(path,F_OK));
        if(iteration==3) { ctx.fs_db=init_fs_db(&ctx); assert(alloc_root_entry(ctx.fs_db,store)); }
    }
    /* MoveObject and CopyObject (Windows Explorer, libmtp hosts). */
    {
        int move=0, copy=0;
        for(unsigned i=0;i<supported_op_size/sizeof supported_op[0];i++) {
            move|=supported_op[i]==MTP_OPERATION_MOVE_OBJECT; copy|=supported_op[i]==MTP_OPERATION_COPY_OBJECT;
        }
        assert(move && copy);                                  /* announced in DeviceInfo */
        char sd[4096], old[4096], path[4200], other[4200];
        snprintf(sd,sizeof sd,"%s-sd",argv[1]); snprintf(old,sizeof old,"%s-old",argv[1]);
        assert(!mkdir(sd,0700) && !mkdir(old,0700));
        assert(!y2_fs_pin(sd) && !y2_fs_pin(old));
        uint32_t card=mtp_add_storage(&ctx,sd,"SD Card",-1,-1,0);
        uint32_t ro=mtp_add_storage(&ctx,old,"Old Storage",-1,-1,UMTP_STORAGE_READONLY);
        assert(card && card!=UINT32_MAX && ro && ro!=UINT32_MAX);
        assert(alloc_root_entry(ctx.fs_db,card) && alloc_root_entry(ctx.fs_db,ro));
        snprintf(path,sizeof path,"%s/Album",argv[1]); assert(!mkdir(path,0700));
        snprintf(path,sizeof path,"%s/Album/Disc 2",argv[1]); assert(!mkdir(path,0700));
        snprintf(path,sizeof path,"%s/Album/one.flac",argv[1]); put(path,"first song",1600000000);
        snprintf(path,sizeof path,"%s/Album/Disc 2/two.flac",argv[1]); put(path,"second song",1600000100);
        snprintf(path,sizeof path,"%s/three.flac",argv[1]); put(path,"third song",1600000200);
        snprintf(path,sizeof path,"%s/kept.mp3",old); put(path,"old file",1500000000);
        assert(!scan_and_add_folder(ctx.fs_db,argv[1],0,store));
        fs_entry *album=child(ctx.fs_db,0,store,"Album"), *three=child(ctx.fs_db,0,store,"three.flac");
        assert(album && three);
        snprintf(path,sizeof path,"%s/Album",argv[1]); assert(!scan_and_add_folder(ctx.fs_db,path,album->handle,store));
        fs_entry *disc=child(ctx.fs_db,album->handle,store,"Disc 2"), *one=child(ctx.fs_db,album->handle,store,"one.flac");
        assert(disc && one);
        assert(!scan_and_add_folder(ctx.fs_db,old,0,ro));
        fs_entry *kept=child(ctx.fs_db,0,ro,"kept.mp3"); assert(kept);
        uint32_t three_handle=three->handle, album_handle=album->handle, created;

        /* Inside one storage: a rename, the handle stays. 0xFFFFFFFF = root. */
        assert(transfer(&ctx,MTP_OPERATION_MOVE_OBJECT,three_handle,store,album_handle,NULL)==MTP_RESPONSE_OK);
        snprintf(path,sizeof path,"%s/Album/three.flac",argv[1]); same(path,"third song",1600000200);
        snprintf(path,sizeof path,"%s/three.flac",argv[1]); assert(access(path,F_OK));
        assert(get_entry_by_handle(ctx.fs_db,three_handle)->parent==album_handle);
        assert(transfer(&ctx,MTP_OPERATION_MOVE_OBJECT,three_handle,store,album_handle,NULL)==MTP_RESPONSE_OK); /* already there */
        /* Never into itself, never over a name, never from or into read-only storage. */
        assert(transfer(&ctx,MTP_OPERATION_MOVE_OBJECT,album_handle,store,disc->handle,NULL)==MTP_RESPONSE_INVALID_PARENT_OBJECT);
        assert(transfer(&ctx,MTP_OPERATION_MOVE_OBJECT,album_handle,store,album_handle,NULL)==MTP_RESPONSE_INVALID_PARENT_OBJECT);
        assert(transfer(&ctx,MTP_OPERATION_COPY_OBJECT,one->handle,store,album_handle,&created)==MTP_RESPONSE_GENERAL_ERROR);
        assert(transfer(&ctx,MTP_OPERATION_COPY_OBJECT,one->handle,ro,0xFFFFFFFF,&created)==MTP_RESPONSE_STORE_READ_ONLY);
        assert(transfer(&ctx,MTP_OPERATION_MOVE_OBJECT,kept->handle,store,0xFFFFFFFF,NULL)==MTP_RESPONSE_STORE_READ_ONLY);
        snprintf(path,sizeof path,"%s/kept.mp3",old); same(path,"old file",1500000000);
        assert(transfer(&ctx,MTP_OPERATION_MOVE_OBJECT,0x7FFFFFF0,store,0,NULL)==MTP_RESPONSE_INVALID_OBJECT_HANDLE);
        assert(transfer(&ctx,MTP_OPERATION_MOVE_OBJECT,three_handle,0x12345678,0,NULL)==MTP_RESPONSE_INVALID_STORAGE_ID);
        assert(transfer(&ctx,MTP_OPERATION_MOVE_OBJECT,three_handle,store,one->handle,NULL)==MTP_RESPONSE_INVALID_PARENT_OBJECT);

        /* Copy: a new object with the same bytes and time; the source stays. */
        assert(transfer(&ctx,MTP_OPERATION_COPY_OBJECT,one->handle,store,0xFFFFFFFF,&created)==MTP_RESPONSE_OK);
        assert(created!=0xFFFFFFFF && created!=one->handle);
        fs_entry *copied=get_entry_by_handle(ctx.fs_db,created); assert(copied && copied->parent==0 && !strcmp(copied->name,"one.flac"));
        snprintf(path,sizeof path,"%s/one.flac",argv[1]); same(path,"first song",1600000000);
        snprintf(path,sizeof path,"%s/Album/one.flac",argv[1]); same(path,"first song",1600000000);
        /* Copy out of read-only storage is allowed (Old Storage → Internal Storage). */
        assert(transfer(&ctx,MTP_OPERATION_COPY_OBJECT,kept->handle,store,0,&created)==MTP_RESPONSE_OK);
        snprintf(path,sizeof path,"%s/kept.mp3",argv[1]); same(path,"old file",1500000000);
        snprintf(path,sizeof path,"%s/kept.mp3",old); same(path,"old file",1500000000);

        /* A folder to the other storage: copied completely, then the source goes. */
        assert(transfer(&ctx,MTP_OPERATION_MOVE_OBJECT,album_handle,card,0xFFFFFFFF,NULL)==MTP_RESPONSE_OK);
        snprintf(path,sizeof path,"%s/Album/one.flac",sd); same(path,"first song",1600000000);
        snprintf(path,sizeof path,"%s/Album/three.flac",sd); same(path,"third song",1600000200);
        snprintf(path,sizeof path,"%s/Album/Disc 2/two.flac",sd); same(path,"second song",1600000100);
        snprintf(path,sizeof path,"%s/Album",argv[1]); assert(access(path,F_OK));
        fs_entry *moved=get_entry_by_handle(ctx.fs_db,album_handle);
        assert(moved && moved->storage_id==card && moved->parent==0);
        assert(child(ctx.fs_db,album_handle,card,"one.flac") && child(ctx.fs_db,album_handle,card,"Disc 2"));
        assert(!child(ctx.fs_db,album_handle,store,"one.flac"));      /* old handles below it are gone */
        snprintf(path,sizeof path,"%s/Album",sd); snprintf(other,sizeof other,"%s/Album",argv[1]);
        /* And back again, a single file between storages. */
        fs_entry *sd_one=child(ctx.fs_db,album_handle,card,"one.flac");
        assert(transfer(&ctx,MTP_OPERATION_MOVE_OBJECT,sd_one->handle,store,0,NULL)==MTP_RESPONSE_GENERAL_ERROR); /* one.flac exists there */
        assert(transfer(&ctx,MTP_OPERATION_COPY_OBJECT,album_handle,store,0,&created)==MTP_RESPONSE_OK);
        snprintf(path,sizeof path,"%s/Album/Disc 2/two.flac",argv[1]); same(path,"second song",1600000100);
        assert(!leftovers(argv[1]) && !leftovers(sd) && !leftovers(old));
        snprintf(path,sizeof path,"rm -rf '%s' '%s'",sd,old); assert(!system(path));
    }
    /* #61: an unread event never holds up requests. The interrupt endpoint is
     * a pipe whose buffer is full, a host that does not read events (KDE's MTP
     * daemon while it sets up): pushing them under inotify_mutex, as the
     * storage mount/unmount commands do, returns at once and leaves the mutex
     * free; once the host reads, the newest events arrive in order. */
    {
        int events[2]; assert(!pipe(events));
        int flags=fcntl(events[1],F_GETFL); assert(!fcntl(events[1],F_SETFL,flags|O_NONBLOCK));
        static char block[4096]; size_t filled=0; ssize_t n;
        while((n=write(events[1],block,sizeof block))>0) filled+=n;
        while((n=write(events[1],block,1))>0) filled+=n;
        assert(!fcntl(events[1],F_SETFL,flags));
        usb.ep_handles[EP_DESCRIPTOR_INT_IN]=events[1]; usb.stop=0; ctx.cancel_req=0;
        struct timespec start, end; clock_gettime(CLOCK_MONOTONIC,&start);
        assert(!pthread_mutex_lock(&ctx.inotify_mutex));
        for(uint32_t i=0;i<20;i++) assert(mtp_push_event(&ctx,MTP_EVENT_STORE_ADDED,1,&i)==16);
        assert(!pthread_mutex_unlock(&ctx.inotify_mutex));
        clock_gettime(CLOCK_MONOTONIC,&end);
        assert(end.tv_sec-start.tv_sec<1);                       /* nothing waited for the host */
        assert(!pthread_mutex_trylock(&ctx.inotify_mutex)); pthread_mutex_unlock(&ctx.inotify_mutex);
        static char sink[1<<20]; size_t drained=0;
        while(drained<filled) { n=read(events[0],sink,filled-drained); assert(n>0); drained+=n; }
        unsigned char event[16]; uint32_t last=0; int count=0;
        struct pollfd ready={.fd=events[0],.events=POLLIN};
        while(poll(&ready,1,500)>0) {
            assert(read(events[0],event,sizeof event)==sizeof event);
            uint32_t code=event[6]|event[7]<<8, value; memcpy(&value,event+12,4);
            assert(code==MTP_EVENT_STORE_ADDED && (!count || value>last));
            last=value; count++;
        }
        /* The one being written when the host stopped reading, then the newest
         * 16 (the oldest are dropped). */
        assert((count==16 || count==17) && last==19);
        usb.ep_handles[EP_DESCRIPTOR_INT_IN]=-1; close(events[0]);
    }
    deinit_fs_db(ctx.fs_db); free(ctx.rdbuffer); free(ctx.rdbuffer2); free(ctx.wrbuffer); close(channel[0]); close(channel[1]);
    puts("PASS events never block requests (#61), FS/HS descriptors, actual SendObjectInfo/SendObject/download: hidden staging, complete, truncated, cancelled, disconnected, new session, concurrent truncation; MoveObject/CopyObject: rename, between storages, folders, read-only, refusals");
}
