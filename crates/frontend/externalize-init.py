#!/usr/bin/env python3
"""Post-build step: move trunk's inline WASM bootstrap script into /init.js.

The Content-Security-Policy (backend + nginx) allows only external same-origin
scripts (script-src 'self' 'wasm-unsafe-eval'), so trunk's injected inline
module script must become an external file. Run after `trunk build`:

    Linux/Docker/CI:  python3 externalize-init.py dist
    Windows:          py externalize-init.py dist

Fails loudly when the expected single inline module script is not found,
e.g. after a trunk upgrade that changes the injected markup.
"""

import os
import re
import sys

dist = sys.argv[1] if len(sys.argv) > 1 else os.environ.get("TRUNK_STAGING_DIR")
if not dist:
    sys.exit("usage: externalize-init.py <dist-dir>")

index_path = os.path.join(dist, "index.html")
with open(index_path, encoding="utf-8") as f:
    html = f.read()

# Inline module scripts only: a script tag without src= and with a body.
pattern = re.compile(
    r"<script(?P<attrs>[^>]*)>(?P<body>.*?)</script>", re.DOTALL | re.IGNORECASE
)
inline = [
    m
    for m in pattern.finditer(html)
    if "src=" not in m.group("attrs").lower() and m.group("body").strip()
]
if len(inline) != 1:
    sys.exit(
        f"expected exactly 1 inline script in {index_path}, found {len(inline)}; "
        "trunk's injected bootstrap markup may have changed - update externalize-init.py"
    )

match = inline[0]
with open(os.path.join(dist, "init.js"), "w", encoding="utf-8", newline="\n") as f:
    f.write(match.group("body").strip() + "\n")

html = html[: match.start()] + '<script type="module" src="/init.js"></script>' + html[match.end() :]
with open(index_path, "w", encoding="utf-8", newline="\n") as f:
    f.write(html)

print("externalize-init: moved inline bootstrap to init.js")
