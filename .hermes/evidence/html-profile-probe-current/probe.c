#define _GNU_SOURCE
#include <fcntl.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>
static void fail(const char *s) { perror(s); exit(2); }
static void owned(const char *path, int directory) {
    struct stat s;
    if (lstat(path, &s) || s.st_uid != getuid() ||
        (directory ? !S_ISDIR(s.st_mode) : !S_ISREG(s.st_mode)) ||
        (s.st_mode & 0077)) fail("private probe path");
}
int main(int argc, char **argv) {
    if (argc != 2 || getuid() == 0 || geteuid() != getuid() || argv[1][0] != '/') { fprintf(stderr, "entry arguments or identity invalid: argc=%d uid=%u euid=%u\n", argc,getuid(),geteuid()); return 2; }
    char input[PATH_MAX], profile[PATH_MAX], output[PATH_MAX];
    if (snprintf(input,sizeof input,"%s/input.html",argv[1]) >= (int)sizeof input ||
        snprintf(profile,sizeof profile,"%s/profile",argv[1]) >= (int)sizeof profile ||
        snprintf(output,sizeof output,"%s/output",argv[1]) >= (int)sizeof output) return 2;
    owned(argv[1],1); owned(input,0); owned(profile,1); owned(output,1);
    struct stat browser;
    if (stat("/usr/libexec/ottrin/html-probe-runtime/chrome", &browser)) fail("browser metadata");
    if (!S_ISREG(browser.st_mode) || browser.st_uid || (browser.st_mode & 0022)) { fprintf(stderr,"untrusted browser uid=%u mode=%o\n",browser.st_uid,browser.st_mode); return 2; }
    execl("/usr/bin/bwrap", "bwrap", "--unshare-all", "--die-with-parent", "--clearenv",
        "--setenv","HOME","/profile", "--setenv","LANG","C.UTF-8",
        "--ro-bind","/usr","/usr", "--ro-bind","/etc","/etc", "--ro-bind","/bin","/bin",
        "--ro-bind","/lib","/lib", "--ro-bind","/lib64","/lib64", "--ro-bind","/opt","/opt",
        "--proc","/proc", "--dev","/dev", "--tmpfs","/tmp", "--dir","/input",
        "--bind",profile,"/profile", "--bind",output,"/output", "--ro-bind",input,"/input/document.html",
        "/usr/libexec/ottrin/html-probe-runtime/chrome", "--headless=new", "--disable-gpu", "--disable-javascript",
        "--disable-background-networking", "--disable-component-update", "--disable-default-apps",
        "--disable-extensions", "--disable-sync", "--metrics-recording-only", "--no-first-run",
        "--no-default-browser-check", "--user-data-dir=/profile", "--screenshot=/output/output.png",
        "--window-size=256,256", "file:///input/document.html", (char *)NULL);
    fail("execute fixed namespace plan");
}
