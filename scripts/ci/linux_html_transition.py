"""Probe the rights retained after an unprivileged AppArmor profile change."""
import errno
import subprocess


PROBE_SOURCE = r'''
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <sched.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

static int label(char *buffer, size_t size) {
 FILE *file = fopen("/proc/self/attr/current", "r");
 if (!file) return -1;
 int ok = fgets(buffer, size, file) != NULL;
 fclose(file);
 if (!ok) return -1;
 buffer[strcspn(buffer, "\n")] = 0;
 return 0;
}

int main(int argc, char **argv) {
 char before[4096], after[4096], userns_label[4096] = "not entered", line[4096], caps[64] = "";
 if (label(before, sizeof before)) return 2;
 FILE *status = fopen("/proc/self/status", "r");
 if (!status) return 3;
 while (fgets(line, sizeof line, status))
  if (sscanf(line, "CapEff: %63s", caps) == 1) break;
 fclose(status);
 if (!caps[0]) return 4;
 int result = -2, error = 0;
 if (argc == 1) {
  void *lib = dlopen("libapparmor.so.1", RTLD_NOW);
  if (!lib) return 5;
  int (*change)(const char *) = dlsym(lib, "aa_change_profile");
  if (!change) return 6;
  result = change("ottrin-html-render");
  error = result < 0 ? errno : 0;
 } else if (argc != 2 || strcmp(argv[1], "--baseline")) return 7;
 if (label(after, sizeof after)) return 8;
 uid_t uid = getuid(), euid = geteuid();
 int userns = unshare(CLONE_NEWUSER);
 int userns_error = userns < 0 ? errno : 0;
 int mountns = -2, mountns_error = 0;
 if (userns == 0) {
  if (label(userns_label, sizeof userns_label)) return 9;
  mountns = unshare(CLONE_NEWNS);
  mountns_error = mountns < 0 ? errno : 0;
 }
 printf("uid=%u\neuid=%u\ncap_eff=%s\nlabel_before=%s\nlabel_after=%s\n"
        "change_rc=%d\nchange_errno=%d\nuserns_rc=%d\nuserns_errno=%d\n"
        "label_after_userns=%s\nmountns_rc=%d\nmountns_errno=%d\n",
        uid, euid, caps, before, after, result, error, userns, userns_error,
        userns_label, mountns, mountns_error);
 return 0;
}
'''


def probe_transition(root):
    source = root / 'transition.c'
    executable = root / 'transition'
    source.write_text(PROBE_SOURCE)
    subprocess.run(['cc', str(source), '-ldl', '-o', str(executable)], check=True)
    result = {}
    for name, arguments in [('baseline', ['--baseline']), ('transition', [])]:
        child = subprocess.run([str(executable), *arguments], check=True,
                               capture_output=True, text=True, timeout=3)
        values = dict(line.split('=', 1) for line in child.stdout.splitlines())
        for key in ['uid', 'euid', 'change_rc', 'change_errno', 'userns_rc', 'userns_errno',
                    'mountns_rc', 'mountns_errno']:
            values[key] = int(values[key])
        result[name] = values
    return result


def verify_restricted_transition(probe, uid):
    baseline, changed = probe['baseline'], probe['transition']
    for state in [baseline, changed]:
        assert state['uid'] == state['euid'] == uid and uid != 0, state
        assert int(state['cap_eff'], 16) == 0, 'Probe must have no effective capabilities'
        assert state['label_before'] == 'unconfined', state
    assert baseline['label_after'] == 'unconfined', baseline

    def enforced_profiles(label):
        parts = label.rsplit(' (', 1)
        assert len(parts) == 2, label
        profiles, mode = parts
        assert mode in ['enforce)', 'mixed)'], label
        return set(profiles.split('//&'))

    if baseline['userns_rc'] == -1:
        assert baseline['userns_errno'] in [errno.EPERM, errno.EACCES], baseline
        assert changed['userns_rc'] == -1 and changed['userns_errno'] in [errno.EPERM, errno.EACCES], (
            'Profile change bypassed the host user namespace denial', changed)
        restriction = 'user namespace creation denied'
    else:
        # AppArmor can admit a user namespace under a restrictive default
        # profile. Challenge CAP_SYS_ADMIN inside it, rather than treating a
        # successful userns call as privilege gain. No mounts are performed.
        assert baseline['userns_rc'] == 0, baseline
        guard = enforced_profiles(baseline['label_after_userns'])
        assert guard and 'unconfined' not in guard, baseline
        for state in [baseline, changed]:
            if state['userns_rc'] == -1:
                assert state['userns_errno'] in [errno.EPERM, errno.EACCES], state
                continue
            assert state['userns_rc'] == 0, state
            assert guard <= enforced_profiles(state['label_after_userns']), state
            assert state['mountns_rc'] == -1 and state['mountns_errno'] in [errno.EPERM, errno.EACCES], (
                'Caller acquired privileged namespace permission', state)
        restriction = 'in-namespace privileged operation denied'
    if changed['change_rc'] == -1:
        assert changed['change_errno'] in [errno.EPERM, errno.EACCES], changed
        assert changed['label_after'] == 'unconfined', changed
        return 'profile change denied; ' + restriction
    assert changed['change_rc'] == 0, changed
    # Newer kernels allow the call but retain unconfined as a stacked profile.
    # The privilege challenge above must still fail; syscall success is not
    # evidence that the caller acquired the trusted executable's permissions.
    assert enforced_profiles(changed['label_after']) == {'ottrin-html-render', 'unconfined'}, changed
    return 'unconfined restrictions stacked; ' + restriction
