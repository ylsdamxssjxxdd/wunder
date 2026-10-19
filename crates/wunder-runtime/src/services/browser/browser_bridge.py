# AI生成
#!/usr/bin/env python3
"""Wunder browser bridge over JSON-line stdio.

The runtime side stays in Python/Playwright while the Rust side owns
session/tab orchestration and public contracts. Three things are enforced here
rather than only in Rust, because a browser can always reach a URL the caller
never named, and can always show things the outer document does not contain:

* the URL policy is applied to every request the browser makes (redirects,
  subresources, script-driven fetch/navigation, popups), not just the entry URL;
* the snapshot and page read cover every frame Playwright can reach plus the
  open shadow roots inside each frame, and every ref records the frame it came
  from so the action can address it again;
* popups and `window.open` targets are adopted into the tab list, so a tab the
  model can see is also a tab it can focus and close.
"""

from __future__ import annotations

import argparse
import base64
import ipaddress
import json
import os
import re
import socket
import sys
import time
import traceback
from collections import OrderedDict
from pathlib import Path
from urllib.parse import urlparse

# Runs inside one frame: open shadow roots are traversed here because they share
# the frame's document; nested frames are handled by the caller so each gets its
# own execution context and selector namespace.
SNAPSHOT_JS = """(opts) => {
  const lines = [];
  const refs = [];
  let localCounter = 0;
  const seenTexts = new Set();
  let used = 0;
  let visited = 0;
  let truncated = false;
  function push(line) {
    const cost = line.length + (lines.length ? 1 : 0);
    if (used + cost > opts.max_chars) { truncated = true; return false; }
    lines.push(line); used += cost; return true;
  }

  const escape = (value) => (window.CSS && CSS.escape ? CSS.escape(value) : String(value));

  function cssPath(el, stopNode) {
    const parts = [];
    let node = el;
    while (node && node.nodeType === Node.ELEMENT_NODE && node !== stopNode) {
      if (node.id) {
        parts.unshift('#' + escape(node.id));
        break;
      }
      let part = node.tagName.toLowerCase();
      if (node.classList.length > 0) {
        const classes = Array.from(node.classList)
          .filter(Boolean)
          .slice(0, 2)
          .map((name) => '.' + escape(name))
          .join('');
        part += classes;
      }
      const parent = node.parentElement;
      if (parent) {
        const siblings = Array.from(parent.children).filter(
          (child) => child.tagName === node.tagName
        );
        if (siblings.length > 1) {
          part += ':nth-of-type(' + (siblings.indexOf(node) + 1) + ')';
        }
      }
      parts.unshift(part);
      node = parent;
    }
    return parts.join(' > ');
  }

  function isVisible(el) {
    if (!(el instanceof HTMLElement)) return false;
    let style = null;
    try {
      style = window.getComputedStyle(el);
    } catch (e) {
      return false;
    }
    if (!style || style.display === 'none' || style.visibility === 'hidden') return false;
    const rect = el.getBoundingClientRect();
    return rect.width > 0 && rect.height > 0;
  }

  function roleOf(el) {
    const explicit = el.getAttribute && el.getAttribute('role');
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
    if (attrs.length > 0) return String(attrs[0]).trim().slice(0, 200);
    if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement || el instanceof HTMLSelectElement) {
      // Names remain stable after filling; values (including passwords) are
      // user data, never a fallback accessibility label.
      const label = el.labels && el.labels[0];
      if (label) return String(label.innerText || label.textContent || '').trim().slice(0, 200);
      if (el instanceof HTMLInputElement) {
        if (['submit', 'button', 'reset'].includes(el.type)) return String(el.value || el.type).slice(0, 200);
        return el.type === 'password' ? 'password' : 'textbox';
      }
      if (el instanceof HTMLTextAreaElement) return 'textbox';
      return 'select';
    }
    const text = (el.innerText || el.textContent || '').replace(/\\s+/g, ' ').trim();
    return text.slice(0, 200);
  }

  function isInteractive(el) {
    if (!(el instanceof HTMLElement)) return false;
    if (el.matches('a[href], button, textarea, select, summary')) return true;
    if (el.matches('input:not([type=hidden])')) return true;
    if (el.hasAttribute('contenteditable')) return true;
    const role = (el.getAttribute('role') || '').toLowerCase();
    if (['button', 'link', 'textbox', 'checkbox', 'radio', 'tab', 'switch', 'menuitem'].includes(role)) {
      return true;
    }
    const tabindex = el.getAttribute('tabindex');
    return tabindex !== null && tabindex !== '-1';
  }

  function lineFor(format, role, name, ref) {
    const safeName = name || role;
    if (format === 'aria') {
      return '- role=' + role + ' name="' + safeName + '" [ref=' + ref + ']';
    }
    if (format === 'ai') {
      return role + ' "' + safeName + '" [ref=' + ref + ']';
    }
    return '- ' + role + ' "' + safeName + '" [ref=' + ref + ']';
  }

  // selectorPrefix carries the shadow-host chain (`>>>`), because Playwright's
  // CSS engine pierces open shadow roots but a bare path is ambiguous inside
  // them.
  function walkRoot(root, stopNode, selectorPrefix, depth) {
    if (!root || depth > 8) return;
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_ELEMENT);
    while (walker.nextNode()) {
      if (truncated || ++visited > 12000) { truncated = true; return; }
      const node = walker.currentNode;
      if (!(node instanceof HTMLElement) || !isVisible(node)) continue;
      const tag = node.tagName.toLowerCase();
      const interactive = isInteractive(node);
      const needsSelector = interactive || node.shadowRoot || tag === 'iframe' || tag === 'frame';
      const own = needsSelector ? cssPath(node, stopNode) : '';
      const selector = selectorPrefix ? selectorPrefix + ' >>> ' + own : own;

      if (tag === 'iframe' || tag === 'frame') {
        const src = node.getAttribute('src') || '';
        if (seenTexts.has('frame:' + src + ':' + selector)) continue;
        seenTexts.add('frame:' + src + ':' + selector);
        push('## nested frame (listed separately) src=' + (src || 'about:blank'));
        continue;
      }

      const role = roleOf(node);
      const needsName = interactive || /^H[1-6]$/.test(node.tagName) ||
        (!opts.compact && node.matches('p, li, td, th, label'));
      const name = needsName ? nameOf(node) : '';
      if (interactive) {
        if (opts.interactive) {
          localCounter += 1;
          const ref = 'e' + (opts.ref_start + localCounter);
          if (!push(lineFor(opts.fmt, role, name, ref))) return;
          refs.push({
            ref: ref,
            role: role,
            name: name,
            selector: selector,
            tag: tag,
          });
        }
      } else if (/^H[1-6]$/.test(node.tagName)) {
        if (name && !seenTexts.has(name)) {
          push('## ' + name);
          seenTexts.add(name);
        }
      } else if (!opts.compact && node.matches('p, li, td, th, label')) {
        if (name && !seenTexts.has(name)) {
          push(name);
          seenTexts.add(name);
        }
      }

      if (node.shadowRoot) {
        walkRoot(node.shadowRoot, node.shadowRoot, selector, depth + 1);
      }
    }
  }

  if (document.body) walkRoot(document.body, document.body, '', 0);
  return { lines: lines, refs: refs, count: localCounter, truncated: truncated };
}"""

