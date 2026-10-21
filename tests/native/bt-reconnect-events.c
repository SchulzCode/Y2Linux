/* SPDX-License-Identifier: GPL-2.0-only */
/* Actual reconnect implementation on a private bus; no HCI, host bus or peer. */
#define main reconnect_production_main
#include "../../tools/connectivity/reconnect.c"
#undef main
#include <assert.h>
#include <sys/resource.h>

static const char *peer="/org/bluez/hci0/dev_00_11_22_33_44_55";
static gint enumerations, requests, server_ready, unknown_reply, hang_page, disconnects;
static GDBusMethodInvocation *paging;
static GDBusConnection *server;
static gint64 requested_at;
static gboolean peer_up, peer_bonded=TRUE, radio_on;
static void method(GDBusConnection *c,const char *sender,const char *path,
 const char *interface,const char *member,GVariant *args,GDBusMethodInvocation *inv,gpointer unused)
{
 (void)c;(void)sender;(void)path;(void)interface;(void)args;(void)unused;
 if(!strcmp(member,"Disconnect")) {
  /* BlueZ cancels a page in progress and fails its ConnectProfile. */
  g_atomic_int_inc(&disconnects);
  if(paging) {g_dbus_method_invocation_return_dbus_error(paging,"org.bluez.Error.Failed","br-connection-canceled");paging=NULL;}
  g_dbus_method_invocation_return_value(inv,NULL);return;
 }
 if(!strcmp(member,"ConnectProfile")) {
  requested_at=g_get_monotonic_time();g_atomic_int_inc(&requests);
  if(g_atomic_int_get(&hang_page)) {paging=inv;return;}   /* an absent peer: BlueZ pages ~20 s */
  g_dbus_method_invocation_return_dbus_error(inv,g_atomic_int_get(&unknown_reply)?"org.freedesktop.DBus.Error.NoReply":"org.bluez.Error.Failed","peer unavailable");
  return;
 }
 assert(!strcmp(member,"GetManagedObjects"));g_atomic_int_inc(&enumerations);
 GVariantBuilder objects,interfaces,props;
 g_variant_builder_init(&objects,G_VARIANT_TYPE("a{oa{sa{sv}}}"));
 g_variant_builder_init(&interfaces,G_VARIANT_TYPE("a{sa{sv}}"));
 g_variant_builder_init(&props,G_VARIANT_TYPE_VARDICT);
 g_variant_builder_add(&props,"{sv}","Powered",g_variant_new_boolean(radio_on));
 g_variant_builder_add(&interfaces,"{s@a{sv}}","org.bluez.Adapter1",g_variant_builder_end(&props));
 g_variant_builder_add(&objects,"{o@a{sa{sv}}}","/org/bluez/hci0",g_variant_builder_end(&interfaces));
 g_variant_builder_init(&interfaces,G_VARIANT_TYPE("a{sa{sv}}"));
 g_variant_builder_init(&props,G_VARIANT_TYPE_VARDICT);
 g_variant_builder_add(&props,"{sv}","Address",g_variant_new_string("00:11:22:33:44:55"));
 const char *uuids[]={SINK,NULL};
 g_variant_builder_add(&props,"{sv}","UUIDs",g_variant_new_strv(uuids,-1));
 const char *keys[]={"Paired","Bonded","Trusted","Connected","ServicesResolved","Blocked"};
 gboolean values[]={TRUE,peer_bonded,TRUE,peer_up,peer_up,FALSE};
 for(unsigned i=0;i<G_N_ELEMENTS(keys);i++) g_variant_builder_add(&props,"{sv}",keys[i],g_variant_new_boolean(values[i]));
 g_variant_builder_add(&interfaces,"{s@a{sv}}","org.bluez.Device1",g_variant_builder_end(&props));
 g_variant_builder_add(&objects,"{o@a{sa{sv}}}",peer,g_variant_builder_end(&interfaces));
 g_dbus_method_invocation_return_value(inv,g_variant_new("(@a{oa{sa{sv}}})",g_variant_builder_end(&objects)));
}
static gpointer run_server(gpointer address)
{
 GMainContext *context=g_main_context_new();g_main_context_push_thread_default(context);
 server=g_dbus_connection_new_for_address_sync(address,
  G_DBUS_CONNECTION_FLAGS_AUTHENTICATION_CLIENT|G_DBUS_CONNECTION_FLAGS_MESSAGE_BUS_CONNECTION,NULL,NULL,NULL);
 assert(server);
 GDBusNodeInfo *info=g_dbus_node_info_new_for_xml(
  "<node><interface name='org.freedesktop.DBus.ObjectManager'><method name='GetManagedObjects'><arg type='a{oa{sa{sv}}}' direction='out'/></method></interface>"
  "<interface name='org.bluez.Device1'><method name='ConnectProfile'><arg type='s' direction='in'/></method><method name='Disconnect'/></interface></node>",NULL);
 const GDBusInterfaceVTable table={.method_call=method};
 assert(g_dbus_connection_register_object(server,"/",info->interfaces[0],&table,NULL,NULL,NULL));
 assert(g_dbus_connection_register_object(server,peer,info->interfaces[1],&table,NULL,NULL,NULL));
 GVariant *reply=g_dbus_connection_call_sync(server,"org.freedesktop.DBus","/org/freedesktop/DBus",
  "org.freedesktop.DBus","RequestName",g_variant_new("(su)","org.bluez",0),NULL,0,1000,NULL,NULL);
 assert(reply);g_variant_unref(reply);g_atomic_int_set(&server_ready,1);
 g_main_loop_run(g_main_loop_new(context,FALSE));return NULL;
}
static void spin(unsigned milliseconds)
{
 gint64 until=g_get_monotonic_time()+milliseconds*1000;
 do { while(g_main_context_iteration(NULL,FALSE)) {} g_usleep(1000); } while(g_get_monotonic_time()<until);
}
static void property(const char *path,const char *interface,const char *key,gboolean value)
{
 GVariantBuilder props;g_variant_builder_init(&props,G_VARIANT_TYPE_VARDICT);
 g_variant_builder_add(&props,"{sv}",key,g_variant_new_boolean(value));
 assert(g_dbus_connection_emit_signal(server,NULL,path,"org.freedesktop.DBus.Properties","PropertiesChanged",
  g_variant_new("(s@a{sv}^as)",interface,g_variant_builder_end(&props),(const char *[]){NULL}),NULL));
 g_dbus_connection_flush_sync(server,NULL,NULL);
 spin(180);
}
static void intent(const char *operation)
{
 gint64 now=g_get_monotonic_time();
 char *text=g_strdup_printf("[intent]\nversion=1\nboot_id=%s\nsequence=%lld\noperation=%s\ndeadline_us=%lld\n",boot_id,(long long)now,operation,(long long)(now+30000000));
 char *path=g_build_filename(intent_path,"bt-control.ini",NULL);
 assert(g_file_set_contents(path,text,-1,NULL));g_free(path);g_free(text);spin(180);
}
static gpointer unrelated_status(gpointer unused)
{
 (void)unused;
 for(unsigned i=0;i<40;i++) {write_runtime("power.json","{}\n");g_usleep(10000);}
 return NULL;
}
static gboolean end_idle(gpointer loop)
{
 g_main_loop_quit(loop);return G_SOURCE_REMOVE;
}
static void unrelated_files_do_not_wake_observer(void)
{
 GMainLoop *loop=g_main_loop_new(NULL,FALSE);
 struct rusage before,after;
 GThread *writer=g_thread_new("status-writer",unrelated_status,NULL);
 g_timeout_add(500,end_idle,loop);
 assert(!getrusage(RUSAGE_THREAD,&before));g_main_loop_run(loop);
 assert(!getrusage(RUSAGE_THREAD,&after));g_thread_join(writer);
 long switches=after.ru_nvcsw-before.ru_nvcsw;
 printf("unrelated_status_idle_switches=%ld\n",switches);
 assert(switches<=8);g_main_loop_unref(loop);
 unlinkat(runtime_directory,"power.json",0);
}
int main(void)
{
 GTestDBus *test=g_test_dbus_new(G_TEST_DBUS_NONE);g_test_dbus_up(test);
 const char *address=g_test_dbus_get_bus_address(test);
 g_thread_new("fake-bluez",run_server,(gpointer)address);
 while(!g_atomic_int_get(&server_ready)) g_usleep(1000);
 bus=g_dbus_connection_new_for_address_sync(address,
  G_DBUS_CONNECTION_FLAGS_AUTHENTICATION_CLIENT|G_DBUS_CONNECTION_FLAGS_MESSAGE_BUS_CONNECTION,NULL,NULL,NULL);assert(bus);
 char *temporary=g_dir_make_tmp("y2-reconnect-XXXXXX",NULL);assert(temporary);
 runtime_path=temporary;runtime_directory=open(temporary,O_DIRECTORY|O_RDONLY);directory=runtime_directory;
 operation_lock=openat(runtime_directory,"bt-operation.lock",O_CREAT|O_RDWR,0600);assert(operation_lock>=0);
 strcpy(boot_id,"11111111-1111-1111-1111-111111111111");strcpy(preferred,"00:11:22:33:44:55");
 connected=g_hash_table_new_full(g_str_hash,g_str_equal,g_free,NULL);
 assert(start_observer());spin(180);assert(!was_powered);assert(enumerations==1);
 unrelated_files_do_not_wake_observer();
 spin(2200);assert(enumerations==1);assert(requests==0);
 /* Atomic committed intent wakes through inotify, without a 30s delay. */
 intent("power_pending");assert(!inhibited && control_until>g_get_monotonic_time());
 radio_on=TRUE;property("/org/bluez/hci0","org.bluez.Adapter1","Powered",TRUE);assert(requests==0);
 gint64 start=g_get_monotonic_time();intent("power_on");assert(!inhibited);assert(requests==1);
 assert(requested_at>=start && requested_at-start<500000);assert(budget.attempts==1);
 printf("intent_request_ms=%.3f\n",(requested_at-start)/1000.0);
 spin(2200);assert(requests==2);spin(2300);assert(requests==2);assert(enumerations==1);
 /* A completed user connect rearms; disconnect/forget/unknown cannot. */
 intent("disconnect");assert(inhibited);intent("forget");assert(inhibited);intent("uncertain");assert(inhibited);
 spin(2100);assert(requests==2);
 /* Paired without Bonded is never an automatic reconnect candidate. */
 peer_bonded=FALSE;property(peer,"org.bluez.Device1","Bonded",FALSE);
 intent("connect");assert(!inhibited);assert(requests==2);
 peer_bonded=TRUE;property(peer,"org.bluez.Device1","Bonded",TRUE);assert(requests==3);
 intent("power_off");radio_on=FALSE;property("/org/bluez/hci0","org.bluez.Adapter1","Powered",FALSE);
 spin(2100);assert(requests==3);
 /* A user's Connect waiting for the lease cancels the automatic page at once
  * (70cce0f1: refused for ~40 s while a switched-off headset was paged). */
 g_atomic_int_set(&hang_page,1);
 radio_on=TRUE;property("/org/bluez/hci0","org.bluez.Adapter1","Powered",TRUE);
 intent("connect");assert(pending && requests==4);
 assert(flock(open(g_build_filename(runtime_path,"bt-operation.lock",NULL),O_RDWR),LOCK_EX|LOCK_NB));
 start=g_get_monotonic_time();intent("connect_pending");
 assert(disconnects==1 && !pending && !inhibited && !strcmp(last_error,"cancelled_for_user"));
 int user=open(g_build_filename(runtime_path,"bt-operation.lock",NULL),O_RDWR);
 assert(!flock(user,LOCK_EX|LOCK_NB));
 printf("user_lease_after_abort_ms=%.3f\n",(g_get_monotonic_time()-start)/1000.0);
 flock(user,LOCK_UN);close(user);spin(2200);assert(requests==4);
 /* A Connect that failed (headphones off) or a codec change only holds
  * automatic reconnect until the intent's deadline, never for the boot. */
 intent("failed");assert(!inhibited && control_until>g_get_monotonic_time());
 intent("codec");assert(!inhibited && control_until>g_get_monotonic_time());
 spin(2200);assert(requests==4);
 g_atomic_int_set(&hang_page,0);
 intent("power_off");radio_on=FALSE;property("/org/bluez/hci0","org.bluez.Adapter1","Powered",FALSE);
 /* Unavailable owner invalidates cached connected state and preserves budget. */
 unsigned saved=budget.attempts;
 GVariant *reply=g_dbus_connection_call_sync(server,"org.freedesktop.DBus","/org/freedesktop/DBus","org.freedesktop.DBus","ReleaseName",g_variant_new("(s)","org.bluez"),NULL,0,1000,NULL,NULL);
 assert(reply);g_variant_unref(reply);spin(180);assert(!strcmp(last_error,"bluez_unavailable"));assert(budget.attempts==saved);
 /* Same-boot helper restart cannot erase exhausted/pending uncertainty. */
 budget.attempts=2;inhibited=TRUE;pending=TRUE;record();
 y2_reconnect_reset(&budget);inhibited=FALSE;pending=FALSE;load_state();
 assert(budget.attempts==2 && inhibited);
 reply=g_dbus_connection_call_sync(server,"org.freedesktop.DBus","/org/freedesktop/DBus","org.freedesktop.DBus","RequestName",g_variant_new("(su)","org.bluez",0),NULL,0,1000,NULL,NULL);
 assert(reply);g_variant_unref(reply);spin(180);assert(budget.attempts==2 && inhibited);
 g_atomic_int_set(&unknown_reply,1);
 radio_on=TRUE;property("/org/bluez/hci0","org.bluez.Adapter1","Powered",TRUE);
 intent("power_on");assert(inhibited && !strcmp(last_error,"completion_unknown_user_retry_required"));
 gint calls=requests;spin(2200);assert(requests==calls);
 printf("PASS: idle snapshots=1; bounded attempts; intent dispatch<500ms; bond/forget/off/owner guards; user aborts page\n");
 /* Isolated bus and thread exit together; no real service was touched. */
 char *control_path=g_build_filename(intent_path,"bt-control.ini",NULL);
 unlink(control_path);g_free(control_path);rmdir(intent_path);g_free(intent_path);
 const char *files[]={"bt-reconnect.json","bt-reconnect.ini","bt-operation.lock",NULL};
 for(const char **name=files;*name;name++) unlinkat(runtime_directory,*name,0);
 close(operation_lock);close(runtime_directory);rmdir(temporary);g_free(temporary);
 g_test_dbus_stop(test);
 return 0;
}
