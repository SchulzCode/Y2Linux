/* SPDX-License-Identifier: GPL-2.0-only */
static struct msdc_host host;
static struct mmc_host mmc;
static struct mmc_card card;
static struct mmc_command cmd,sbc,stop;
static struct mmc_data data;
static struct mmc_request mrq;
static unsigned char buffer[4096];
static void setup(unsigned op,unsigned sector,unsigned count,bool use_sbc) {
    memset(registers,0,sizeof(registers));memset(buffer,0,sizeof(buffer));
    ncommands=prepared=started=done=resets=poll_fault=0;
    card=(struct mmc_card){true};mmc=(struct mmc_host){&card,&host};
    host=(struct msdc_host){.y2_emmc=true,.y2_identified=true,.y2_blockaddr=true,
        .y2_sectors=15269888,.mmc=&mmc,.base=registers};
    cmd=(struct mmc_command){.opcode=op,.arg=sector,.flags=MMC_RSP_R1|MMC_CMD_ADTC,.data=&data};
    sbc=(struct mmc_command){.opcode=23,.arg=count,.flags=MMC_RSP_R1};
    stop=(struct mmc_command){.opcode=12,.flags=MMC_RSP_R1B};
    data=(struct mmc_data){.flags=op==24||op==25?MMC_DATA_WRITE:MMC_DATA_READ,
        .blocks=count,.blksz=512,.stop=&stop,.buffer=buffer};
    mrq=(struct mmc_request){.cmd=&cmd,.sbc=use_sbc?&sbc:NULL,
        .stop=op==18||op==25?&stop:NULL,.data=&data};
}
static void response(u32 status) {
    writel(status,registers+SDC_RESP0);
    assert(msdc_cmd_done(&host,MSDC_INT_CMDRDY,&mrq,host.cmd));
}
int main(int argc,char **argv) {
    assert(argc==2);FILE *file=fopen(argv[1],"rb");assert(file);
    assert(fread(factory_mbr,1,512,file)==512);fclose(file);
    /* Sector zero: exact CMD23(8) -> CMD18(0) -> 4096-byte completion. */
    setup(18,0,8,true);msdc_ops_request(&mmc,&mrq);
    assert(ncommands==1&&commands[0]==23&&arguments[0]==8&&!started);
    response(R1_READY_FOR_DATA|(R1_STATE_TRAN<<9));
    assert(ncommands==2&&commands[1]==18&&arguments[1]==0&&!started);
    assert(!(encodings[1]&BIT(29))&&((encodings[1]>>11)&3)==2);
    assert(readl(registers+SDC_BLK_NUM)==8);
    response(R1_READY_FOR_DATA|(R1_STATE_TRAN<<9));
    msdc_data_xfer_done(&host,MSDC_INT_XFER_COMPL,&mrq,&data);
    assert(started==1&&done==1&&!data.error&&data.bytes_xfered==4096);
    assert(buffer[510]==0x55&&buffer[511]==0xaa&&ncommands==2);
    /* SBC flags must survive unchanged, including one-block reliable writes. */
    const unsigned flags[]={0,0x80000000,0x20000000,0xa0000000};
    for(unsigned i=0;i<4;i++) {
        setup(25,166912,1,true);sbc.arg|=flags[i];msdc_ops_request(&mmc,&mrq);
        assert(commands[0]==23&&arguments[0]==(flags[i]|1));response(0);
        assert(commands[1]==25&&arguments[1]==166912);
        assert(((encodings[1]>>11)&3)==2&&(encodings[1]&BIT(13))&&!(encodings[1]&BIT(29)));
        response(0);msdc_data_xfer_done(&host,MSDC_INT_XFER_COMPL,&mrq,&data);
        assert(done==1&&data.bytes_xfered==512&&!data.error);
    }
    /* A rejected SBC cannot issue its read or start DMA. */
    setup(18,0,8,true);msdc_ops_request(&mmc,&mrq);response(R1_ILLEGAL_COMMAND);
    assert(sbc.error==-EIO&&ncommands==1&&!started&&done==1);
    /* Rejected data command, and ready+CRC simultaneously, are failures. */
    setup(18,0,8,true);msdc_ops_request(&mmc,&mrq);response(0);response(R1_ADDRESS_ERROR);
    assert(cmd.error==-EIO&&!started&&done==1);
    setup(18,0,8,true);msdc_ops_request(&mmc,&mrq);
    msdc_cmd_done(&host,MSDC_INT_CMDRDY|MSDC_INT_RSPCRCERR,&mrq,&sbc);
    assert(sbc.error==-EILSEQ&&ncommands==1&&!started&&done==1);
    /* Open-ended multiblock uses CMD12; CMD17 uses single-block encoding. */
    setup(18,1024,8,false);msdc_ops_request(&mmc,&mrq);response(0);
    assert(arguments[0]==1024);msdc_data_xfer_done(&host,MSDC_INT_XFER_COMPL,&mrq,&data);
    assert(ncommands==2&&commands[1]==12&&!done);response(0);assert(done==1);
    setup(17,145408,1,false);msdc_ops_request(&mmc,&mrq);
    assert(arguments[0]==145408&&((encodings[0]>>11)&3)==1);
    /* Every DMA error wins even if XFER_COMPL arrives at the same time. */
    const u32 faults[]={MSDC_INT_DATTMO,MSDC_INT_DATCRCERR,MSDC_INT_DMA_BDCSERR,
        MSDC_INT_DMA_GPDCSERR,MSDC_INT_DMA_PROTECT};
    const int errors[]={-ETIMEDOUT,-EILSEQ,-EIO,-EIO,-EIO};
    for(unsigned i=0;i<5;i++)for(unsigned completed=0;completed<2;completed++) {
        setup(18,0,8,true);msdc_ops_request(&mmc,&mrq);response(0);response(0);
        msdc_data_xfer_done(&host,faults[i]|(completed?MSDC_INT_XFER_COMPL:0),&mrq,&data);
        assert(data.error==errors[i]&&!data.bytes_xfered&&done==1&&resets==1);
    }
    for(unsigned fault=1;fault<=2;fault++) {
        setup(18,0,8,true);msdc_ops_request(&mmc,&mrq);response(0);response(0);poll_fault=fault;
        msdc_data_xfer_done(&host,MSDC_INT_XFER_COMPL,&mrq,&data);
        assert(data.error==-ETIMEDOUT&&!data.bytes_xfered&&done==1);
    }
    /* Forbidden writes must never reach a command or DMA preparation. */
    const unsigned protected[]={0,1024,2048,8192,18432,38912,59392,59648,
        60416,93184,125952,138240,139264,145408,146432,1846272,3742720,15269887};
    for(unsigned i=0;i<sizeof(protected)/sizeof(*protected);i++) {
        setup(25,protected[i],1,true);msdc_ops_request(&mmc,&mrq);
        assert(cmd.error==-EROFS&&done==1&&!ncommands&&!prepared&&!started);
    }
    puts("PASS production MMC sequencing, sector-zero fixture, wire addresses, FUA, stop, faults and protected writes");
}