# Selector of the element handed in as an argument, used to address the frame
# host element from its parent frame.
HOST_PATH_JS = """(el) => {
  const escape = (value) => (window.CSS && CSS.escape ? CSS.escape(value) : String(value));
  const parts = [];
  let node = el;
  while (node && node.nodeType === Node.ELEMENT_NODE) {
    if (node.id) {
      parts.unshift('#' + escape(node.id));
      break;
    }
    let part = node.tagName.toLowerCase();
    const parent = node.parentElement;
    if (parent) {
      const siblings = Array.from(parent.children).filter(
        (child) => child.tagName === node.tagName
      );
      if (siblings.length > 1) {
        part += ':nth-of-type(' + (siblings.indexOf(node) + 1) + ')';
      }
    }
    parts.unshift(part);
    node = parent;
  }
  return parts.join(' > ');
}"""

READABLE_JS = """(maxChars) => {
  const lines = [];
  const skip = new Set(['SCRIPT', 'STYLE', 'NOSCRIPT', 'TEMPLATE', 'SVG', 'CANVAS', 'IFRAME', 'FRAME']);
  const seen = new Set();
  let used = 0;
  let visited = 0;
  let truncated = false;

  function push(text) {
    const value = (text || '').replace(/\\s+/g, ' ').trim();
    if (!value || seen.has(value)) return;
    const remaining = maxChars - used - (lines.length ? 1 : 0);
    if (remaining <= 0) { truncated = true; return; }
    seen.add(value);
    const part = value.slice(0, remaining);
    used += part.length + (lines.length ? 1 : 0);
    lines.push(part);
    if (part.length < value.length) truncated = true;
  }

  function walk(node, depth) {
    if (!node || truncated) return;
    if (depth > 128) { truncated = true; return; }
    if (++visited > 12000) { truncated = true; return; }
    if (node.nodeType === Node.TEXT_NODE) {
      push(node.textContent);
      return;
    }
    if (node.nodeType === Node.DOCUMENT_FRAGMENT_NODE) {
      for (const child of node.childNodes) { if (truncated) break; walk(child, depth + 1); }
      return;
    }
    if (node.nodeType !== Node.ELEMENT_NODE) return;
    const el = node;
    if (skip.has(el.tagName)) return;
    const isHeading = /^H[1-6]$/.test(el.tagName);
    if (isHeading) { push('## ' + (el.innerText || el.textContent || '')); return; }
    else if (el.tagName === 'LI') { push('- ' + (el.innerText || el.textContent || '')); return; }
    else if (el.childNodes && el.childNodes.length === 0) push(el.innerText || el.textContent || '');
    if (el.shadowRoot) walk(el.shadowRoot, depth + 1);
    for (const child of el.childNodes || []) { if (truncated) break; walk(child, depth + 1); }
  }

  const main = document.querySelector('main, article, [role="main"], .content, #content');
  walk(main || document.body, 0);
  const joined = lines.join('\\n').replace(/\\n{3,}/g, '\\n\\n').trim();
  return {content: joined, truncated: truncated};
}"""


