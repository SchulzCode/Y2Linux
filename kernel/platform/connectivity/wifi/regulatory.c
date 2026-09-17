// SPDX-License-Identifier: GPL-2.0-only
/* cfg80211 is the regulatory authority. The legacy fullmac firmware receives
 * only its permitted HT20 station channels and per-channel power ceilings.
 * Channels requiring passive-only initiation remain excluded in this first
 * station implementation; no country table from another product is used. */
#include "gl_os.h"
#include "precomp.h"
struct y2_regulation { u8 alpha2[2], allowed[14]; s8 power[14]; };
static WLAN_STATUS apply(P_ADAPTER_T adapter, PVOID buffer, UINT_32 size, PUINT_32 used)
{
	const struct y2_regulation *reg=buffer;
	P_GLUE_INFO_T glue=adapter->prGlueInfo;
	P_REG_INFO_T info=&glue->rRegInfo;
	CMD_SET_DOMAIN_INFO_T command={0};
	SET_TXPWR_CTRL_T power={0};
	unsigned group=0, last=0; WLAN_STATUS status;
	*used=sizeof(*reg);
	if (size!=sizeof(*reg)) return WLAN_STATUS_INVALID_LENGTH;
	for (unsigned i=0;i<14;i++) {
		power.acTxPwrLimit2G[i]=reg->power[i];
		if (!reg->allowed[i]) { last=0; continue; }
		if (last && i==last) command.rSubBand[group-1].ucNumChannels++;
		else {
			if (group==MAX_SUBBAND_NUM) return WLAN_STATUS_INVALID_DATA;
			command.rSubBand[group++]=(CMD_SUBBAND_INFO){.ucRegClass=81,.ucBand=BAND_2G4,.ucChannelSpan=1,.ucFirstChannelNum=i+1,.ucNumChannels=1};
		}
		last=i+1;
	}
	if (!group) return WLAN_STATUS_INVALID_DATA;
	command.u2CountryCode=(reg->alpha2[0]<<8)|reg->alpha2[1];
	command.uc2G4Bandwidth=command.uc5GBandwidth=CONFIG_BW_20M;
	status=wlanSendSetQueryCmd(adapter,CMD_ID_SET_DOMAIN_INFO,TRUE,FALSE,FALSE,NULL,NULL,
		sizeof(command),(PUINT_8)&command,NULL,0);
	if (status!=WLAN_STATUS_PENDING) return status;
	info->eRegChannelListMap=REG_CH_MAP_CUSTOMIZED;
	for (unsigned i=0;i<MAX_SUBBAND_NUM;i++) {
        const CMD_SUBBAND_INFO *b=&command.rSubBand[i];
        info->rDomainInfo.rSubBand[i]=(DOMAIN_SUBBAND_INFO){b->ucRegClass,b->ucBand,b->ucChannelSpan,b->ucFirstChannelNum,b->ucNumChannels,0};
    }
	adapter->prDomainInfo=&info->rDomainInfo;
	adapter->rWifiVar.rConnSettings.u2CountryCode=command.u2CountryCode;
	glue->rTxPwr=power;
	return wlanoidSetTxPower(adapter,&power,sizeof(power),used);
}
void y2_wifi_reg_notifier(struct wiphy *wiphy, struct regulatory_request *request)
{
	struct y2_regulation reg={0};
	P_GLUE_INFO_T glue=wiphy_priv(wiphy);
	struct ieee80211_supported_band *band=wiphy->bands[NL80211_BAND_2GHZ];
	UINT_32 used; WLAN_STATUS ret;
	memcpy(reg.alpha2,request->alpha2,2);
	for (unsigned i=0;i<band->n_channels;i++) {
		struct ieee80211_channel *ch=&band->channels[i]; unsigned n=ch->hw_value;
		if (!n || n>13 || (ch->flags & (IEEE80211_CHAN_DISABLED|IEEE80211_CHAN_NO_IR|IEEE80211_CHAN_RADAR))) continue;
		reg.allowed[n-1]=1;
		reg.power[n-1]=clamp_t(int,2*ch->max_power,-64,63);
	}
	ret=kalIoctl(glue,apply,&reg,sizeof(reg),FALSE,FALSE,TRUE,FALSE,&used);
	if (ret!=WLAN_STATUS_SUCCESS) {
		dev_err(glue->rHifInfo.Dev,"Wi-Fi regulatory setup failed: status=%#x\n",ret);
		y2_wifi_error(glue);
	}
}
unsigned y2_wifi_scan_channels(P_ADAPTER_T adapter, P_MSG_SCN_SCAN_REQ scan)
{
	UINT_8 count=0;
	rlmDomainGetChnlList(adapter,BAND_2G4,ARRAY_SIZE(scan->arChnlInfoList),&count,scan->arChnlInfoList);
	scan->eScanChannel=SCAN_CHANNEL_SPECIFIED;
	scan->ucChannelListNum=count;
	return count;
}
