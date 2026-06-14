#!/usr/bin/env python3
"""Wunder browser bridge over JSON-line stdio.

This bridge intentionally keeps the runtime side in Python/Playwright for now,
while the Rust side owns session/tab orchestration and public contracts.
"""

from __future__ import annotations

import argparse
import base64
import json
import sys
import traceback
from collections import OrderedDict


def respond(data):
    sys.stdout.write(json.dumps(data, ensure_ascii=False) + "\n")
    sys.stdout.flush()


class BridgeSession:
    def __init__(self, args):
        self.args = args
        self.timeout_ms = max(args.timeout, 1) * 1000
        try:
            from playwright.sync_api import sync_playwright
        except Exception:
            respond(
                {
                    "success": False,
                    "error": "playwright not installed. Run: pip install playwright && playwright install chromium",
                }
            )
            raise SystemExit(0)
        self.pw = sync_playwright().start()
        launch_args = [item for item in args.launch_arg if item]
        self.browser = self.pw.chromium.launch(
            headless=args.headless,
            args=launch_args or None,
        )
        self.context = self.browser.new_context(
            viewport={"width": args.width, "height": args.height},
            accept_downloads=True,
            user_agent=(
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 "
                "(KHTML, like Gecko) Chrome/135.0.0.0 Safari/537.36"
            ),
        )
        self.pages = OrderedDict()
        self.refs = {}
        self.target_counter = 0
        self.active_target_id = None
        target_id, _ = self._new_page()
        self.active_target_id = target_id

    def close(self):
        try:
            self.context.close()
        except Exception:
            pass
        try:
            self.browser.close()
        except Exception:
            pass
        try:
            self.pw.stop()
        except Exception:
            pass

    def handle(self, raw):
        action = str(raw.get("action", "")).strip().lower()
        if action in ("start", "status"):
            return self.status()
        if action in ("tabs", "list_tabs"):
            return self.list_tabs()
        if action in ("open", "open_tab"):
            return self.open_tab(raw.get("url"))
        if action in ("focus", "focus_tab"):
            return self.focus_tab(raw.get("target_id"))
        if action in ("close", "close_tab"):
            target_id = raw.get("target_id")
            if target_id:
                return self.close_tab(target_id)
            return {"success": True, "data": {"status": "closing"}}
        if action == "navigate":
            return self.navigate(raw.get("url"), raw.get("target_id"))
        if action == "snapshot":
            return self.snapshot(
                target_id=raw.get("target_id"),
                fmt=raw.get("format") or "role",
                interactive=bool(raw.get("interactive", True)),
                compact=bool(raw.get("compact", True)),
                max_chars=int(raw.get("max_chars") or 40000),
            )
        if action == "act":
            return self.act(raw.get("request") or {}, raw.get("target_id"))
        if action in ("click", "type", "press", "hover", "wait"):
            request = dict(raw)
            request["kind"] = action
            return self.act(request, raw.get("target_id"))
        if action == "screenshot":
            return self.screenshot(
                target_id=raw.get("target_id"),
                full_page=bool(raw.get("full_page", False)),
            )
        if action in ("read_page", "readpage", "read"):
            return self.read_page(
                target_id=raw.get("target_id"),
                max_chars=int(raw.get("max_chars") or 40000),
            )
        return {"success": False, "error": f"Unknown action: {action}"}

    def status(self):
        return {
            "success": True,
            "data": {
                "status": "ready",
                "tabs": self._tabs_state(),
                "active_target_id": self.active_target_id,
            },
        }

    def list_tabs(self):
        return {
            "success": True,
            "data": {
                "tabs": self._tabs_state(),
                "active_target_id": self.active_target_id,
            },
        }

    def open_tab(self, url=None):
        if len(self.pages) >= max(int(self.args.max_tabs), 1):
            return {
                "success": False,
                "error": f"Browser session reached max tabs limit ({self.args.max_tabs})",
            }
        target_id, page = self._new_page()
        if url:
            page.goto(url, wait_until="domcontentloaded", timeout=self.timeout_ms)
        return {
            "success": True,
            "data": {
                "target_id": target_id,
                "title": page.title(),
                "url": page.url,
                "tabs": self._tabs_state(),
                "active_target_id": self.active_target_id,
            },
        }

    def focus_tab(self, target_id):
        page, target_id = self._resolve_page(target_id)
        self.active_target_id = target_id
        page.bring_to_front()
        return {
            "success": True,
            "data": {
                "target_id": target_id,
                "title": page.title(),
                "url": page.url,
                "tabs": self._tabs_state(),
                "active_target_id": self.active_target_id,
            },
        }

    def close_tab(self, target_id):
        page, target_id = self._resolve_page(target_id)
        try:
            page.close()
        except Exception:
            pass
        self.pages.pop(target_id, None)
        self.refs.pop(target_id, None)
        if not self.pages:
            new_target_id, _ = self._new_page()
            self.active_target_id = new_target_id
        elif self.active_target_id == target_id:
            self.active_target_id = next(iter(self.pages))
        return {
            "success": True,
            "data": {
                "closed": True,
                "target_id": target_id,
                "tabs": self._tabs_state(),
                "active_target_id": self.active_target_id,
            },
        }

    def navigate(self, url, target_id=None):
        if not url:
            return {"success": False, "error": "Missing 'url' parameter"}
        page, target_id = self._resolve_page(target_id)
        page.goto(url, wait_until="domcontentloaded", timeout=self.timeout_ms)
        self.refs[target_id] = {}
        return {
            "success": True,
            "data": {
                "target_id": target_id,
                "title": page.title(),
                "url": page.url,
                "content": extract_readable(page, 40000),
                "tabs": self._tabs_state(),
            },
        }

    def snapshot(self, target_id=None, fmt="role", interactive=True, compact=True, max_chars=40000):
        page, target_id = self._resolve_page(target_id)
        payload = build_snapshot(page, fmt=fmt, interactive=interactive, compact=compact)
        refs_map = {}
        for item in payload["refs"]:
            refs_map[item["ref"]] = item
        self.refs[target_id] = refs_map
        text = "\n".join(payload["lines"]).strip()
        truncated = False
        if len(text) > max_chars:
            text = text[:max_chars].rstrip() + "\n\n[Truncated]"
            truncated = True
        return {
            "success": True,
            "data": {
                "target_id": target_id,
                "title": page.title(),
                "url": page.url,
                "format": fmt,
                "snapshot": text,
                "refs": refs_map,
                "stats": {
                    "lines": len(payload["lines"]),
                    "chars": len(text),
                    "refs": len(refs_map),
                    "interactive": payload["interactive"],
                },
                "truncated": truncated,
            },
        }

    def act(self, request, target_id=None):
        if not isinstance(request, dict):
            return {"success": False, "error": "Browser act 'request' must be an object"}
        kind = str(request.get("kind", "")).strip().lower()
        if not kind:
            return {"success": False, "error": "Browser act request missing 'kind'"}
        page, target_id = self._resolve_page(target_id or request.get("target_id"))
        if kind == "batch":
            steps = request.get("steps") or request.get("actions") or request.get("items") or []
            if not isinstance(steps, list):
                return {"success": False, "error": "Browser batch 'steps' must be an array"}
            if len(steps) > 10:
                return {"success": False, "error": "Browser batch supports at most 10 steps"}
            results = []
            for step in steps:
                result = self.act(step, target_id)
                if not result.get("success"):
                    return result
                results.append(result.get("data"))
            return {
                "success": True,
                "data": {"target_id": target_id, "results": results, "url": page.url},
            }
        if kind == "wait":
            return self._wait(page, target_id, request)
        if kind == "evaluate":
            expression = request.get("script") or request.get("expression")
            if not expression:
                return {"success": False, "error": "Browser evaluate missing 'script' or 'expression'"}
            result = page.evaluate(expression)
            return {
                "success": True,
                "data": {"target_id": target_id, "result": result, "url": page.url},
            }
        if kind == "close":
            return self.close_tab(target_id)
        locator = self._resolve_locator(page, target_id, request)
        timeout_ms = int(request.get("timeout_ms") or self.timeout_ms)
        if kind == "click":
            if bool(request.get("double_click", False)):
                locator.dblclick(timeout=timeout_ms)
            else:
                locator.click(timeout=timeout_ms)
            return self._action_result(page, target_id, kind)
        if kind in ("type", "fill"):
            text = request.get("text")
            if text is None:
                return {"success": False, "error": "Browser type/fill missing 'text'"}
            locator.fill(str(text), timeout=timeout_ms)
            return self._action_result(page, target_id, kind, extra={"text": text})
        if kind == "press":
            key = request.get("key")
            if not key:
                return {"success": False, "error": "Browser press missing 'key'"}
            locator.press(str(key), timeout=timeout_ms)
            return self._action_result(page, target_id, kind, extra={"key": key})
        if kind == "hover":
            locator.hover(timeout=timeout_ms)
            return self._action_result(page, target_id, kind)
        if kind == "select":
            value = request.get("value") or request.get("text")
            if value is None:
                return {"success": False, "error": "Browser select missing 'value' or 'text'"}
            locator.select_option(value=str(value), timeout=timeout_ms)
            return self._action_result(page, target_id, kind, extra={"value": value})
        if kind == "drag":
            target_locator = self._resolve_locator(
                page,
                target_id,
                {
                    "ref": request.get("to_ref"),
                    "selector": request.get("to_selector"),
                    "text": request.get("to_text"),
                },
            )
            locator.drag_to(target_locator, timeout=timeout_ms)
            return self._action_result(page, target_id, kind)
        return {"success": False, "error": f"Unsupported browser act kind: {kind}"}

    def screenshot(self, target_id=None, full_page=False):
        page, target_id = self._resolve_page(target_id)
        content = page.screenshot(full_page=full_page)
        return {
            "success": True,
            "data": {
                "target_id": target_id,
                "url": page.url,
                "format": "png",
                "image_base64": base64.b64encode(content).decode("utf-8"),
                "full_page": full_page,
            },
        }

    def read_page(self, target_id=None, max_chars=40000):
        page, target_id = self._resolve_page(target_id)
        content = extract_readable(page, max_chars)
        return {
            "success": True,
            "data": {
                "target_id": target_id,
                "title": page.title(),
                "url": page.url,
                "content": content,
            },
        }

    def _new_page(self):
        self.target_counter += 1
        target_id = f"tab-{self.target_counter}"
        page = self.context.new_page()
        page.set_default_timeout(self.timeout_ms)
        page.set_default_navigation_timeout(self.timeout_ms)
        self.pages[target_id] = page
        self.refs[target_id] = {}
        self.active_target_id = target_id
        try:
            page.bring_to_front()
        except Exception:
            pass
        return target_id, page

    def _resolve_page(self, target_id=None):
        if target_id:
            target_id = str(target_id)
            page = self.pages.get(target_id)
            if page is not None:
                self.active_target_id = target_id
                return page, target_id
            raise ValueError(f"Unknown browser target_id: {target_id}")
        if self.active_target_id and self.active_target_id in self.pages:
            return self.pages[self.active_target_id], self.active_target_id
        if not self.pages:
            return self._new_page()
        target_id = next(iter(self.pages))
        self.active_target_id = target_id
        return self.pages[target_id], target_id

    def _tabs_state(self):
        tabs = []
        for target_id, page in self.pages.items():
            tabs.append(
                {
                    "target_id": target_id,
                    "title": safe_page_title(page),
                    "url": safe_page_url(page),
                    "active": target_id == self.active_target_id,
                }
            )
        return tabs

    def _resolve_locator(self, page, target_id, request):
        ref_id = request.get("ref")
        if ref_id:
            ref = self.refs.get(target_id, {}).get(str(ref_id))
            if ref is None:
                raise ValueError(f"Unknown browser ref: {ref_id}. Capture a new snapshot first.")
            selector = ref.get("selector")
            if not selector:
                raise ValueError(f"Browser ref {ref_id} has no selector mapping")
            return page.locator(selector).first
        selector = request.get("selector")
        if selector:
            return page.locator(str(selector)).first
        text = request.get("text")
        if text and str(request.get("kind", "")).lower() in ("click", "hover"):
            return page.get_by_text(str(text), exact=False).first
        raise ValueError("Browser action requires 'ref' or 'selector'")

    def _wait(self, page, target_id, request):
        timeout_ms = int(request.get("timeout_ms") or self.timeout_ms)
        if request.get("wait_ms") is not None:
            page.wait_for_timeout(int(request.get("wait_ms")))
            return self._action_result(page, target_id, "wait", extra={"wait_ms": request.get("wait_ms")})
        selector = request.get("selector")
        if selector:
            page.locator(str(selector)).first.wait_for(state="visible", timeout=timeout_ms)
            return self._action_result(page, target_id, "wait", extra={"selector": selector})
        text = request.get("text")
        if text:
            page.get_by_text(str(text), exact=False).first.wait_for(timeout=timeout_ms)
            return self._action_result(page, target_id, "wait", extra={"text": text})
        url = request.get("url")
        if url:
            page.wait_for_url(str(url), timeout=timeout_ms)
            return self._action_result(page, target_id, "wait", extra={"url": url})
        load_state = request.get("load_state")
        if load_state:
            page.wait_for_load_state(str(load_state), timeout=timeout_ms)
            return self._action_result(page, target_id, "wait", extra={"load_state": load_state})
        return {"success": False, "error": "Browser wait requires one of wait_ms/selector/text/url/load_state"}

    def _action_result(self, page, target_id, kind, extra=None):
        payload = {
            "target_id": target_id,
            "kind": kind,
            "title": safe_page_title(page),
            "url": safe_page_url(page),
            "tabs": self._tabs_state(),
            "active_target_id": self.active_target_id,
        }
        if extra:
            payload.update(extra)
        return {"success": True, "data": payload}


