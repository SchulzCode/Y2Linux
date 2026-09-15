// SPDX-License-Identifier: GPL-2.0-only
/* Bounded qualification client, not Y2PlayerNative. Standard ALSA BlueALSA PCM
 * plus BlueZ's MPRIS player registration for AVRCP controls and metadata. */
#define _GNU_SOURCE
#include <alsa/asoundlib.h>
#include <gio/gio.h>
#include <glib-unix.h>
#include <fcntl.h>
#include <sys/file.h>
#include <sys/stat.h>
#include <unistd.h>
#include "../../kernel/platform/connectivity/protocol.h"
#define PLAYER "/org/y2linux/qualification/player"
#define IFACE "org.mpris.MediaPlayer2.Player"
static GMainLoop *loop;
static GDBusConnection *bus;
static GMutex guard;
static gint playing=1, stopping, failed, wrote_pcm;
static guint rate, tracks=1, next_track;
static guint64 position, frames;
static gint16 *pcm;
static char device[96];
static int lease=-1;
static const char xml[]=
"<node><interface name='org.mpris.MediaPlayer2.Player'>"
"<method name='Play'/><method name='Pause'/><method name='PlayPause'/><method name='Stop'/>"
"<method name='Next'/><method name='Previous'/>"
"<property name='PlaybackStatus' type='s' access='read'/>"
"<property name='Metadata' type='a{sv}' access='read'/>"
"<property name='Position' type='x' access='read'/>"
"<property name='CanPlay' type='b' access='read'/><property name='CanPause' type='b' access='read'/>"
"<property name='CanGoNext' type='b' access='read'/><property name='CanGoPrevious' type='b' access='read'/>"
"<property name='CanControl' type='b' access='read'/><property name='CanSeek' type='b' access='read'/>"
"</interface></node>";
static GVariant *metadata(void)
{
	GVariantBuilder b; g_variant_builder_init(&b,G_VARIANT_TYPE_VARDICT);
	g_variant_builder_add(&b,"{sv}","mpris:trackid",g_variant_new_object_path("/org/y2linux/qualification/tone"));
	g_variant_builder_add(&b,"{sv}","mpris:length",g_variant_new_int64(frames*1000000/rate));
	g_variant_builder_add(&b,"{sv}","xesam:title",g_variant_new_string("Y2 connectivity qualification"));
	const char *artists[]={"Y2Linux",NULL};
	g_variant_builder_add(&b,"{sv}","xesam:artist",g_variant_new_strv(artists,-1));
	g_variant_builder_add(&b,"{sv}","xesam:album",g_variant_new_string("Generated low-level audio fixture"));
	g_variant_builder_add(&b,"{sv}","xesam:trackNumber",g_variant_new_int32(tracks));
	return g_variant_builder_end(&b);
}
static GVariant *property(GDBusConnection *c,const gchar *s,const gchar *p,const gchar *i,
			  const gchar *name,GError **error,gpointer data)
{
	(void)c;(void)s;(void)p;(void)i;(void)error;(void)data;
	if (!strcmp(name,"PlaybackStatus")) return g_variant_new_string(g_atomic_int_get(&playing)?"Playing":"Paused");
	if (!strcmp(name,"Metadata")) return metadata();
	if (!strcmp(name,"Position")) {
		g_mutex_lock(&guard); guint64 pos=position; g_mutex_unlock(&guard);
		return g_variant_new_int64(pos*1000000/rate);
	}
	if (!strcmp(name,"CanSeek")) return g_variant_new_boolean(FALSE);
	return g_variant_new_boolean(TRUE);
}
static void changed(void)
{
	GVariantBuilder b; g_variant_builder_init(&b,G_VARIANT_TYPE_VARDICT);
	g_variant_builder_add(&b,"{sv}","PlaybackStatus",g_variant_new_string(g_atomic_int_get(&playing)?"Playing":"Paused"));
	g_variant_builder_add(&b,"{sv}","Metadata",metadata());
	g_dbus_connection_emit_signal(bus,NULL,PLAYER,"org.freedesktop.DBus.Properties","PropertiesChanged",
		g_variant_new("(s@a{sv}@as)",IFACE,g_variant_builder_end(&b),g_variant_new_strv(NULL,0)),NULL);
}
static void method(GDBusConnection *c,const gchar *sender,const gchar *path,const gchar *iface,
	const gchar *name,GVariant *args,GDBusMethodInvocation *inv,gpointer data)
{
	(void)c;(void)sender;(void)path;(void)iface;(void)args;(void)data;
	gboolean enable=g_atomic_int_get(&playing);
	if (!strcmp(name,"Play")) enable=TRUE;
	else if (!strcmp(name,"Pause") || !strcmp(name,"Stop")) enable=FALSE;
	else if (!strcmp(name,"PlayPause")) enable=!enable;
	else if (!strcmp(name,"Next") || !strcmp(name,"Previous")) {
		g_mutex_lock(&guard); next_track=1; tracks++; g_mutex_unlock(&guard);
	} else { g_dbus_method_invocation_return_error(inv,G_DBUS_ERROR,G_DBUS_ERROR_UNKNOWN_METHOD,"Unsupported control");return; }
	if (enable && flock(lease,LOCK_SH|LOCK_NB)) {
		g_dbus_method_invocation_return_error(inv,G_DBUS_ERROR,G_DBUS_ERROR_FAILED,"Suspend is in progress");return;
	}
	g_atomic_int_set(&playing,enable);
	if (!enable) flock(lease,LOCK_UN);
	g_print("AVRCP control: %s\n",name);
	changed();g_dbus_method_invocation_return_value(inv,NULL);
}
static const GDBusInterfaceVTable vtable={.method_call=method,.get_property=property};
static gpointer audio_thread(gpointer data)
{
	(void)data;snd_pcm_t *handle=NULL; guint64 sent=0; unsigned errors=0;
	while (!g_atomic_int_get(&stopping)) {
		if (!g_atomic_int_get(&playing)) {
			if (handle) { snd_pcm_drop(handle);snd_pcm_close(handle);handle=NULL; }
			g_usleep(100000);continue;
		}
		if (!handle) {
			int ret=snd_pcm_open(&handle,device,SND_PCM_STREAM_PLAYBACK,SND_PCM_NONBLOCK);
			if (ret<0) { handle=NULL;g_usleep(1000000);continue; }
			ret=snd_pcm_set_params(handle,SND_PCM_FORMAT_S16_LE,SND_PCM_ACCESS_RW_INTERLEAVED,2,rate,1,200000);
			if (ret<0) { snd_pcm_close(handle);handle=NULL;g_usleep(1000000);continue; }
			g_print("ALSA Bluetooth PCM opened; codec must be recorded from BlueALSA\n");
		}
		g_mutex_lock(&guard);
		if (next_track) { sent=0; next_track=0; }
		g_mutex_unlock(&guard);
		snd_pcm_sframes_t n=snd_pcm_writei(handle,pcm+sent*2,MIN((guint64)1024,frames-sent));
		if (n==-EAGAIN) { snd_pcm_wait(handle,100);continue; }
		if (n<0) {
			errors++;
			if (snd_pcm_recover(handle,n,1)<0) { snd_pcm_close(handle);handle=NULL;g_usleep(1000000); }
			continue;
		}
		sent=(sent+n)%frames;
		if (n>0) g_atomic_int_set(&wrote_pcm,1);
		g_mutex_lock(&guard);position=sent;g_mutex_unlock(&guard);
	}
	if (handle) { snd_pcm_drop(handle);snd_pcm_close(handle); }
	g_print("PCM write/recovery errors: %u\n",errors);return NULL;
}
static gboolean finish(gpointer data)
{
	(void)data;g_main_loop_quit(loop);return G_SOURCE_REMOVE;
}
static void registered(GObject *object,GAsyncResult *result,gpointer data)
{
	(void)data;GError *error=NULL;
	GVariant *reply=g_dbus_connection_call_finish(G_DBUS_CONNECTION(object),result,&error);
	if (!reply) { g_printerr("BlueZ player registration failed\n");g_clear_error(&error);failed=1;g_main_loop_quit(loop); }
	else { g_variant_unref(reply);g_print("BlueZ AVRCP player registered\n"); }
}
int main(int argc,char **argv)
{
	if (argc!=3 || strlen(argv[1])!=17) { fputs("Usage: y2-a2dp-check BDADDR stereo-PCM16.wav (10 minute limit)\n",stderr);return 2; }
	for (unsigned i=0;i<17;i++) if ((i%3==2 && argv[1][i]!=':') || (i%3!=2 && !g_ascii_isxdigit(argv[1][i]))) return 2;
	gchar *wav;gsize bytes;GError *error=NULL;
	struct stat st;
	if (stat(argv[2],&st) || !S_ISREG(st.st_mode) || st.st_size<44 || st.st_size>16*1024*1024 ||
	    !g_file_get_contents(argv[2],&wav,&bytes,&error)) return 1;
	if (memcmp(wav,"RIFF",4) || memcmp(wav+8,"WAVE",4)) return 2;
	unsigned at=12; gboolean format=FALSE;
	while (at+8<=bytes) {
		unsigned n=y2_conn_le32((unsigned char *)wav+at+4);at+=8;
		if (n>bytes-at) return 2;
		if (!memcmp(wav+at-8,"fmt ",4)) {
			if (n<16 || y2_conn_le16((unsigned char *)wav+at)!=1 || y2_conn_le16((unsigned char *)wav+at+2)!=2 ||
			    y2_conn_le16((unsigned char *)wav+at+14)!=16 || y2_conn_le16((unsigned char *)wav+at+12)!=4) return 2;
			rate=y2_conn_le32((unsigned char *)wav+at+4);format=rate>=22050&&rate<=48000;
		} else if (!memcmp(wav+at-8,"data",4)) {
			if (!format || !n || (n&3)) return 2;
			frames=n/4;pcm=g_memdup2(wav+at,n);break;
		}
		at+=n+(n&1);
	}
	g_free(wav);if (!pcm) return 2;
	snprintf(device,sizeof(device),"bluealsa:DEV=%s,PROFILE=a2dp",argv[1]);
	lease=open("/run/y2/activity.lock",O_WRONLY|O_CREAT|O_CLOEXEC|O_NOFOLLOW,0600);
	if (lease<0 || flock(lease,LOCK_SH|LOCK_NB)) return 1;
	bus=g_bus_get_sync(G_BUS_TYPE_SYSTEM,NULL,&error);if (!bus) return 1;
	GDBusNodeInfo *info=g_dbus_node_info_new_for_xml(xml,&error);if (!info) return 1;
	if (!g_dbus_connection_register_object(bus,PLAYER,info->interfaces[0],&vtable,NULL,NULL,&error)) return 1;
	loop=g_main_loop_new(NULL,FALSE);
	GVariantBuilder props;g_variant_builder_init(&props,G_VARIANT_TYPE_VARDICT);
	g_variant_builder_add(&props,"{sv}","PlaybackStatus",g_variant_new_string("Playing"));
	g_variant_builder_add(&props,"{sv}","Metadata",metadata());
	g_variant_builder_add(&props,"{sv}","CanPlay",g_variant_new_boolean(TRUE));
	g_variant_builder_add(&props,"{sv}","CanPause",g_variant_new_boolean(TRUE));
	g_variant_builder_add(&props,"{sv}","CanGoNext",g_variant_new_boolean(TRUE));
	g_variant_builder_add(&props,"{sv}","CanGoPrevious",g_variant_new_boolean(TRUE));
	g_dbus_connection_call(bus,"org.bluez","/org/bluez/hci0","org.bluez.Media1","RegisterPlayer",
		g_variant_new("(o@a{sv})",PLAYER,g_variant_builder_end(&props)),NULL,G_DBUS_CALL_FLAGS_NONE,10000,NULL,registered,NULL);
	g_unix_signal_add(SIGINT,finish,NULL);g_unix_signal_add(SIGTERM,finish,NULL);g_timeout_add_seconds(600,finish,NULL);
	GThread *thread=g_thread_new("a2dp-pcm",audio_thread,NULL);
	g_main_loop_run(loop);g_atomic_int_set(&stopping,1);g_thread_join(thread);
	close(lease);g_free(pcm);g_object_unref(bus);g_dbus_node_info_unref(info);g_main_loop_unref(loop);
	return failed || !g_atomic_int_get(&wrote_pcm) ? 1 : 0;
}
