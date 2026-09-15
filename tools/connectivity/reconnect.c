// SPDX-License-Identifier: GPL-2.0-only
/* Audio-device policy above unmodified BlueZ. Device/profile operations use
 * D-Bus; a read-only Linux management socket distinguishes remote link loss
 * from an intentional local disconnect. It sends no HCI/mgmt commands.
 * One preferred, paired, trusted A2DP sink; no automatic phone/GATT pairing. */
#define _GNU_SOURCE
#include <gio/gio.h>
#include <glib-unix.h>
#include <bluetooth/bluetooth.h>
#include <bluetooth/hci.h>
#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <unistd.h>

#define SINK "0000110b-0000-1000-8000-00805f9b34fb"
static GDBusConnection *bus;
static GHashTable *connected;
static char preferred[18];
static gboolean inhibited, pending, was_powered;
static unsigned attempts;
static gint64 next_attempt;
static int directory;

static gboolean address_valid(const char *s)
{
	if (strlen(s)!=17) return FALSE;
	for (unsigned i=0;i<17;i++) {
		if (i%3==2) { if (s[i]!=':') return FALSE; }
		else if (!g_ascii_isxdigit(s[i])) return FALSE;
	}
	return TRUE;
}
static void remember(const char *address)
{
	if (!strcmp(address,preferred)) return;
	int fd=openat(directory,"preferred-audio.new",O_WRONLY|O_CREAT|O_TRUNC|O_NOFOLLOW|O_CLOEXEC,0600);
	if (fd<0) return;
	if (fchmod(fd,0600) || write(fd,address,17)!=17 || fsync(fd)) { close(fd); return; }
	close(fd);
	if (renameat(directory,"preferred-audio.new",directory,"preferred-audio") || fsync(directory)) return;
	g_strlcpy(preferred,address,sizeof(preferred));
}
static void connection_done(GObject *object,GAsyncResult *result,gpointer unused)
{
	(void)unused;
	GError *error=NULL;
	GVariant *reply=g_dbus_connection_call_finish(G_DBUS_CONNECTION(object),result,&error);
	if (reply) { g_variant_unref(reply); attempts=0; }
	else if (attempts<6) attempts++;
	g_clear_error(&error); pending=FALSE;
	/* Bounded requests and backoff; a missing peer does not hold up boot.
	 * After the first burst, retry at most once per minute. */
	next_attempt=g_get_monotonic_time()+MIN(60U,1U<<attempts)*G_USEC_PER_SEC;
}
static gboolean management(gint fd,GIOCondition condition,gpointer unused)
{
	(void)unused;
	if (condition & (G_IO_ERR|G_IO_HUP|G_IO_NVAL)) { inhibited=TRUE; return G_SOURCE_REMOVE; }
	unsigned char packet[1024]; ssize_t n;
	while ((n=recv(fd,packet,sizeof(packet),MSG_DONTWAIT))>0) {
		/* mgmt_hdr: opcode/index/length, followed by address/type/reason. */
		if (n!=14 || packet[0]!=0x0c || packet[1] || packet[4]!=8 || packet[5]) continue;
		char address[18];
		snprintf(address,sizeof(address),"%02X:%02X:%02X:%02X:%02X:%02X",
			packet[11],packet[10],packet[9],packet[8],packet[7],packet[6]);
		if (g_ascii_strcasecmp(address,preferred)) continue;
		/* Local disconnect/authentication failure require user action.
		 * Radio off/on or a later explicit successful connection clears it. */
		if (packet[13]==2 || packet[13]==4) inhibited=TRUE;
	}
	return G_SOURCE_CONTINUE;
}
static gboolean poll_devices(gpointer unused)
{
	(void)unused;
	GError *error=NULL;
	GVariant *reply=g_dbus_connection_call_sync(bus,"org.bluez","/",
		"org.freedesktop.DBus.ObjectManager","GetManagedObjects",NULL,
		G_VARIANT_TYPE("(a{oa{sa{sv}}})"),G_DBUS_CALL_FLAGS_NONE,3000,NULL,&error);
	if (!reply) { g_clear_error(&error); was_powered=FALSE; return G_SOURCE_CONTINUE; }
	GVariant *objects=g_variant_get_child_value(reply,0), *interfaces, *properties;
	GVariantIter it; const char *path; gboolean powered=FALSE;
	g_variant_iter_init(&it,objects);
	while (g_variant_iter_next(&it,"{&o@a{sa{sv}}}",&path,&interfaces)) {
		properties=g_variant_lookup_value(interfaces,"org.bluez.Adapter1",G_VARIANT_TYPE_VARDICT);
		if (properties) { gboolean on=FALSE; g_variant_lookup(properties,"Powered","b",&on); powered|=on; g_variant_unref(properties); }
		g_variant_unref(interfaces);
	}
	if (powered && !was_powered) { inhibited=FALSE; attempts=0; next_attempt=0; }
	was_powered=powered;
	char *candidate=NULL;
	GHashTable *now=g_hash_table_new_full(g_str_hash,g_str_equal,g_free,NULL);
	g_variant_iter_init(&it,objects);
	while (g_variant_iter_next(&it,"{&o@a{sa{sv}}}",&path,&interfaces)) {
		properties=g_variant_lookup_value(interfaces,"org.bluez.Device1",G_VARIANT_TYPE_VARDICT);
		g_variant_unref(interfaces);
		if (!properties) continue;
		const char *address=NULL; gboolean paired=FALSE,trusted=FALSE,up=FALSE,resolved=FALSE,blocked=FALSE,sink=FALSE;
		g_variant_lookup(properties,"Address","&s",&address);
		g_variant_lookup(properties,"Paired","b",&paired);
		g_variant_lookup(properties,"Trusted","b",&trusted);
		g_variant_lookup(properties,"Connected","b",&up);
		g_variant_lookup(properties,"ServicesResolved","b",&resolved);
		g_variant_lookup(properties,"Blocked","b",&blocked);
		GVariant *uuids=g_variant_lookup_value(properties,"UUIDs",G_VARIANT_TYPE_STRING_ARRAY);
		if (uuids) {
			GVariantIter u; const char *uuid; g_variant_iter_init(&u,uuids);
			while (g_variant_iter_next(&u,"&s",&uuid)) sink |= !g_ascii_strcasecmp(uuid,SINK);
			g_variant_unref(uuids);
		}
		if (address && address_valid(address) && sink && paired && trusted && !blocked) {
			if (up && resolved) {
				g_hash_table_add(now,g_strdup(path));
				if (!g_hash_table_contains(connected,path)) { remember(address); inhibited=FALSE; attempts=0; }
			} else if (!up && !g_ascii_strcasecmp(preferred,address)) {
				g_free(candidate); candidate=g_strdup(path);
			}
		}
		g_variant_unref(properties);
	}
	/* Do not displace another connected audio device. */
	if (powered && !inhibited && !pending && candidate && !g_hash_table_size(now) &&
	    g_get_monotonic_time()>=next_attempt) {
		pending=TRUE;
		g_dbus_connection_call(bus,"org.bluez",candidate,"org.bluez.Device1","ConnectProfile",
			g_variant_new("(s)",SINK),NULL,G_DBUS_CALL_FLAGS_NONE,45000,NULL,connection_done,NULL);
	}
	g_free(candidate); g_hash_table_unref(connected); connected=now;
	g_variant_unref(objects); g_variant_unref(reply);
	return G_SOURCE_CONTINUE;
}
int main(void)
{
	if (geteuid()) return 1;
	struct stat st;
	directory=open("/data/bluetooth",O_RDONLY|O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC);
	if (directory<0 || fstat(directory,&st) || st.st_uid || (st.st_mode&077)) return 1;
	int file=openat(directory,"preferred-audio",O_RDONLY|O_NOFOLLOW|O_CLOEXEC|O_NONBLOCK);
	if (file>=0) {
		if (fstat(file,&st) || !S_ISREG(st.st_mode) || st.st_uid || (st.st_mode&077) ||
		    st.st_size!=17 || read(file,preferred,17)!=17 || !address_valid(preferred)) return 1;
		close(file);
	} else if (errno!=ENOENT) return 1;
	int fd=socket(AF_BLUETOOTH,SOCK_RAW|SOCK_CLOEXEC|SOCK_NONBLOCK,BTPROTO_HCI);
	struct sockaddr_hci address={.hci_family=AF_BLUETOOTH,.hci_dev=HCI_DEV_NONE,.hci_channel=HCI_CHANNEL_CONTROL};
	if (fd<0 || bind(fd,(void *)&address,sizeof(address))) return 1;
	bus=g_bus_get_sync(G_BUS_TYPE_SYSTEM,NULL,NULL); if (!bus) return 1;
	connected=g_hash_table_new_full(g_str_hash,g_str_equal,g_free,NULL);
	g_unix_fd_add(fd,G_IO_IN|G_IO_HUP|G_IO_ERR,management,NULL);
	g_timeout_add_seconds(2,poll_devices,NULL);
	GMainLoop *loop=g_main_loop_new(NULL,FALSE); g_main_loop_run(loop);
	return 0;
}