class Policy:
    """URL policy mirrored from the Rust runtime and applied inside the browser."""

    def __init__(self, args):
        self.allow_private_network = bool(args.allow_private_network)
        self.allow_file_scheme = not bool(args.deny_file_scheme)
        self.allow_hosts = {
            str(item).strip().lower() for item in (args.allow_host or []) if str(item).strip()
        }
        # Interception is installed only when the policy can actually refuse a
        # request: with private network access allowed there is nothing left to
        # gate at the network layer (scheme and file:// checks stay at the entry),
        # so the local workbench keeps page loads at full speed.
        self.restrictive = not self.allow_private_network
        self._dns_cache = OrderedDict()

    @staticmethod
    def _ip_private(raw):
        try:
            ip = ipaddress.ip_address(raw)
        except ValueError:
            return False
        if ip.version == 6 and ip.ipv4_mapped is not None:
            ip = ip.ipv4_mapped
        return bool(
            ip.is_private
            or ip.is_loopback
            or ip.is_link_local
            or ip.is_unspecified
            or ip.is_reserved
            or ip.is_multicast
        )

    def _dns_reaches_private(self, host):
        if host in self._dns_cache:
            self._dns_cache.move_to_end(host)
            return self._dns_cache[host]
        try:
            infos = socket.getaddrinfo(host, None)
        except Exception:
            private = False
        else:
            private = any(self._ip_private(item[4][0]) for item in infos)
        while len(self._dns_cache) >= 256:
            self._dns_cache.popitem(last=False)
        self._dns_cache[host] = private
        return private

    @staticmethod
    def _numeric_alias(host):
        """Collapse inet_aton-style hostnames (2130706433, 127.1, 0x7f.0.0.1).

        Chromium accepts these and packs them into an IPv4 address, so a bare
        literal test would let them alias a private host straight through.
        """
        if "_" in host or not re.fullmatch(r"[0-9a-fA-Fx.]+", host):
            return None
        parts = host.split(".")
        if not 1 <= len(parts) <= 4 or any(not part or part == "." for part in parts):
            return None
        try:
            chunks = [Policy._parse_part(part) for part in parts]
        except ValueError:
            return None
        if any(chunk is None for chunk in chunks):
            return None
        trailing_limit = 256 ** (5 - len(chunks))
        if any(
            chunk < 0 or chunk >= (trailing_limit if index == len(chunks) - 1 else 256)
            for index, chunk in enumerate(chunks)
        ):
            return None
        packed = chunks[-1]
        for index, chunk in enumerate(chunks[:-1]):
            # The first part is the most significant byte, counted down from 24.
            packed |= chunk << (8 * (3 - index))
        return ".".join(str((packed >> shift) & 0xFF) for shift in (24, 16, 8, 0))

    @staticmethod
    def _parse_part(part):
        """inet_aton number: 0x.. is hex, leading 0 is octal, else decimal."""
        lowered = part.lower()
        if lowered.startswith("0x"):
            if len(lowered) == 2:
                return None
            return int(lowered, 16)
        if lowered.startswith("0") and len(lowered) > 1:
            if any(char not in "01234567" for char in lowered[1:]):
                return None
            return int(lowered, 8)
        if not lowered.isdigit():
            return None
        return int(lowered, 10)

    def check(self, url):
        """Return (allowed, reason) for anything the browser is about to request."""
        try:
            parsed = urlparse(str(url))
        except Exception:
            return False, "unparseable URL"
        scheme = (parsed.scheme or "").lower()
        if scheme in ("", "about", "data", "blob"):
            return True, ""
        if scheme == "file":
            if self.allow_file_scheme:
                return True, ""
            return False, "file:// URLs are disabled"
        if scheme not in ("http", "https"):
            return False, "only http/https URLs are supported (got %s)" % scheme
        if self.allow_private_network:
            return True, ""
        host = (parsed.hostname or "").strip().lower().strip("[]")
        if not host:
            return True, ""
        if host in self.allow_hosts:
            return True, ""
        if (
            host in ("localhost", "host.docker.internal", "gateway.docker.internal")
            or host.endswith(".localhost")
            or host.endswith(".local")
            or host.endswith(".internal")
        ):
            return False, "private-network host '%s'" % host
        if self._ip_private(host):
            return False, "private-network address '%s'" % host
        alias = self._numeric_alias(host)
        if alias is not None and self._ip_private(alias):
            return False, "private-network address '%s' (%s)" % (host, alias)
        # Anything that is not already a private literal is resolved: decimal,
        # octal and short-form hostnames all alias private addresses once the
        # stub resolver sees them, so a numeric literal cannot skip this check.
        if self._dns_reaches_private(host):
            return False, "host '%s' resolves to a private address" % host
        return True, ""


NET_ERROR_RE = re.compile(r"net::(ERR_[A-Z0-9_]+)")

# `net::ERR_*` codes that name a broken or refused transport: the same URL will
# fail again until the network changes, but a different host may still work.
RETRYABLE_NET_ERRORS = frozenset(
    {
        "ERR_CONNECTION_CLOSED",
        "ERR_CONNECTION_REFUSED",
        "ERR_CONNECTION_RESET",
        "ERR_CONNECTION_TIMED_OUT",
        "ERR_CONNECTION_ABORTED",
        "ERR_CONNECTION_FAILED",
        "ERR_NETWORK_CHANGED",
        "ERR_INTERNET_DISCONNECTED",
        "ERR_TIMED_OUT",
        "ERR_EMPTY_RESPONSE",
        "ERR_ADDRESS_UNREACHABLE",
    }
)


def failure_details(exc, url):
    """Structured companion to a Playwright error message.

    The message keeps the original text for humans; this payload gives the model
    a `code`, the failing host and a retry verdict, mirroring the web_fetch
    failure contract so one branch in the caller covers both tools.
    """
    message = str(exc)
    match = NET_ERROR_RE.search(message)
    net_error = match.group(1) if match else None
    host = None
    try:
        host = urlparse(url).hostname
    except Exception:
        host = None
    if net_error == "ERR_NAME_NOT_RESOLVED":
        code = "TOOL_BROWSER_DNS_FAILED"
    elif net_error in RETRYABLE_NET_ERRORS:
        code = "TOOL_BROWSER_CONNECTION_FAILED"
    elif net_error:
        code = "TOOL_BROWSER_NAVIGATION_FAILED"
    elif "Timeout" in message or "timeout" in message:
        code = "TOOL_BROWSER_TIMEOUT"
    else:
        code = "TOOL_BROWSER_NAVIGATION_FAILED"
    retryable = code in ("TOOL_BROWSER_CONNECTION_FAILED", "TOOL_BROWSER_TIMEOUT")
    details = {
        "phase": "navigation",
        "code": code,
        "retryable": retryable,
        "failure_summary": compact_error(message),
        "next_step_hint": (
            "Retry once, then reach the same resource another way (an API endpoint, or web_fetch)."
            if retryable
            else "The address itself is the problem; pick a different URL or host."
        ),
    }
    if host:
        details["host"] = host
    if net_error:
        details["net_error"] = net_error
    if retryable:
        details["retry_after_ms"] = 30000
    return details


