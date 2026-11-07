# -*- coding: utf-8 -*-
"""Shared Slint scanner: extract string literals with a quote/comment state
machine (line comments are only recognized outside strings)."""
import re

CJK = re.compile(r"[\u4e00-\u9fff]")


def has_cjk(text):
    return bool(CJK.search(text))


def scan(source):
    """Yield (start, end, text) for each string literal in Slint source.
    end is the index after the closing quote; text is the raw literal body."""
    i = 0
    n = len(source)
    while i < n:
        ch = source[i]
        if ch == '"':
            j = i + 1
            body = []
            while j < n:
                c = source[j]
                if c == "\\" and j + 1 < n:
                    body.append(source[j : j + 2])
                    j += 2
                    continue
                if c == '"':
                    break
                body.append(c)
                j += 1
            yield (i, j + 1, "".join(body))
            i = j + 1
            continue
        if ch == "/" and i + 1 < n and source[i + 1] == "/":
            nl = source.find("\n", i)
            i = n if nl < 0 else nl + 1
            continue
        i += 1
