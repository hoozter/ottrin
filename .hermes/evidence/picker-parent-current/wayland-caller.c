#include <gtk/gtk.h>
#include <gdk/gdkwayland.h>
#include <stdio.h>
static void exported(GdkWindow *window, const char *handle, gpointer unused) { (void)window;(void)unused;printf("%s\n",handle);fflush(stdout); }
int main(int argc,char **argv) {
 gtk_init(&argc,&argv);GtkWidget *w=gtk_window_new(GTK_WINDOW_TOPLEVEL);
 gtk_window_set_title(GTK_WINDOW(w),"Owned Wayland caller QA");gtk_window_set_default_size(GTK_WINDOW(w),500,300);
 g_signal_connect(w,"destroy",G_CALLBACK(gtk_main_quit),NULL);gtk_widget_show_all(w);
 if(!gdk_wayland_window_export_handle(gtk_widget_get_window(w),exported,NULL,NULL))return 2;
 gtk_main();return 0;
}
