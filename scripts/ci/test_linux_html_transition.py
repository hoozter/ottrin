"""Admission controls for the production host-policy challenge."""
import errno
import unittest
from linux_html_transition import verify_restricted_transition


class TransitionPolicyTests(unittest.TestCase):
    def fixture(self):
        state = dict(uid=1001, euid=1001, cap_eff='0000000000000000',
                     label_before='unconfined', label_after='unconfined',
                     change_rc=-2, change_errno=0, userns_rc=-1, userns_errno=errno.EPERM,
                     mountns_rc=-2, mountns_errno=0, label_after_userns='not entered')
        return dict(baseline=state, transition=dict(state, change_rc=-1, change_errno=errno.EACCES))

    def test_denied_change_retains_restrictions(self):
        self.assertIn('profile change denied', verify_restricted_transition(self.fixture(), 1001))

    def test_successful_change_must_retain_stack_and_deny_userns(self):
        for label in ['ottrin-html-render//&unconfined (mixed)',
                      'unconfined//&ottrin-html-render (enforce)']:
            probe = self.fixture()
            probe['transition'].update(change_rc=0, change_errno=0, label_after=label)
            self.assertIn('restrictions stacked', verify_restricted_transition(probe, 1001))

    def test_rejects_permission_gain_even_with_expected_stack(self):
        probe = self.fixture()
        probe['transition'].update(change_rc=0, change_errno=0,
                                   label_after='ottrin-html-render//&unconfined (mixed)',
                                   userns_rc=0, userns_errno=0)
        with self.assertRaises(AssertionError):
            verify_restricted_transition(probe, 1001)

    def default_profile_fixture(self):
        probe = self.fixture()
        for state in probe.values():
            state.update(userns_rc=0, userns_errno=0, mountns_rc=-1, mountns_errno=errno.EPERM,
                         label_after_userns='unprivileged_userns (enforce)')
        probe['transition'].update(change_rc=0, change_errno=0,
                                   label_after='ottrin-html-render//&unconfined (enforce)',
                                   label_after_userns='ottrin-html-render//&unprivileged_userns (enforce)')
        return probe

    def test_default_profile_retains_capability_denial_after_stack(self):
        self.assertIn('in-namespace privileged operation denied',
                      verify_restricted_transition(self.default_profile_fixture(), 1001))

    def test_rejects_privileged_operation_or_lost_guard_after_userns(self):
        for updates in [dict(mountns_rc=0, mountns_errno=0),
                        dict(label_after_userns='ottrin-html-render (enforce)'),
                        dict(label_after_userns='unconfined'),
                        dict(label_after_userns='unprivileged_userns (complain)')]:
            probe = self.default_profile_fixture()
            probe['transition'].update(updates)
            with self.subTest(updates=updates), self.assertRaises(AssertionError):
                verify_restricted_transition(probe, 1001)

    def test_default_profile_can_still_deny_the_transition_or_userns(self):
        probe = self.default_profile_fixture()
        probe['transition'].update(change_rc=-1, change_errno=errno.EPERM, label_after='unconfined')
        self.assertIn('profile change denied', verify_restricted_transition(probe, 1001))
        probe['transition'].update(userns_rc=-1, userns_errno=errno.EPERM)
        self.assertIn('profile change denied', verify_restricted_transition(probe, 1001))

    def test_rejects_plain_trusted_profile_even_if_another_policy_denies_userns(self):
        probe = self.fixture()
        probe['transition'].update(change_rc=0, change_errno=0,
                                   label_after='ottrin-html-render (enforce)')
        with self.assertRaises(AssertionError):
            verify_restricted_transition(probe, 1001)

    def test_rejects_missing_profile_privileged_caller_or_invalid_baseline(self):
        for target, updates in [('transition', dict(change_errno=errno.ENOENT)),
                                ('transition', dict(cap_eff='200000000')),
                                ('baseline', dict(userns_rc=0, userns_errno=0)),
                                ('baseline', dict(label_before='other-profile (enforce)'))]:
            probe = self.fixture()
            probe[target].update(updates)
            with self.subTest(target=target, updates=updates), self.assertRaises(AssertionError):
                verify_restricted_transition(probe, 1001)


if __name__ == '__main__':
    unittest.main()