def compact_error(message):
    """Drop the Playwright call log: it is operator detail, not model input."""
    head = message.split("Call log:", 1)[0].strip()
    head = head.split("\n", 1)[0].strip()
    return head or message.strip().split("\n", 1)[0].strip()


def _elapsed_ms(since):
    return int((time.monotonic() - since) * 1000)


def respond(data):
    sys.stdout.write(json.dumps(data, ensure_ascii=False) + "\n")
    sys.stdout.flush()


class BridgeSession:
    def __init__(self, args, policy):
        self.args = args
        self.policy = policy
        self.timeout_ms = max(args.timeout, 1) * 1000
        self.blocked = []
        # A cold start is the dominant cost of the first browser call, so the
        # phases are timed and reported: without them a 50s launch is an
        # undifferentiated "browser is slow" report.
        self.startup_ms = {}
        started = time.monotonic()
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
        self.startup_ms["import_ms"] = _elapsed_ms(started)
        mark = time.monotonic()
        self.pw = sync_playwright().start()
        self.startup_ms["driver_ms"] = _elapsed_ms(mark)
        launch_args = [item for item in args.launch_arg if item]
        # Launch strategy mirrors deepseek-harness `browser-use`: either launch a
        # managed browser (optionally on a user-supplied channel / executable) or
        # attach to an already-running browser over CDP so existing tabs, logins
        # and the user's own Chromium build are preserved.
        self.mode = str(getattr(args, "mode", None) or "launch").strip().lower()
        channel = (getattr(args, "channel", None) or "").strip() or None
        executable_path = (getattr(args, "executable_path", None) or "").strip() or None
        attach_endpoint = (getattr(args, "attach_endpoint", None) or "").strip() or None
        self.channel = channel
        self.executable_path = executable_path
        self.attach_endpoint = attach_endpoint
        self._attached = self.mode == "attach" and bool(attach_endpoint)
        mark = time.monotonic()
        if self._attached:
            self.browser = self.pw.chromium.connect_over_cdp(attach_endpoint)
            if self.browser.contexts:
                # Reuse the caller's browsing context: keeps tabs and session.
                self.context = self.browser.contexts[0]
                self._owns_context = False
            else:
                self.context = self._new_context(args)
                self._owns_context = True
        else:
            launch_kwargs = {
                "headless": args.headless,
                "args": launch_args or None,
            }
            if channel:
                launch_kwargs["channel"] = channel
            if executable_path:
                launch_kwargs["executable_path"] = executable_path
            self.browser = self.pw.chromium.launch(**launch_kwargs)
            self.context = self._new_context(args)
            self._owns_context = True
        self.startup_ms["launch_ms"] = _elapsed_ms(mark)
        self.startup_ms["total_ms"] = _elapsed_ms(started)
        self.pages = OrderedDict()
        self.pages_by_key = {}
        self.refs = {}
        self.target_counter = 0
        self.active_target_id = None
        try:
            if policy.restrictive:
                # Interception costs a round trip per request, so it is only
                # installed when the policy can actually refuse something.
                self.context.route("**/*", self._handle_route)
        except Exception:
            pass
        for page in list(self.context.pages):
            self._register(page)
        if not self.pages:
            self._new_page()
        else:
            self.active_target_id = next(iter(self.pages))

    def _new_context(self, args):
        options = {
            "viewport": {"width": args.width, "height": args.height},
            "accept_downloads": True,
        }
        user_agent = self._launch_user_agent()
        if user_agent:
            options["user_agent"] = user_agent
        return self.browser.new_context(**options)

    def _launch_user_agent(self):
        # The UA must name the browser actually being driven (an older bundled
        # Chromium or a system Edge/Chrome), not a hardcoded build. Without a
        # version, leave the browser's own default UA untouched.
        try:
            version = (self.browser.version or "").strip()
        except Exception:
            return None
        if not version:
            return None
        return (
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 "
            "(KHTML, like Gecko) Chrome/%s Safari/537.36" % version
        )

    def _handle_route(self, route, request):
        allowed, reason = self.policy.check(request.url)
        if allowed:
            try:
                route.continue_()
            except Exception:
                pass
            return
        self.blocked.append(reason)
        try:
            route.fulfill(
                status=403,
                content_type="text/plain; charset=utf-8",
                body="Blocked by wunder browser URL policy: %s\n" % reason,
            )
        except Exception:
            try:
                route.abort("blockedbyclient")
            except Exception:
                pass

    def _register(self, page):
        self.target_counter += 1
        target_id = "tab-%d" % self.target_counter
        try:
            page.set_default_timeout(self.timeout_ms)
            page.set_default_navigation_timeout(self.timeout_ms)
        except Exception:
            pass
        self.pages[target_id] = page
        self.pages_by_key[id(page)] = target_id
        self.refs[target_id] = {}
        if self.active_target_id is None:
            self.active_target_id = target_id
        return target_id, page

    def _sync_pages(self):
        """Reconcile the tab table with the context's live pages.

        Popups are adopted here rather than from the `page` event: a popup can
        emit more than one event before its first navigation commits, and an
        event-driven table fills up with handles that point at nothing.
        """
        try:
            live = list(self.context.pages)
        except Exception:
            return
        limit = max(int(self.args.max_tabs), 1)
        live_ids = set()
        for page in live:
            key = id(page)
            live_ids.add(key)
            if key in self.pages_by_key:
                continue
            if len(self.pages) >= limit:
                try:
                    page.close()
                except Exception:
                    pass
                continue
            self._register(page)
        for key, target_id in list(self.pages_by_key.items()):
            if key not in live_ids:
                self.pages_by_key.pop(key, None)
                self.pages.pop(target_id, None)
                self.refs.pop(target_id, None)
        if self.active_target_id not in self.pages:
            self.active_target_id = next(iter(self.pages), None)

    def close(self):
        # Never tear down a browser we merely attached to; only drop the context
        # we created ourselves, then stop the driver.
        if getattr(self, "_owns_context", True):
            try:
                self.context.close()
            except Exception:
                pass
        if not getattr(self, "_attached", False):
            try:
                self.browser.close()
            except Exception:
                pass
        try:
            self.pw.stop()
        except Exception:
            pass

    def handle(self, raw):
        result = self._handle(raw)
        if raw.get("include_snapshot") and result.get("success"):
            data = result.setdefault("data", {})
            try:
                snapshot = self.snapshot(
                    target_id=data.get("target_id") or raw.get("target_id"),
                    max_chars=char_budget(raw.get("max_chars")),
                    ready_timeout_ms=raw.get("snapshot_timeout_ms"),
                )["data"]
                for key in ("snapshot", "truncated", "target_id", "url", "title"):
                    data[key] = snapshot[key]
                if snapshot.get("blocked"):
                    data.setdefault("blocked", []).extend(snapshot["blocked"])
                # The observation can finish after a SPA redirect. Keep the
                # identity and tab list from that observation, not the action.
                data["tabs"] = snapshot["tabs"]
                page, _ = self._resolve_page(snapshot["target_id"])
                verdict = self._final_url_verdict(page)
                if verdict is not None:
                    return verdict
            except Exception as exc:
                # The action has already succeeded: a failed observation must
                # not invite repeating a submitted form.
                data["snapshot_error"] = "Snapshot unavailable (%s)" % type(exc).__name__
        return result

    def _handle(self, raw):
        self._sync_pages()
        action = str(raw.get("action", "")).strip().lower()
        if action in ("start", "status"):
            return self.status()
        if action in ("tabs", "list_tabs"):
            return self.list_tabs()
        if action in ("open", "open_tab"):
            return self.open_tab(raw.get("url"), raw.get("timeout_ms"))
        if action in ("focus", "focus_tab"):
            return self.focus_tab(raw.get("target_id"))
        if action in ("close", "close_tab"):
            target_id = raw.get("target_id")
            if target_id:
                return self.close_tab(target_id)
            return {"success": True, "data": {"status": "closing"}}
        if action == "navigate":
            return self.navigate(raw.get("url"), raw.get("target_id"), raw.get("timeout_ms"))
        if action == "snapshot":
            return self.snapshot(
                target_id=raw.get("target_id"),
                fmt=raw.get("format") or "role",
                interactive=bool(raw.get("interactive", True)),
                compact=bool(raw.get("compact", True)),
                max_chars=char_budget(raw.get("max_chars")),
                ready_timeout_ms=raw.get("snapshot_timeout_ms"),
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
                max_chars=char_budget(raw.get("max_chars")),
            )
        return {"success": False, "error": "Unknown action: %s" % action}

    def status(self):
        return {
            "success": True,
            "data": {
                "status": "ready",
                "mode": self.mode,
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

    def open_tab(self, url=None, timeout_ms=None):
        if url:
            allowed, reason = self.policy.check(url)
            if not allowed:
                return {"success": False, "error": "Navigation blocked: %s" % reason}
        target_id, page = self._new_page()
        if page is None:
            return {
                "success": False,
                "error": "Browser session reached max tabs limit (%s)" % self.args.max_tabs,
            }
        if url:
            page.goto(url, wait_until="domcontentloaded", timeout=self._timeout_ms(timeout_ms))
            verdict = self._final_url_verdict(page)
            if verdict is not None:
                return verdict
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
        self.pages_by_key.pop(id(page), None)
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

    def navigate(self, url, target_id=None, timeout_ms=None):
        if not url:
            return {"success": False, "error": "Missing 'url' parameter"}
        allowed, reason = self.policy.check(url)
        if not allowed:
            return {"success": False, "error": "Navigation blocked: %s" % reason}
        page, target_id = self._resolve_page(target_id)
        try:
            page.goto(url, wait_until="domcontentloaded", timeout=self._timeout_ms(timeout_ms))
        except Exception as exc:
            # A raw Playwright stack is unreadable for the model and hides the
            # two facts it can act on: which host failed and whether retrying
            # can possibly help. Details are preserved, the call log is not.
            return {"success": False, "error": str(exc), "data": failure_details(exc, url)}
        verdict = self._final_url_verdict(page)
        if verdict is not None:
            return verdict
        self.refs[target_id] = {}
        return {
            "success": True,
            "data": {
                "target_id": target_id,
                "title": page.title(),
                "url": page.url,
                "tabs": self._tabs_state(),
            },
        }

    def _final_url_verdict(self, page):
        """A redirect can land somewhere the entry URL never named."""
        allowed, reason = self.policy.check(page.url)
        if allowed:
            return None
        try:
            page.goto("about:blank", wait_until="domcontentloaded", timeout=self.timeout_ms)
        except Exception:
            pass
        return {"success": False, "error": "Navigation ended at a blocked address: %s" % reason}

    def snapshot(self, target_id=None, fmt="role", interactive=True, compact=True, max_chars=40000,
                 ready_timeout_ms=None):
        max_chars = char_budget(max_chars)
        page, target_id = self._resolve_page(target_id)
        # DOMContentLoaded precedes client rendering. Retry only an empty or
        # interrupted observation; populated pages pay no readiness delay.
        grace_ms = min(self._timeout_ms(ready_timeout_ms if ready_timeout_ms is not None else 1500), 5000)
        if ready_timeout_ms == 0:
            grace_ms = 0
        deadline = time.monotonic() + grace_ms / 1000
        # Failed standalone observations must not leave a previous page's refs
        # available for a later action.
        self.refs[target_id] = {}
        while True:
            url = safe_page_url(page)
            try:
                lines, refs, truncated = self._collect(page, fmt, interactive, compact, max_chars)
                title = safe_page_title(page)
                tabs = self._tabs_state()
                if url != safe_page_url(page):
                    raise RuntimeError("Page changed while taking snapshot")
            except Exception as exc:
                transient = any(message in str(exc) for message in (
                    "Execution context was destroyed", "Cannot find context",
                    "Frame was detached", "Page changed while taking snapshot",
                ))
                if not transient or time.monotonic() >= deadline:
                    raise
            else:
                if lines or truncated or time.monotonic() >= deadline:
                    break
            remaining_ms = (deadline - time.monotonic()) * 1000
            if remaining_ms > 0:
                page.wait_for_timeout(min(50, remaining_ms))
        refs_map = {item["ref"]: item for item in refs}
        self.refs[target_id] = refs_map
        text = "\n".join(lines).strip()
        for tab in tabs:
            if tab["target_id"] == target_id:
                tab.update(url=url, title=title)
        return {
            "success": True,
            "data": {
                "target_id": target_id,
                "title": title,
                "url": url,
                "tabs": tabs,
                "format": fmt,
                "snapshot": text,
                "stats": {
                    "lines": len(lines),
                    "chars": len(text),
                    "refs": len(refs_map),
                    "frames": len(self._reachable_frames(page)),
                },
                "truncated": truncated,
                "blocked": self._drain_blocked(),
            },
        }

    def _collect(self, page, fmt, interactive, compact, max_chars):
        """Snapshot every reachable frame; refs remember which frame they came from."""
        lines = []
        refs = []
        ref_start = 0
        used = 0
        truncated = False
        main = page.main_frame
        frames = self._reachable_frames(page)
        for frame in frames:
            is_main = frame is main or frame.parent_frame is None
            header = "" if is_main else "## frame url=%s" % (frame.url or "about:blank")
            overhead = (len(header) + 1 if header else 0) + (1 if lines else 0)
            remaining = max_chars - used - overhead
            if remaining <= 0:
                truncated = True
                break
            frame_path = [] if is_main else self._frame_path(frame)
            try:
                payload = frame.evaluate(
                    SNAPSHOT_JS,
                    {
                        "fmt": fmt,
                        "interactive": interactive,
                        "compact": compact,
                        "ref_start": ref_start,
                        "max_chars": remaining,
                    },
                )
            except Exception as exc:
                if is_main:
                    raise
                message = "## frame unreadable (%s)" % _short(str(exc))
                if len(message) + used + overhead <= max_chars:
                    lines.append(message)
                    used += len(message) + overhead
                truncated = True
                continue
            if not is_main:
                lines.append(header)
            for item in payload.get("refs") or []:
                entry = dict(item)
                entry["frame_path"] = list(frame_path)
                refs.append(entry)
            lines.extend(payload.get("lines") or [])
            used += len("\n".join(payload.get("lines") or [])) + overhead
            ref_start += int(payload.get("count") or 0)
            if payload.get("truncated"):
                truncated = True
                break
        return lines, refs, truncated

    def _reachable_frames(self, page):
        try:
            return list(page.frames)
        except Exception:
            return []

    def _frame_path(self, frame):
        """frame_locator hops from the main page down to `frame`."""
        hops = []
        current = frame
        while current is not None and current.parent_frame is not None:
            parent = current.parent_frame
            try:
                handle = current.frame_element()
                selector = parent.evaluate(HOST_PATH_JS, handle)
            except Exception:
                return hops
            hops.insert(0, selector)
            current = parent
        return hops

    def _drain_blocked(self):
        # Report policy blocks without echoing an unbounded list.
        recent = self.blocked[-3:]
        del self.blocked[:]
        return recent

    def act(self, request, target_id=None):
        if not isinstance(request, dict):
            return {"success": False, "error": "Browser act 'request' must be an object"}
        kind = str(request.get("kind", "")).strip().lower()
        if not kind:
            return {"success": False, "error": "Browser act request missing 'kind'"}
        if kind == "batch":
            return self.batch(request, target_id)
        page, target_id = self._resolve_page(target_id or request.get("target_id"))
        if kind == "wait":
            return self._wait(page, target_id, request)
        if kind == "evaluate":
            expression = request.get("script") or request.get("expression")
            if not expression:
                return {"success": False, "error": "Browser evaluate missing 'script' or 'expression'"}
            result = page.evaluate(expression)
            verdict = self._final_url_verdict(page)
            if verdict is not None:
                return verdict
            payload = {"target_id": target_id, "result": result, "url": page.url}
            blocked = self._drain_blocked()
            if blocked:
                payload["blocked"] = blocked
            return {"success": True, "data": payload}
        if kind == "close":
            return self.close_tab(target_id)
        if kind == "navigate":
            return self.navigate(
                request.get("url"), target_id, request.get("timeout_ms")
            )
        if kind == "press" and not request.get("ref") and not request.get("selector"):
            key = request.get("key")
            if not key:
                return {"success": False, "error": "Browser press missing 'key'"}
            page.keyboard.press(str(key))
            return self._action_result(page, target_id, kind, extra={"key": key})
        locator_request = ({"ref": request.get("from_ref"), "selector": request.get("from_selector")}
                           if kind == "drag" else request)
        locator = self._resolve_locator(page, target_id, locator_request)
        timeout_ms = self._timeout_ms(request.get("timeout_ms"), request.get("timeout_secs"))
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
            try:
                locator.fill(str(text), timeout=timeout_ms)
                if request.get("submit"):
                    locator.press("Enter", timeout=timeout_ms)
            except Exception as exc:
                return {"success": False, "error": "Browser input failed (%s)" % type(exc).__name__}
            return self._action_result(page, target_id, kind)
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
            value = request.get("value")
            if value is None:
                value = request.get("text")
            if value is None:
                return {"success": False, "error": "Browser select missing 'value' or 'text'"}
            locator.select_option(value=value, timeout=timeout_ms)
            return self._action_result(page, target_id, kind)
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
        return {"success": False, "error": "Unsupported browser act kind: %s" % kind}

    def batch(self, request, target_id=None):
        """Flat, bounded step list: nesting cannot multiply one call's work."""
        steps = request.get("steps", [])
        if not isinstance(steps, list):
            return {"success": False, "error": "Browser batch 'steps' must be an array"}
        if not 1 <= len(steps) <= 10:
            return {"success": False, "error": "Browser batch requires 1 to 10 steps"}
        for step in steps:
            if not isinstance(step, dict) or not step.get("kind"):
                return {"success": False, "error": "Browser batch steps require an object with 'kind'"}
            if isinstance(step, dict) and str(step.get("kind", "")).strip().lower() == "batch":
                return {"success": False, "error": "Browser batch steps must not nest batch"}
            if str(step["kind"]).strip().lower() not in {"type", "fill", "click", "press", "select", "hover", "drag", "wait", "navigate"}:
                return {"success": False, "error": "Unsupported browser batch step kind"}
        results = []
        for index, step in enumerate(steps):
            try:
                result = self.act(step, target_id)
            except Exception as exc:
                # Playwright errors can contain locator.fill arguments. Report
                # the failed operation and type without echoing user inputs.
                result = {"success": False, "error": "Browser %s failed (%s)" % (step["kind"], type(exc).__name__)}
            if not result.get("success"):
                return {"success": False, "error": result.get("error", "Batch step failed"),
                        "data": {"completed_steps": len(results), "failed_step": index + 1,
                                 "results": results}}
            data = result.get("data") or {}
            results.append({key: data[key] for key in ("kind", "target_id", "url") if key in data})
        page, resolved = self._resolve_page(target_id)
        return {"success": True, "data": {"target_id": resolved, "completed_steps": len(results),
                                          "results": results, "url": page.url}}

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
        content, truncated = self._read_all_frames(page, char_budget(max_chars))
        return {
            "success": True,
            "data": {
                "target_id": target_id,
                "title": safe_page_title(page),
                "url": safe_page_url(page),
                "content": content,
                "truncated": truncated,
            },
        }

    def _read_all_frames(self, page, max_chars):
        chunks = []
        used = 0
        truncated = False
        for frame in self._reachable_frames(page):
            header = "" if frame is page.main_frame or frame.parent_frame is None else "## frame url=%s\n" % (frame.url or "about:blank")
            separator = 2 if chunks else 0
            budget = max_chars - used - len(header) - separator
            if budget <= 0:
                truncated = True
                break
            body, clipped = extract_readable(frame, budget)
            body = header + body if body else ""
            if body:
                chunks.append(body)
                used += len(body) + separator
            if clipped:
                truncated = True
                break
        return "\n\n".join(chunks), truncated

    def _new_page(self):
        limit = max(int(self.args.max_tabs), 1)
        if len(self.pages) >= limit:
            return None, None
        target_id, page = self._register(self.context.new_page())
        self.active_target_id = target_id
        try:
            page.bring_to_front()
        except Exception:
            pass
        return target_id, page

    def _timeout_ms(self, value=None, seconds=None):
        if value is None and seconds is not None:
            try:
                return min(max(int(seconds) * 1000, 1), self.timeout_ms * 4)
            except Exception:
                return self.timeout_ms
        if value is None:
            return self.timeout_ms
        try:
            timeout_ms = int(value)
        except Exception:
            return self.timeout_ms
        return min(max(timeout_ms, 1), self.timeout_ms * 4)

    def _resolve_page(self, target_id=None):
        if target_id:
            target_id = str(target_id)
            page = self.pages.get(target_id)
            if page is not None:
                self.active_target_id = target_id
                return page, target_id
            raise ValueError("Unknown browser target_id: %s" % target_id)
        if self.active_target_id and self.active_target_id in self.pages:
            return self.pages[self.active_target_id], self.active_target_id
        if not self.pages:
            return self._new_page()
        target_id = next(iter(self.pages))
        self.active_target_id = target_id
        return self.pages[target_id], target_id

    def _tabs_state(self):
        tabs = []
        # Copied: a popup can be adopted by the event callback while this list is
        # being built, and mutating the registry mid-iteration raises.
        for target_id, page in list(self.pages.items()):
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
                raise ValueError(
                    "Unknown browser ref: %s. Capture a new snapshot first." % ref_id
                )
            selector = ref.get("selector")
            if not selector:
                raise ValueError("Browser ref %s has no selector mapping" % ref_id)
            return self._locator_for(page, ref.get("frame_path") or [], selector)
        selector = request.get("selector")
        if selector:
            return page.locator(str(selector)).first
        text = request.get("text")
        if text and str(request.get("kind", "")).lower() in ("click", "hover"):
            return page.get_by_text(str(text), exact=False).first
        raise ValueError("Browser action requires 'ref' or 'selector'")

    @staticmethod
    def _locator_for(page, frame_path, selector):
        root = page
        for hop in frame_path:
            root = root.frame_locator(str(hop))
        return root.locator(str(selector)).first

    def _wait(self, page, target_id, request):
        timeout_ms = self._timeout_ms(request.get("timeout_ms"), request.get("timeout_secs"))
        state = request.get("state") or "visible"
        if state not in ("visible", "hidden", "attached", "detached"):
            return {"success": False, "error": "Browser wait state must be visible/hidden/attached/detached"}
        selector, text, url, load_state = (request.get(key) for key in ("selector", "text", "url", "load_state"))
        has_condition = any((selector, text, url, load_state))
        # A duration is a fallback pause, never a reason to skip explicit
        # conditions or to delay a condition that is already satisfied.
        if not has_condition and request.get("wait_ms") is not None:
            wait_ms = min(max(int(request["wait_ms"]), 0), timeout_ms)
            page.wait_for_timeout(wait_ms)
            return self._action_result(
                page, target_id, "wait", extra={"wait_ms": wait_ms}
            )
        deadline = time.monotonic() + timeout_ms / 1000

        def remaining_ms():
            # Playwright treats zero as an unbounded timeout.
            return max(1, int((deadline - time.monotonic()) * 1000))

        matched = {}
        if selector:
            page.locator(str(selector)).first.wait_for(state=state, timeout=remaining_ms())
            matched.update(selector=selector, state=state)
        if text:
            page.get_by_text(str(text), exact=False).first.wait_for(state=state, timeout=remaining_ms())
            matched.update(text=text, state=state)
        if url:
            # This matches the current URL; it never initiates a request. The
            # action result still verifies the actual final URL against policy.
            page.wait_for_url(str(url), wait_until="domcontentloaded", timeout=remaining_ms())
            matched["expected_url"] = url
        if load_state:
            page.wait_for_load_state(str(load_state), timeout=remaining_ms())
            matched["load_state"] = load_state
        if has_condition:
            return self._action_result(page, target_id, "wait", extra=matched)
        return {
            "success": False,
            "error": "Browser wait requires one of wait_ms/selector/text/url/load_state",
        }

    def _action_result(self, page, target_id, kind, extra=None):
        verdict = self._final_url_verdict(page)
        if verdict is not None:
            return verdict
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
        blocked = self._drain_blocked()
        if blocked:
            payload["blocked"] = blocked
        return {"success": True, "data": payload}


def _short(text, limit=120):
    value = " ".join(str(text).split())
    return value[:limit]


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


def extract_readable(target, max_chars=40000):
    """Read one frame (or page) as text, piercing open shadow roots."""
    limit = char_budget(max_chars)
    try:
        result = target.evaluate(READABLE_JS, limit)
        return result["content"][:limit], bool(result["truncated"])
    except Exception:
        try:
            content = target.evaluate("limit => (document.body ? document.body.innerText : '').slice(0, limit + 1)", limit)
        except Exception:
            return "", True
    return content[:limit], len(content) > limit


def char_budget(value):
    return min(max(int(value if value is not None else 40000), 1), 40000)


def main():
    parser = argparse.ArgumentParser(description="Wunder Browser Bridge")
    parser.add_argument("--headless", action="store_true", default=True)
    parser.add_argument("--no-headless", dest="headless", action="store_false")
    parser.add_argument("--width", type=int, default=1280)
    parser.add_argument("--height", type=int, default=720)
    parser.add_argument("--max-tabs", dest="max_tabs", type=int, default=8)
    parser.add_argument("--timeout", type=int, default=30)
    parser.add_argument("--launch-arg", dest="launch_arg", action="append", default=[])
    parser.add_argument("--mode", default="launch")
    parser.add_argument("--channel", default=None)
    parser.add_argument("--executable-path", dest="executable_path", default=None)
    parser.add_argument("--attach-endpoint", dest="attach_endpoint", default=None)
    parser.add_argument(
        "--allow-private-network", dest="allow_private_network", action="store_true", default=False
    )
    parser.add_argument(
        "--allow-file-scheme", dest="deny_file_scheme", action="store_false", default=True
    )
    parser.add_argument("--allow-host", dest="allow_host", action="append", default=[])
    args = parser.parse_args()

    # Bundled supplements install Playwright browsers under
    # <python_root>/ms-playwright. Prefer that bundled Chromium by default so
    # browser tasks never depend on the user's own browser and still run
    # offline; only fall back to a system browser when the supplement ships none
    # (supplement browsers are always Chromium, never Google Chrome, because the
    # branded Chrome build is not redistributable). The interpreter lives either
    # directly in <python_root> (Windows embeddable) or in <python_root>/bin
    # (Linux python-build-standalone), so probe both layouts.
    if not os.environ.get("PLAYWRIGHT_BROWSERS_PATH"):
        exe_dir = Path(sys.executable).resolve().parent
        for bundled in (
            exe_dir / "ms-playwright",
            exe_dir.parent / "ms-playwright",
            exe_dir.parent.parent / "ms-playwright",
        ):
            if bundled.is_dir():
                os.environ["PLAYWRIGHT_BROWSERS_PATH"] = str(bundled)
                break

    policy = Policy(args)
    try:
        session = BridgeSession(args, policy)
    except Exception as exc:
        respond(
            {
                "success": False,
                "error": "%s: %s" % (type(exc).__name__, exc),
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
                "mode": session.mode,
                "tabs": session._tabs_state(),
                "active_target_id": session.active_target_id,
                "startup_ms": session.startup_ms,
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
                        "error": "%s: %s" % (type(exc).__name__, exc),
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