def safe_page_title(page):
    try:
        return page.title()
    except Exception:
        return ""


def safe_page_url(page):
    try:
        return page.url
    except Exception:
        return ""


def build_snapshot(page, fmt="role", interactive=True, compact=True):
    result = page.evaluate(
        """({fmt, interactive, compact}) => {
        function isVisible(el) {
            if (!(el instanceof HTMLElement)) return false;
            const style = window.getComputedStyle(el);
            if (style.display === 'none' || style.visibility === 'hidden') return false;
            const rect = el.getBoundingClientRect();
            return rect.width > 0 && rect.height > 0;
        }
        function cssPath(el) {
            if (!(el instanceof HTMLElement)) return '';
            if (el.id) return '#' + CSS.escape(el.id);
            const parts = [];
            let node = el;
            while (node && node instanceof HTMLElement && node !== document.body) {
                let part = node.tagName.toLowerCase();
                if (node.classList.length > 0) {
                    const classes = Array.from(node.classList)
                        .filter(Boolean)
                        .slice(0, 2)
                        .map((name) => '.' + CSS.escape(name))
                        .join('');
                    part += classes;
                }
                const parent = node.parentElement;
                if (parent) {
                    const siblings = Array.from(parent.children).filter(
                        (child) => child.tagName === node.tagName
                    );
                    if (siblings.length > 1) {
                        part += `:nth-of-type(${siblings.indexOf(node) + 1})`;
                    }
                }
                parts.unshift(part);
                node = parent;
            }
            return parts.join(' > ');
        }
        function roleOf(el) {
            const explicit = el.getAttribute('role');
            if (explicit) return explicit;
            const tag = el.tagName.toLowerCase();
            if (tag === 'a') return 'link';
            if (tag === 'button') return 'button';
            if (tag === 'input') {
                const type = (el.getAttribute('type') || 'text').toLowerCase();
                if (['submit', 'button', 'reset'].includes(type)) return 'button';
                if (type === 'checkbox') return 'checkbox';
                if (type === 'radio') return 'radio';
                return 'textbox';
            }
            if (tag === 'textarea') return 'textbox';
            if (tag === 'select') return 'combobox';
            if (tag === 'option') return 'option';
            if (tag === 'img') return 'image';
            return tag;
        }
        function nameOf(el) {
            const attrs = [
                el.getAttribute('aria-label'),
                el.getAttribute('alt'),
                el.getAttribute('title'),
                el.getAttribute('placeholder'),
            ].filter(Boolean);
            if (attrs.length > 0) return attrs[0].trim();
            if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement || el instanceof HTMLSelectElement) {
                const value = el.value || el.getAttribute('value');
                if (value) return String(value).trim();
            }
            const text = (el.innerText || el.textContent || '').replace(/\\s+/g, ' ').trim();
            return text.slice(0, 200);
        }
        function lineFor(format, role, name, ref) {
            const safeName = name || role;
            if (format === 'aria') {
                return `- role=${role} name=\\"${safeName}\\" [ref=${ref}]`;
            }
            if (format === 'ai') {
                return `${role} \\"${safeName}\\" [ref=${ref}]`;
            }
            return `- ${role} \\"${safeName}\\" [ref=${ref}]`;
        }
        function isInteractive(el) {
            if (!(el instanceof HTMLElement)) return false;
            if (el.matches('a[href], button, textarea, select, summary')) return true;
            if (el.matches('input:not([type=hidden])')) return true;
            if (el.hasAttribute('contenteditable')) return true;
            if (el.hasAttribute('role') && ['button','link','textbox','checkbox','radio','tab','switch','menuitem'].includes((el.getAttribute('role') || '').toLowerCase())) {
                return true;
            }
            const tabindex = el.getAttribute('tabindex');
            return tabindex !== null && tabindex !== '-1';
        }
        const lines = [];
        const refs = [];
        let interactiveCount = 0;
        let refCounter = 0;
        const seenTexts = new Set();
        const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_ELEMENT);
        while (walker.nextNode()) {
            const node = walker.currentNode;
            if (!(node instanceof HTMLElement) || !isVisible(node)) continue;
            const role = roleOf(node);
            const name = nameOf(node);
            if (isInteractive(node)) {
                if (!interactive) continue;
                refCounter += 1;
                const ref = `e${refCounter}`;
                refs.push({
                    ref,
                    role,
                    name,
                    selector: cssPath(node),
                    tag: node.tagName.toLowerCase(),
                });
                lines.push(lineFor(fmt, role, name, ref));
                interactiveCount += 1;
                continue;
            }
            if (node.matches('h1, h2, h3, h4, h5, h6')) {
                if (name && !seenTexts.has(name)) {
                    lines.push('## ' + name);
                    seenTexts.add(name);
                }
                continue;
            }
            if (!compact && node.matches('p, li, td, th, label')) {
                if (name && !seenTexts.has(name)) {
                    lines.push(name);
                    seenTexts.add(name);
                }
                continue;
            }
            if (compact && node.matches('main, article') && name && !seenTexts.has(name)) {
                lines.push(name);
                seenTexts.add(name);
            }
        }
        return { lines, refs, interactive: interactiveCount };
    }""",
        {"fmt": fmt, "interactive": interactive, "compact": compact},
    )
    return result


