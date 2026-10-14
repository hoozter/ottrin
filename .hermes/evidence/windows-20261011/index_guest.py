"""Use the shared QGA runner only against the task-owned disposable overlay."""
import sys
import guest_exec

guest_exec.DOMAIN = 'ottrin-index-qa'
if __name__ == '__main__':
    sys.exit(guest_exec.execute(sys.stdin.read()))
