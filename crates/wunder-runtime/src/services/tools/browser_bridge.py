#!/usr/bin/env python3
"""Wunder Browser Bridge - Playwright automation over JSON-line stdio protocol.

Reads JSON commands from stdin (one per line), executes browser actions via
Playwright, and writes JSON responses to stdout (one per line).

Usage:
    python browser_bridge.py [--headless] [--width 1280] [--height 720] [--timeout 30]
"""

import argparse
import base64
import json
import sys
import traceback


def main():
    parser = argparse.ArgumentParser(description="Wunder Browser Bridge")
    parser.add_argument("--headless", action="store_true", default=True)
    parser.add_argument("--no-headless", dest="headless", action="store_false")
    parser.add_argument("--width", type=int, default=1280)
    parser.add_argument("--height", type=int, default=720)
    parser.add_argument("--timeout", type=int, default=30)
    args = parser.parse_args()

    timeout_ms = args.timeout * 1000

    try:
        from playwright.sync_api import sync_playwright
    except Exception:
        respond(
            {
                "success": False,
                "error": "playwright not installed. Run: pip install playwright && playwright install chromium",
            }
        )
        return

    pw = sync_playwright().start()
    browser = pw.chromium.launch(headless=args.headless)
    context = browser.new_context(
        viewport={"width": args.width, "height": args.height},
        user_agent=(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 "
            "(KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"
        ),
    )
    page = context.new_page()
    page.set_default_timeout(timeout_ms)
    page.set_default_navigation_timeout(timeout_ms)

    respond({"success": True, "data": {"status": "ready"}})

    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        action = None
        try:
            cmd = json.loads(line)
            action = cmd.get("action", "")
            result = handle_command(page, context, action, cmd, timeout_ms)
            respond(result)
        except Exception as exc:
            respond({"success": False, "error": f"{type(exc).__name__}: {exc}"})

        if action == "Close":
            break

    try:
        context.close()
        browser.close()
        pw.stop()
    except Exception:
        pass


def handle_command(page, context, action, cmd, timeout_ms):
    if action == "Navigate":
        url = cmd.get("url", "")
        if not url:
            return {"success": False, "error": "Missing 'url' parameter"}
        page.goto(url, wait_until="domcontentloaded", timeout=timeout_ms)
        title = page.title()
        content = extract_readable(page)
        return {
            "success": True,
            "data": {"title": title, "url": page.url, "content": content},
        }

    if action == "Click":
        selector = cmd.get("selector", "")
        if not selector:
            return {"success": False, "error": "Missing 'selector' parameter"}
        try:
            page.click(selector, timeout=timeout_ms)
        except Exception:
            page.get_by_text(selector, exact=False).first.click(timeout=timeout_ms)
        page.wait_for_load_state("domcontentloaded", timeout=timeout_ms)
        title = page.title()
        return {
            "success": True,
            "data": {"clicked": selector, "title": title, "url": page.url},
        }

    if action == "Type":
        selector = cmd.get("selector", "")
        text = cmd.get("text", "")
        if not selector:
            return {"success": False, "error": "Missing 'selector' parameter"}
        if not text:
            return {"success": False, "error": "Missing 'text' parameter"}
        page.fill(selector, text, timeout=timeout_ms)
        return {"success": True, "data": {"typed": text, "selector": selector}}

    if action == "Screenshot":
        screenshot_bytes = page.screenshot(full_page=False)
        b64 = base64.b64encode(screenshot_bytes).decode("utf-8")
        return {
            "success": True,
            "data": {"image_base64": b64, "format": "png", "url": page.url},
        }

    if action == "ReadPage":
        title = page.title()
        content = extract_readable(page)
        return {
            "success": True,
            "data": {"title": title, "url": page.url, "content": content},
        }

    if action == "Close":
        return {"success": True, "data": {"status": "closed"}}

    return {"success": False, "error": f"Unknown action: {action}"}


def extract_readable(page):
    try:
        content = page.evaluate(
            """() => {
            const clone = document.body.cloneNode(true);
            const remove = ['script', 'style', 'nav', 'footer', 'header', 'aside',
                           'iframe', 'noscript', 'svg', 'canvas'];
            remove.forEach(tag => {
                clone.querySelectorAll(tag).forEach(el => el.remove());
            });

            const main = clone.querySelector('main, article, [role="main"], .content, #content');
            const source = main || clone;

            const lines = [];
            const walk = (node) => {
                if (node.nodeType === 3) {
                    const text = node.textContent.trim();
                    if (text) lines.push(text);
                } else if (node.nodeType === 1) {
                    const tag = node.tagName.toLowerCase();
                    if (['h1','h2','h3','h4','h5','h6'].includes(tag)) {
                        lines.push('\n## ' + node.textContent.trim());
                    } else if (tag === 'li') {
                        lines.push('- ' + node.textContent.trim());
                    } else if (tag === 'a' && node.href) {
                        lines.push('[' + node.textContent.trim() + '](' + node.href + ')');
                    } else if (['p', 'div', 'section', 'td', 'th'].includes(tag)) {
                        for (const child of node.childNodes) walk(child);
                        lines.push('');
                    } else {
                        for (const child of node.childNodes) walk(child);
                    }
                }
            };
            walk(source);
            return lines.join('\n').replace(/\n{3,}/g, '\n\n').trim();
        }"""
        )
        max_chars = 50000
        if len(content) > max_chars:
            content = content[:max_chars] + f"\n\n[Truncated - {len(content)} total chars]"
        return content
    except Exception:
        try:
            text = page.inner_text("body")
            if len(text) > 50000:
                text = text[:50000] + f"\n\n[Truncated - {len(text)} total chars]"
            return text
        except Exception:
            return "(could not extract page content)"


def respond(data):
    sys.stdout.write(json.dumps(data) + "\n")
    sys.stdout.flush()


if __name__ == "__main__":
    main()