def extract_readable(page, max_chars=40000):
    try:
        content = page.evaluate(
            """() => {
            const clone = document.body.cloneNode(true);
            const remove = ['script', 'style', 'nav', 'footer', 'header', 'aside', 'iframe', 'noscript', 'svg', 'canvas'];
            remove.forEach((tag) => clone.querySelectorAll(tag).forEach((el) => el.remove()));
            const main = clone.querySelector('main, article, [role="main"], .content, #content');
            const source = main || clone;
            const lines = [];
            const walker = document.createTreeWalker(source, NodeFilter.SHOW_ELEMENT | NodeFilter.SHOW_TEXT);
            while (walker.nextNode()) {
                const node = walker.currentNode;
                if (node.nodeType === Node.TEXT_NODE) {
                    const text = (node.textContent || '').replace(/\\s+/g, ' ').trim();
                    if (text) lines.push(text);
                    continue;
                }
                if (!(node instanceof HTMLElement)) continue;
                if (/^H[1-6]$/.test(node.tagName)) {
                    const text = (node.innerText || '').replace(/\\s+/g, ' ').trim();
                    if (text) lines.push('## ' + text);
                } else if (node.tagName === 'LI') {
                    const text = (node.innerText || '').replace(/\\s+/g, ' ').trim();
                    if (text) lines.push('- ' + text);
                }
            }
            return lines.join('\\n').replace(/\\n{3,}/g, '\\n\\n').trim();
        }"""
        )
        if len(content) > max_chars:
            return content[:max_chars].rstrip() + f"\\n\\n[Truncated - {len(content)} total chars]"
        return content
    except Exception:
        text = page.inner_text("body")
        if len(text) > max_chars:
            return text[:max_chars].rstrip() + f"\\n\\n[Truncated - {len(text)} total chars]"
        return text


