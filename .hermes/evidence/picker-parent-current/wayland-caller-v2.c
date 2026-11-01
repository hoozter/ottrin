#include <gtk/gtk.h>
#include <gdk/gdkwayland.h>
#include <wayland-client.h>
#include "foreign-v2.h"
#include <stdio.h>
#include <string.h>
static struct zxdg_exporter_v2 *exporter;
static void global(void *data,struct wl_registry *r,uint32_t name,const char *interface,uint32_t version) {
 (void)data;(void)version;if(!strcmp(interface,"zxdg_exporter_v2"))exporter=wl_registry_bind(r,name,&zxdg_exporter_v2_interface,1);
}
static void removed(void *data,struct wl_registry *r,uint32_t name){(void)data;(void)r;(void)name;}
static void exported(void *data,struct zxdg_exported_v2 *e,const char *handle){(void)data;(void)e;printf("%s\n",handle);fflush(stdout);}
int main(int argc,char **argv) {
 gtk_init(&argc,&argv);GtkWidget *w=gtk_window_new(GTK_WINDOW_TOPLEVEL);
 gtk_window_set_title(GTK_WINDOW(w),"Owned Wayland caller QA");gtk_window_set_default_size(GTK_WINDOW(w),500,300);
 g_signal_connect(w,"destroy",G_CALLBACK(gtk_main_quit),NULL);gtk_widget_show_all(w);
 struct wl_display *display=gdk_wayland_display_get_wl_display(gdk_display_get_default());
 struct wl_registry *registry=wl_display_get_registry(display);
 static const struct wl_registry_listener globals={global,removed};wl_registry_add_listener(registry,&globals,NULL);wl_display_roundtrip(display);if(!exporter)return 2;
 struct zxdg_exported_v2 *e=zxdg_exporter_v2_export_toplevel(exporter,gdk_wayland_window_get_wl_surface(gtk_widget_get_window(w)));
 static const struct zxdg_exported_v2_listener listener={exported};zxdg_exported_v2_add_listener(e,&listener,NULL);wl_display_flush(display);
 gtk_main();zxdg_exported_v2_destroy(e);zxdg_exporter_v2_destroy(exporter);wl_registry_destroy(registry);return 0;
}
