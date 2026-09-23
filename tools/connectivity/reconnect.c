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
#include <sys/file.h>
#include <unistd.h>
#include "reconnect-policy.h"

#define SINK "0000110b-0000-1000-8000-00805f9b34fb"
static GDBusConnection *bus;
static GHashTable *connected;
static char preferred[18];
static gboolean inhibited, pending, was_powered;
static struct y2_reconnect_budget budget;
static gint64 connected_since, control_sequence, control_until;
static unsigned failures, completed, total_attempts;
static char boot_id[64];
static int runtime_directory;
static int operation_lock;
static const char *last_error;
static int directory;

static void write_runtime(const char *name,const char *data)
{
	char temporary[80];snprintf(temporary,sizeof(temporary),".%s.tmp",name);
	int fd=openat(runtime_directory,temporary,O_WRONLY|O_CREAT|O_TRUNC|O_NOFOLLOW|O_CLOEXEC,0600);
	if(fd<0) return;
	size_t length=strlen(data);
	if(write(fd,data,length)!=(ssize_t)length) {close(fd);unlinkat(runtime_directory,temporary,0);return;}
	close(fd);renameat(runtime_directory,temporary,runtime_directory,name);
}
static void record(void)
{
	gint64 now=g_get_monotonic_time();
	const char *state=!was_powered?"Off":g_hash_table_size(connected)?"Connected":
		pending?"Connecting":inhibited?"Inhibited":
		budget.attempts>=2 || (budget.deadline && now>=budget.deadline)?"Exhausted":"Waiting";
	char *json=g_strdup_printf("{\"schema\":1,\"boot_id\":\"%s\",\"monotonic_us\":%lld,\"state\":\"%s\","
		"\"attempts\":%u,\"total_attempts\":%u,\"failures\":%u,\"completed_calls\":%u,"
		"\"pending\":%s,\"inhibited\":%s,\"last_error\":%s%s%s}\n",
		boot_id,(long long)now,state,budget.attempts,total_attempts,failures,completed,
		pending?"true":"false",inhibited?"true":"false",last_error?"\"":"",last_error?last_error:"null",last_error?"\"":"");
	write_runtime("bt-reconnect.json",json);g_free(json);
	GKeyFile *file=g_key_file_new();
	g_key_file_set_string(file,"state","boot_id",boot_id);
	g_key_file_set_integer(file,"state","attempts",budget.attempts);
	g_key_file_set_int64(file,"state","deadline",budget.deadline);
	g_key_file_set_int64(file,"state","next",budget.next);
	g_key_file_set_int64(file,"state","control_sequence",control_sequence);
	g_key_file_set_boolean(file,"state","inhibited",inhibited);
	g_key_file_set_boolean(file,"state","pending",pending);
	g_key_file_set_boolean(file,"state","powered",was_powered);
	g_key_file_set_integer(file,"state","total_attempts",MIN(total_attempts,2147483647U));
	g_key_file_set_integer(file,"state","failures",MIN(failures,2147483647U));
	g_key_file_set_integer(file,"state","completed",MIN(completed,2147483647U));
	char *data=g_key_file_to_data(file,NULL,NULL);write_runtime("bt-reconnect.ini",data);g_free(data);g_key_file_unref(file);
}
static void load_state(void)
{
	GKeyFile *file=g_key_file_new();
	if(g_key_file_load_from_file(file,"/run/y2/bt-reconnect.ini",G_KEY_FILE_NONE,NULL)) {
		char *boot=g_key_file_get_string(file,"state","boot_id",NULL);
		if(!g_strcmp0(boot,boot_id)) {
			budget.attempts=CLAMP(g_key_file_get_integer(file,"state","attempts",NULL),0,2);
			budget.deadline=g_key_file_get_int64(file,"state","deadline",NULL);
			budget.next=g_key_file_get_int64(file,"state","next",NULL);
			inhibited=g_key_file_get_boolean(file,"state","inhibited",NULL) || g_key_file_get_boolean(file,"state","pending",NULL);
			was_powered=g_key_file_get_boolean(file,"state","powered",NULL);
			control_sequence=g_key_file_get_int64(file,"state","control_sequence",NULL);
			total_attempts=MAX(0,g_key_file_get_integer(file,"state","total_attempts",NULL));
			failures=MAX(0,g_key_file_get_integer(file,"state","failures",NULL));
			completed=MAX(0,g_key_file_get_integer(file,"state","completed",NULL));
		}
		g_free(boot);
	}
	g_key_file_unref(file);
}
static void control(void)
{
	GKeyFile *file=g_key_file_new();
	if(g_key_file_load_from_file(file,"/run/y2/bt-control.ini",G_KEY_FILE_NONE,NULL)) {
		char *boot=g_key_file_get_string(file,"intent","boot_id",NULL);
		char *operation=g_key_file_get_string(file,"intent","operation",NULL);
		gint64 sequence=g_key_file_get_int64(file,"intent","sequence",NULL);
		gint64 until=g_key_file_get_int64(file,"intent","deadline_us",NULL),now=g_get_monotonic_time();
		if(g_key_file_get_integer(file,"intent","version",NULL)==1 &&
			!g_strcmp0(boot,boot_id) && sequence>control_sequence && sequence<=now &&
			until>=sequence && until-sequence<=30000000) {
			if(!g_strcmp0(operation,"connect") || !g_strcmp0(operation,"power_on")) {
				if(!pending && until>=now) {inhibited=FALSE;y2_reconnect_reset(&budget);last_error=NULL;}
			} else if(!g_strcmp0(operation,"disconnect") || !g_strcmp0(operation,"forget") ||
				!g_strcmp0(operation,"power_off") || !g_strcmp0(operation,"pair") ||
				!g_strcmp0(operation,"uncertain") || !g_strcmp0(operation,"connect_pending") ||
				!g_strcmp0(operation,"power_pending")) inhibited=TRUE;
			control_sequence=sequence;control_until=until;record();
		}
		g_free(boot);g_free(operation);
	}
	g_key_file_unref(file);
}

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
	if (reply) { g_variant_unref(reply); completed++; last_error=NULL; }
	else {
		failures++;last_error="profile_connection_failed";
		/* A local/client timeout cannot prove that BlueZ stopped its call.
		 * Reconcile observations; never overlap another automatic request. */
		if(!error || !g_dbus_error_is_remote_error(error) ||
			g_error_matches(error,G_DBUS_ERROR,G_DBUS_ERROR_NO_REPLY) ||
			g_error_matches(error,G_DBUS_ERROR,G_DBUS_ERROR_TIMEOUT)) {
			inhibited=TRUE;last_error="completion_unknown_user_retry_required";
		}
	}
	g_clear_error(&error); pending=FALSE;
	flock(operation_lock,LOCK_UN);
	budget.next=g_get_monotonic_time()+2*G_USEC_PER_SEC;record();
}
static gboolean management(gint fd,GIOCondition condition,gpointer unused)
{
	(void)unused;
	control();
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
		 * An explicit connect/power-on intent clears it. */
		if (packet[13]==2 || packet[13]==4) inhibited=TRUE;
	}
	return G_SOURCE_CONTINUE;
}
static gboolean poll_devices(gpointer unused)
{
	(void)unused;
	control();
	GError *error=NULL;
	GVariant *reply=g_dbus_connection_call_sync(bus,"org.bluez","/",
		"org.freedesktop.DBus.ObjectManager","GetManagedObjects",NULL,
		G_VARIANT_TYPE("(a{oa{sa{sv}}})"),G_DBUS_CALL_FLAGS_NONE,3000,NULL,&error);
	if (!reply) { g_clear_error(&error); last_error="bluez_unavailable";record();return G_SOURCE_CONTINUE; }
	GVariant *objects=g_variant_get_child_value(reply,0), *interfaces, *properties;
	GVariantIter it; const char *path; gboolean powered=FALSE;
	g_variant_iter_init(&it,objects);
	while (g_variant_iter_next(&it,"{&o@a{sa{sv}}}",&path,&interfaces)) {
		properties=g_variant_lookup_value(interfaces,"org.bluez.Adapter1",G_VARIANT_TYPE_VARDICT);
		if (properties) { gboolean on=FALSE; g_variant_lookup(properties,"Powered","b",&on); powered|=on; g_variant_unref(properties); }
		g_variant_unref(interfaces);
	}
	if (powered && !was_powered && !inhibited) y2_reconnect_reset(&budget);
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
				if (!g_hash_table_contains(connected,path)) remember(address);
			} else if (!up && !g_ascii_strcasecmp(preferred,address)) {
				g_free(candidate); candidate=g_strdup(path);
			}
		}
		g_variant_unref(properties);
	}
	/* Do not displace another connected audio device. */
	gint64 timestamp=g_get_monotonic_time();
	if(g_hash_table_size(now)) {
		if(!connected_since) connected_since=timestamp;
		if(timestamp-connected_since>=30*G_USEC_PER_SEC && !inhibited) y2_reconnect_reset(&budget);
	} else connected_since=0;
	if (powered && !inhibited && !pending && candidate && !g_hash_table_size(now) &&
	    !flock(operation_lock,LOCK_EX|LOCK_NB)) {
		/* Re-read the user's intent while holding the same operation lock. */
		control();
		if(!inhibited && timestamp>=control_until && y2_reconnect_attempt(&budget,timestamp)) {
			pending=TRUE;
			total_attempts++;record();
			g_dbus_connection_call(bus,"org.bluez",candidate,"org.bluez.Device1","ConnectProfile",
				g_variant_new("(s)",SINK),NULL,G_DBUS_CALL_FLAGS_NONE,10000,NULL,connection_done,NULL);
		} else flock(operation_lock,LOCK_UN);
	}
	g_free(candidate); g_hash_table_unref(connected); connected=now;
	g_variant_unref(objects); g_variant_unref(reply);
	record();
	return G_SOURCE_CONTINUE;
}
int main(void)
{
	if (geteuid()) return 1;
	runtime_directory=open("/run/y2",O_RDONLY|O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC);if(runtime_directory<0)return 1;
	int lock=openat(runtime_directory,"bt-reconnect.lock",O_CREAT|O_RDWR|O_NOFOLLOW|O_CLOEXEC,0600);
	if(lock<0 || flock(lock,LOCK_EX|LOCK_NB))return 1;
	operation_lock=openat(runtime_directory,"bt-operation.lock",O_CREAT|O_RDWR|O_NOFOLLOW|O_CLOEXEC,0600);
	if(operation_lock<0)return 1;
	FILE *boot=fopen("/proc/sys/kernel/random/boot_id","r");
	if(!boot || fscanf(boot,"%63s",boot_id)!=1)return 1;
	fclose(boot);load_state();
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
