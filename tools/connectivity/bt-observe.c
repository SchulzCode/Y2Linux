// SPDX-License-Identifier: GPL-2.0-only
/* Read-only BlueZ/BlueALSA 5 observation. No transport, radio or codec mutation. */
#include <gio/gio.h>
#include <stdio.h>
#include <string.h>

static GString *out;
static void quoted(const char *text)
{
	if (!text) { g_string_append(out,"null"); return; }
	g_string_append_c(out,'"');
	for (unsigned n=0;text[n] && n<4096;n++) {
		unsigned char c=text[n];
		if (c=='"' || c=='\\') { g_string_append_c(out,'\\'); g_string_append_c(out,c); }
		else if (c<32) g_string_append_printf(out,"\\u%04x",c);
		else g_string_append_c(out,c);
	}
	g_string_append_c(out,'"');
}
static void json(GVariant *value, unsigned depth)
{
	if (!value || depth>8 || out->len>262144) { g_string_append(out,"null"); return; }
	GVariantClass type=g_variant_classify(value);
	switch(type) {
	case G_VARIANT_CLASS_VARIANT: {
		GVariant *child=g_variant_get_variant(value); json(child,depth+1); g_variant_unref(child); break;
	}
	case G_VARIANT_CLASS_STRING: case G_VARIANT_CLASS_OBJECT_PATH: case G_VARIANT_CLASS_SIGNATURE:
		quoted(g_variant_get_string(value,NULL)); break;
	case G_VARIANT_CLASS_BOOLEAN: g_string_append(out,g_variant_get_boolean(value)?"true":"false"); break;
	case G_VARIANT_CLASS_BYTE: g_string_append_printf(out,"%u",g_variant_get_byte(value)); break;
	case G_VARIANT_CLASS_UINT16: g_string_append_printf(out,"%u",g_variant_get_uint16(value)); break;
	case G_VARIANT_CLASS_UINT32: g_string_append_printf(out,"%u",g_variant_get_uint32(value)); break;
	case G_VARIANT_CLASS_INT16: g_string_append_printf(out,"%d",g_variant_get_int16(value)); break;
	case G_VARIANT_CLASS_INT32: g_string_append_printf(out,"%d",g_variant_get_int32(value)); break;
	case G_VARIANT_CLASS_ARRAY: {
		gboolean dict=g_variant_type_is_dict_entry(g_variant_type_element(g_variant_get_type(value)));
		g_string_append_c(out,dict?'{':'[');
		gsize length=MIN(g_variant_n_children(value),256);
		for(gsize i=0;i<length;i++) {
			GVariant *child=g_variant_get_child_value(value,i);
			if(i) g_string_append_c(out,',');
			if(dict) {
				GVariant *key=g_variant_get_child_value(child,0), *item=g_variant_get_child_value(child,1);
				quoted(g_variant_get_string(key,NULL)); g_string_append_c(out,':'); json(item,depth+1);
				g_variant_unref(key); g_variant_unref(item);
			} else json(child,depth+1);
			g_variant_unref(child);
		}
		g_string_append_c(out,dict?'}':']'); break;
	}
	default: g_string_append(out,"null");
	}
}
static GVariant *call(GDBusConnection *bus, const char *owner, const char *path,
	const char *interface, const char *method, GVariant *args, const char *type)
{
	if(!bus || !owner) return NULL;
	GVariant *reply=g_dbus_connection_call_sync(bus,owner,path,interface,method,args,
		G_VARIANT_TYPE(type),G_DBUS_CALL_FLAGS_NO_AUTO_START,350,NULL,NULL);
	if(!reply) return NULL;
	GVariant *value=g_variant_get_child_value(reply,0); g_variant_unref(reply); return value;
}
static char *owner(GDBusConnection *bus,const char *name)
{
	GVariant *value=call(bus,"org.freedesktop.DBus","/org/freedesktop/DBus",
		"org.freedesktop.DBus","GetNameOwner",g_variant_new("(s)",name),"(s)");
	if(!value) return NULL;
	char *name_owner=g_variant_dup_string(value,NULL); g_variant_unref(value); return name_owner;
}
static void field(const char *name,GVariant *value)
{
	quoted(name); g_string_append_c(out,':'); json(value,0);
}
int main(void)
{
	GDBusConnection *bus=g_bus_get_sync(G_BUS_TYPE_SYSTEM,NULL,NULL);
	char *bz=owner(bus,"org.bluez"), *ba=owner(bus,"org.bluealsa");
	GVariant *bluez=call(bus,bz,"/","org.freedesktop.DBus.ObjectManager","GetManagedObjects",NULL,"(a{oa{sa{sv}}})");
	GVariant *bluealsa=call(bus,ba,"/org/bluealsa","org.freedesktop.DBus.ObjectManager","GetManagedObjects",NULL,"(a{oa{sa{sv}}})");
	GVariant *manager=call(bus,ba,"/org/bluealsa","org.freedesktop.DBus.Properties","GetAll",
		g_variant_new("(s)","org.bluealsa.Manager1"),"(a{sv})");
	GVariant *codecs=NULL; char *pcm_path=NULL;
	if(bluealsa) {
		GVariantIter iter; const char *path; GVariant *interfaces;
		g_variant_iter_init(&iter,bluealsa);
		while(g_variant_iter_next(&iter,"{&o@a{sa{sv}}}",&path,&interfaces)) {
			GVariant *props=g_variant_lookup_value(interfaces,"org.bluealsa.PCM1",G_VARIANT_TYPE_VARDICT);
			const char *transport=NULL,*mode=NULL;
			if(props && g_variant_lookup(props,"Transport","&s",&transport) &&
				g_variant_lookup(props,"Mode","&s",&mode) && !strcmp(transport,"A2DP-source") && !strcmp(mode,"sink")) {
				pcm_path=g_strdup(path);
				codecs=call(bus,ba,path,"org.bluealsa.PCM1","GetCodecs",NULL,"(a{sa{sv}})");
			}
			if(props) g_variant_unref(props);
			g_variant_unref(interfaces);
			if(pcm_path) break; /* one bounded additional capability query */
		}
	}
	char *bz_after=owner(bus,"org.bluez"), *ba_after=owner(bus,"org.bluealsa");
	gboolean stable=!g_strcmp0(bz,bz_after) && !g_strcmp0(ba,ba_after);
	out=g_string_new("{\"schema\":1,\"stable_owners\":");
	g_string_append(out,stable?"true":"false");
	g_string_append(out,",\"bluez_owner\":"); quoted(bz);
	g_string_append(out,",\"bluealsa_owner\":"); quoted(ba);
	g_string_append_c(out,','); field("bluez",stable?bluez:NULL);
	g_string_append_c(out,','); field("bluealsa",stable?bluealsa:NULL);
	g_string_append_c(out,','); field("manager",stable?manager:NULL);
	g_string_append(out,",\"codec_pcm\":"); quoted(stable?pcm_path:NULL);
	g_string_append_c(out,','); field("mutually_available_codecs",stable?codecs:NULL);
	g_string_append(out,"}");
	if(out->len>262144) puts("{\"schema\":1,\"error\":\"observation_output_limit\"}"); else puts(out->str);
	g_string_free(out,TRUE);
	if(bluez)g_variant_unref(bluez);
	if(bluealsa)g_variant_unref(bluealsa);
	if(manager)g_variant_unref(manager);
	if(codecs)g_variant_unref(codecs);
	g_free(bz);g_free(ba);g_free(bz_after);g_free(ba_after);g_free(pcm_path);
	if(bus)g_object_unref(bus);
	return 0;
}