def main():
    parser = argparse.ArgumentParser(description="Wunder Browser Bridge")
    parser.add_argument("--headless", action="store_true", default=True)
    parser.add_argument("--no-headless", dest="headless", action="store_false")
    parser.add_argument("--width", type=int, default=1280)
    parser.add_argument("--height", type=int, default=720)
    parser.add_argument("--max-tabs", type=int, default=8)
    parser.add_argument("--timeout", type=int, default=30)
    parser.add_argument("--launch-arg", action="append", default=[])
    args = parser.parse_args()

    try:
        session = BridgeSession(args)
    except Exception as exc:
        respond(
            {
                "success": False,
                "error": f"{type(exc).__name__}: {exc}",
                "traceback": traceback.format_exc(limit=6),
                "phase": "startup",
            }
        )
        return 1
    respond(
        {
            "success": True,
            "data": {
                "status": "ready",
                "tabs": session._tabs_state(),
                "active_target_id": session.active_target_id,
            },
        }
    )
    try:
        for line in sys.stdin:
            line = line.strip()
            if not line:
                continue
            action = ""
            try:
                command = json.loads(line)
                action = str(command.get("action", "")).strip().lower()
                result = session.handle(command)
                respond(result)
                if action == "close" and not command.get("target_id"):
                    break
            except SystemExit:
                break
            except Exception as exc:
                respond(
                    {
                        "success": False,
                        "error": f"{type(exc).__name__}: {exc}",
                        "traceback": traceback.format_exc(limit=2),
                    }
                )
                if action == "close":
                    break
    finally:
        session.close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
