/* Real development sandbox caller: public portal API only, no backend shortcut. */
#include <gio/gio.h>
#include <glib/gstdio.h>
#include <stdio.h>
#include <string.h>
static GMainLoop *loop;
static int outcome = 1;
static gboolean saving = FALSE;
static const char *expected = "preserve sandbox exported document\n";
static void response(GDBusConnection *connection, const gchar *sender, const gchar *path,
                     const gchar *interface, const gchar *signal, GVariant *parameters, gpointer data) {
  (void)connection;(void)sender;(void)path;(void)interface;(void)signal;(void)data;
  guint32 code;GVariant *results;g_variant_get(parameters,"(u@a{sv})",&code,&results);
  gchar **uris=NULL;g_variant_lookup(results,"uris","^as",&uris);
  if(code==0 && uris && uris[0] && !uris[1]) {
    GError *error=NULL;gchar *filename=g_filename_from_uri(uris[0],NULL,&error);
    gchar *content=NULL;gsize size=0;
    if(saving && filename && g_str_has_suffix(filename,"/saved-from-sandbox.txt")) {
      FILE *file=fopen(filename,"wb");
      if(file) {
        const char *payload="saved by sandbox caller\n";
        gboolean written=fwrite(payload,1,strlen(payload),file)==strlen(payload);
        if(fclose(file)!=0)written=FALSE;
        if(written){printf("PASS saved_uri=%s\n",uris[0]);outcome=0;}
      }
      if(outcome)fprintf(stderr,"Save export write failed\n");
    } else if(!saving && filename && g_str_has_suffix(filename,"/original\200.txt") &&
       g_file_get_contents(filename,&content,&size,&error) && size==strlen(expected) && memcmp(content,expected,size)==0) {
      printf("PASS exported_uri=%s bytes=%zu\n",uris[0],size);outcome=0;
    } else fprintf(stderr,"Export read failed: %s\n",error?error->message:"unexpected export path/content");
    g_clear_error(&error);g_free(filename);g_free(content);
  } else fprintf(stderr,"Unexpected response %u\n",code);
  g_strfreev(uris);g_variant_unref(results);g_main_loop_quit(loop);
}
static gboolean timed_out(gpointer data) {(void)data;fprintf(stderr,"Sandbox request timeout\n");g_main_loop_quit(loop);return G_SOURCE_REMOVE;}
int main(int argc,char **argv) {
  if(argc!=2 && argc!=3)return 2;
  saving=argc==3 && strcmp(argv[2],"save")==0;
  gchar *source=g_build_filename(argv[1],"original\200.txt",NULL);
  if(g_file_test(source,G_FILE_TEST_EXISTS)){fprintf(stderr,"Original visible inside sandbox\n");g_free(source);return 3;}g_free(source);
  gchar *info=NULL;gsize length=0;
  if(!g_file_get_contents("/.flatpak-info",&info,&length,NULL) || !strstr(info,"org.ottrin.SandboxQA")){fprintf(stderr,"Missing genuine sandbox identity\n");g_free(info);return 4;}g_free(info);
  GError *error=NULL;GDBusConnection *connection=g_bus_get_sync(G_BUS_TYPE_SESSION,NULL,&error);
  if(!connection){fprintf(stderr,"Session bus: %s\n",error->message);g_error_free(error);return 5;}
  loop=g_main_loop_new(NULL,FALSE);
  guint subscription=g_dbus_connection_signal_subscribe(connection,"org.freedesktop.portal.Desktop","org.freedesktop.portal.Request","Response",NULL,NULL,G_DBUS_SIGNAL_FLAGS_NONE,response,NULL,NULL);
  GVariantBuilder opts;g_variant_builder_init(&opts,G_VARIANT_TYPE_VARDICT);
  g_variant_builder_add(&opts,"{sv}","handle_token",g_variant_new_string("sandboxopen"));
  g_variant_builder_add(&opts,"{sv}","current_folder",g_variant_new_bytestring(argv[1]));
  if(saving)g_variant_builder_add(&opts,"{sv}","current_name",g_variant_new_string("saved-from-sandbox.txt"));
  printf("READY original_inaccessible=true sandbox_identity=org.ottrin.SandboxQA\n");fflush(stdout);
  GVariant *result=g_dbus_connection_call_sync(connection,"org.freedesktop.portal.Desktop","/org/freedesktop/portal/desktop","org.freedesktop.portal.FileChooser",saving?"SaveFile":"OpenFile",g_variant_new("(ssa{sv})","",saving?"Sandbox portal Save QA":"Sandbox portal Open QA",&opts),G_VARIANT_TYPE("(o)"),G_DBUS_CALL_FLAGS_NONE,10000,NULL,&error);
  if(!result){fprintf(stderr,"OpenFile: %s\n",error->message);g_error_free(error);outcome=6;}
  else {g_variant_unref(result);g_timeout_add_seconds(60,timed_out,NULL);g_main_loop_run(loop);}
  g_dbus_connection_signal_unsubscribe(connection,subscription);g_main_loop_unref(loop);g_object_unref(connection);return outcome;
}
