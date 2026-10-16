"""Offline browser contract regression against a real headless Chromium page."""

import argparse
import importlib.util
import json
import time
import unittest
from unittest.mock import patch
from pathlib import Path


MODULE_PATH = Path(__file__).resolve().parents[1] / "crates/wunder-runtime/src/services/browser/browser_bridge.py"
SPEC = importlib.util.spec_from_file_location("browser_bridge", MODULE_PATH)
bridge = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(bridge)


class BrowserBridgeTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        args = argparse.Namespace(timeout=2, launch_arg=[], mode="launch", channel=None,
                                  executable_path=None, attach_endpoint=None, headless=True,
                                  width=900, height=600, max_tabs=4,
                                  allow_private_network=True, deny_file_scheme=True, allow_host=[])
        cls.session = bridge.BridgeSession(args, bridge.Policy(args))

    @classmethod
    def tearDownClass(cls):
        cls.session.close()

    def setUp(self):
        self.page, self.target = self.session._resolve_page()
        self.page.goto("about:blank")
        self.page.set_content("""<h1>Sample form</h1>
          <input id="field" placeholder="Sample field">
          <input id="secret" type="password">
          <select id="choice" multiple><option value="">Empty</option>
            <option value="a">A</option><option value="b">B</option></select>
          <button id="send" onclick="setTimeout(() => document.querySelector('#status').textContent='Complete', 30)">Send</button>
          <p id="status">Ready</p>""")

    def act(self, request, **options):
        return self.session.handle({"action": "act", "target_id": self.target,
                                    "request": request, **options})

    def test_batch_fills_waits_and_observes_without_echo(self):
        result = self.act({"kind": "batch", "steps": [
            {"kind": "type", "selector": "#field", "text": "sample-value"},
            {"kind": "type", "selector": "#secret", "text": "synthetic-secret"},
            {"kind": "click", "selector": "#send"},
            {"kind": "wait", "text": "Complete", "timeout_ms": 1000},
        ]}, include_snapshot=True, max_chars=500)
        self.assertTrue(result["success"])
        self.assertEqual(result["data"]["completed_steps"], 4)
        self.assertIn("snapshot", result["data"])
        self.assertNotIn("synthetic-secret", json.dumps(result))
        self.assertEqual(self.page.locator("#field").input_value(), "sample-value")
        self.assertEqual(self.page.locator("#secret").input_value(), "synthetic-secret")
        self.assertEqual(self.page.locator("#status").inner_text(), "Complete")

    def test_submit_presses_enter_and_input_errors_do_not_echo(self):
        self.page.locator("#field").evaluate("el => el.onkeydown = e => { if(e.key === 'Enter') document.querySelector('#status').textContent='Submitted'; }")
        self.assertTrue(self.act({"kind": "type", "selector": "#field", "text": "sample", "submit": True})["success"])
        self.assertEqual(self.page.locator("#status").inner_text(), "Submitted")
        result = self.act({"kind": "type", "selector": "#missing", "text": "synthetic-secret", "timeout_ms": 30})
        self.assertFalse(result["success"])
        self.assertNotIn("synthetic-secret", json.dumps(result))

    def test_batch_reports_partial_progress_and_stops(self):
        result = self.act({"kind": "batch", "steps": [
            {"kind": "type", "selector": "#field", "text": "first"},
            {"kind": "click", "selector": "#missing", "timeout_ms": 30},
            {"kind": "type", "selector": "#field", "text": "last"},
        ]})
        self.assertFalse(result["success"])
        self.assertEqual(result["data"]["completed_steps"], 1)
        self.assertEqual(result["data"]["failed_step"], 2)
        self.assertEqual(self.page.locator("#field").input_value(), "first")

    def test_invalid_batches_fail_before_mutating(self):
        for steps in ([], [None], [{"kind": "batch", "steps": []}], [{"kind": "evaluate"}]):
            result = self.act({"kind": "batch", "steps": [
                {"kind": "type", "selector": "#field", "text": "changed"}, *steps
            ]} if steps else {"kind": "batch", "steps": []})
            self.assertFalse(result["success"])
            self.assertEqual(self.page.locator("#field").input_value(), "")

    def test_text_budgets_include_frames_and_unicode(self):
        self.page.set_content("<h1>Sample heading</h1><p>" + "文😀sample " * 1000 + "</p><iframe srcdoc='<p>nested text</p>'></iframe>")
        for budget in (1, 3, 50, 300, 512, 16000):
            result = self.session.read_page(self.target, budget)["data"]
            self.assertLessEqual(len(result["content"]), budget)
            if budget <= 512:
                self.assertTrue(result["truncated"])
        self.page.set_content("<h1>Heading</h1><p>Body</p>")
        content = self.session.read_page(self.target, 300)["data"]["content"]
        self.assertEqual(content.count("Heading"), 1)

    def test_snapshot_is_bounded_and_ref_map_matches_visible_refs(self):
        self.page.set_content("<input type='password' value='synthetic-secret'>" +
                              "<button>Sample action</button>" * 100)
        for budget in (1, 50, 300):
            data = self.session.snapshot(self.target, max_chars=budget)["data"]
            self.assertLessEqual(len(data["snapshot"]), budget)
            self.assertNotIn("synthetic-secret", json.dumps(data))
            for ref in self.session.refs[self.target]:
                self.assertIn("[ref=" + ref + "]", data["snapshot"])

    def test_select_arrays_and_empty_values(self):
        self.assertTrue(self.act({"kind": "select", "selector": "#choice", "value": ["a", "b"]})["success"])
        self.assertEqual(self.page.locator("#choice").evaluate("el => Array.from(el.selectedOptions, o => o.value)"), ["a", "b"])
        self.assertTrue(self.act({"kind": "select", "selector": "#choice", "value": ""})["success"])

    def test_press_key_uses_current_keyboard_focus(self):
        self.page.locator("#field").focus()
        self.assertTrue(self.act({"kind": "press", "key": "Tab"})["success"])
        self.assertEqual(self.page.evaluate("document.activeElement.id"), "secret")

    def test_wait_supports_url_and_selector(self):
        result = self.act({"kind": "wait", "url": "about:*", "timeout_ms": 100})
        self.assertTrue(result["success"])
        self.assertEqual(result["data"]["url"], "about:blank")
        self.assertEqual(result["data"]["expected_url"], "about:*")
        self.assertTrue(self.act({"kind": "wait", "selector": "#send", "timeout_ms": 100})["success"])
        self.assertEqual(self.session._timeout_ms(999999), 8000)

    def test_wait_conditions_override_delay_and_support_hidden_loading(self):
        self.page.evaluate("""() => {
            const el = document.querySelector('#status');
            el.id = 'loading';
            setTimeout(() => el.remove(), 60);
        }""")
        with patch.object(self.page, "wait_for_timeout", side_effect=AssertionError("fixed delay used")):
            result = self.act({"kind": "wait", "selector": "#loading", "state": "hidden",
                               "wait_ms": 2000, "timeout_ms": 1000})
        self.assertTrue(result["success"])
        self.assertEqual(result["data"]["state"], "hidden")

    def test_wait_checks_all_conditions_with_a_shared_timeout(self):
        with patch.object(self.page, "wait_for_url", side_effect=RuntimeError("URL not ready")) as wait_url:
            with self.assertRaisesRegex(RuntimeError, "URL not ready"):
                self.act({"kind": "wait", "selector": "#send", "url": "**/complete",
                          "timeout_ms": 100})
        self.assertEqual(wait_url.call_args.args, ("**/complete",))
        self.assertGreater(wait_url.call_args.kwargs["timeout"], 0)
        self.assertLessEqual(wait_url.call_args.kwargs["timeout"], 100)

    def test_batch_wait_failure_keeps_submission_progress(self):
        self.page.locator("#send").evaluate("el => el.onclick = () => window.submissions = (window.submissions || 0) + 1")
        result = self.act({"kind": "batch", "steps": [
            {"kind": "click", "selector": "#send"},
            {"kind": "wait", "selector": "#missing", "timeout_ms": 30},
            {"kind": "click", "selector": "#send"},
        ]}, include_snapshot=True)
        self.assertFalse(result["success"])
        self.assertEqual(result["data"]["completed_steps"], 1)
        self.assertEqual(result["data"]["failed_step"], 2)
        self.assertEqual(self.page.evaluate("window.submissions"), 1)

    def test_snapshot_waits_for_delayed_client_render_and_syncs_identity(self):
        url = "https://example.test/initial"
        self.page.route(url, lambda route: route.fulfill(content_type="text/html", body="""
          <script>setTimeout(() => {
            history.replaceState({}, '', '/ready'); document.title='Ready page';
            document.body.innerHTML='<input placeholder="Sample field"><button>Continue</button>';
          }, 120)</script>"""))
        try:
            result = self.session.handle({"action": "navigate", "url": url,
                                          "target_id": self.target, "include_snapshot": True,
                                          "max_chars": 1000, "snapshot_timeout_ms": 1000})
            self.assertTrue(result["success"])
            data = result["data"]
            self.assertIn("Sample field", data["snapshot"])
            self.assertEqual(data["url"], "https://example.test/ready")
            self.assertEqual(data["title"], "Ready page")
            self.assertEqual(data["tabs"][0]["url"], data["url"])
            self.assertTrue(self.act({"kind": "type", "ref": "e1", "text": "sample"})["success"])
        finally:
            self.page.unroute(url)

    def test_snapshot_empty_wait_is_bounded_and_can_be_disabled(self):
        self.page.set_content("<div></div>")
        with patch.object(self.page, "wait_for_timeout", side_effect=AssertionError("unexpected delay")):
            data = self.session.snapshot(self.target, ready_timeout_ms=0)["data"]
        self.assertEqual(data["snapshot"], "")
        started = time.monotonic()
        data = self.session.snapshot(self.target, ready_timeout_ms=80)["data"]
        self.assertEqual(data["snapshot"], "")
        self.assertLess(time.monotonic() - started, 1)
        self.page.set_content("<button>Ready</button>")
        with patch.object(self.page, "wait_for_timeout", side_effect=AssertionError("populated snapshot delayed")):
            self.assertIn("Ready", self.session.snapshot(self.target)["data"]["snapshot"])

    def test_snapshot_retries_transient_context_loss_only(self):
        collect = self.session._collect
        with patch.object(self.session, "_collect", side_effect=[
            RuntimeError("Execution context was destroyed"), collect(self.page, "role", True, True, 1000),
        ]):
            self.assertIn("Sample form", self.session.snapshot(self.target)["data"]["snapshot"])
        with patch.object(self.session, "_collect", side_effect=RuntimeError("unavailable")) as failed:
            with self.assertRaisesRegex(RuntimeError, "unavailable"):
                self.session.snapshot(self.target)
            self.assertEqual(failed.call_count, 1)
        self.assertEqual(self.session.refs[self.target], {})

    def test_observation_updates_url_and_tabs_after_action(self):
        url = "https://example.test/start"
        self.page.route(url, lambda route: route.fulfill(content_type="text/html", body="<button>Continue</button>"))
        try:
            self.page.goto(url)
            original = self.session.snapshot
            def redirected_snapshot(**kwargs):
                self.page.evaluate("history.pushState({}, '', '/complete'); document.title='Complete page'")
                return original(**kwargs)
            with patch.object(self.session, "snapshot", side_effect=redirected_snapshot):
                result = self.act({"kind": "click", "selector": "button"}, include_snapshot=True)
            self.assertTrue(result["success"])
            data = result["data"]
            self.assertEqual(data["url"], "https://example.test/complete")
            self.assertEqual(data["title"], "Complete page")
            self.assertEqual(data["tabs"][0]["url"], data["url"])
        finally:
            self.page.unroute(url)

    def test_observation_failure_does_not_mark_successful_action_failed(self):
        with patch.object(self.session, "snapshot", side_effect=RuntimeError("unavailable")):
            result = self.act({"kind": "click", "selector": "#send"}, include_snapshot=True)
        self.assertTrue(result["success"])
        self.assertIn("snapshot_error", result["data"])

    def test_navigation_and_batch_need_only_two_round_trips(self):
        url = "https://example.test/form"
        self.page.route(url, lambda route: route.fulfill(content_type="text/html", body="""
          <input id="sample" placeholder="Sample field"><button id="send" onclick="
            history.pushState({}, '', '/complete'); document.body.innerHTML='<h1>Complete</h1>'">Send</button>"""))
        try:
            navigation = self.session.handle({"action": "navigate", "url": url,
                                              "target_id": self.target, "include_snapshot": True,
                                              "max_chars": 1000})
            self.assertTrue(navigation["success"])
            self.assertIn("[ref=e1]", navigation["data"]["snapshot"])
            result = self.act({"kind": "batch", "steps": [
                {"kind": "type", "ref": "e1", "text": "sample-value"},
                {"kind": "click", "ref": "e2"},
                {"kind": "wait", "url": "**/complete", "timeout_ms": 1000},
            ]}, include_snapshot=True, max_chars=1000)
            self.assertTrue(result["success"])
            self.assertIn("Complete", result["data"]["snapshot"])
            self.assertEqual(result["data"]["url"], "https://example.test/complete")
            self.assertEqual(result["data"]["results"][-1]["url"], result["data"]["url"])
        finally:
            self.page.unroute(url)

    def test_multi_frame_budget_accounts_for_separators(self):
        self.page.set_content("<p>main</p><iframe srcdoc='<p>nested sample</p>'></iframe>")
        self.page.frame_locator("iframe").locator("p").wait_for()
        result = self.session.read_page(self.target, 300)["data"]
        self.assertIn("nested sample", result["content"])
        for budget in range(1, 80):
            data = self.session.read_page(self.target, budget)["data"]
            self.assertLessEqual(len(data["content"]), budget)

    def test_screenshot_returns_image_payload(self):
        data = self.session.screenshot(self.target)["data"]
        self.assertTrue(data["image_base64"].startswith("iVBOR"))
        self.assertEqual(data["format"], "png")

    def test_open_shadow_roots_remain_readable_and_interactive(self):
        self.page.set_content("<div id='host'></div>")
        self.page.locator("#host").evaluate("el => el.attachShadow({mode:'open'}).innerHTML='<p>Shadow sample</p><button>Shadow action</button>'")
        data = self.session.read_page(self.target, 300)["data"]
        self.assertIn("Shadow sample", data["content"])
        snapshot = self.session.snapshot(self.target, max_chars=300)["data"]["snapshot"]
        self.assertIn("Shadow action", snapshot)
        self.assertTrue(self.act({"kind": "click", "ref": "e1"})["success"])


if __name__ == "__main__":
    unittest.main()
