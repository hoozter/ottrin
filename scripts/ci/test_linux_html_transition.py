"""Admission controls for the production host-policy challenge."""
import errno
import unittest
from linux_html_transition import verify_restricted_transition


class TransitionPolicyTests(unittest.TestCase):
    def fixture(self):
        state = dict(uid=1001, euid=1001, cap_eff='0000000000000000',
                     label_before='unconfined', label_after='unconfined',
                     change_rc=-2, change_errno=0, userns_rc=-1, userns_errno=errno.EPERM)
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
