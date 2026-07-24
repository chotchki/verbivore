#!/usr/bin/env python3
"""One-time polite mirror of a spread of CSS Zen Garden designs: identical
HTML, radically different stylesheets — the variation-grid thesis in its
purest form (same labels, different pixels). Designs stay copyright their
authors; this mirror is a local training cache, never redistributed."""
import pathlib, re, sys, time, urllib.request

DESIGNS = ["214", "213", "212", "211", "210", "202", "199", "190", "185",
           "180", "176", "172", "168", "160", "151", "140", "123", "110", "099", "069"]
ROOT = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else "vendor/zengarden")

def fetch(url):
    req = urllib.request.Request(url, headers={"User-Agent": "verbivore-corpus-mirror (one-time)"})
    with urllib.request.urlopen(req, timeout=30) as r:
        return r.read()

def main():
    if (ROOT / "index.html").exists():
        print("zengarden mirrored, skipping")
        return
    ROOT.mkdir(parents=True, exist_ok=True)
    rows = []
    for d in DESIGNS:
        dest = ROOT / d
        dest.mkdir(exist_ok=True)
        try:
            html = fetch(f"https://csszengarden.com/{d}/").decode("utf-8", "replace")
            css = fetch(f"https://csszengarden.com/{d}/{d}.css?v=8may2013").decode("utf-8", "replace")
        except Exception as e:
            print(f"design {d}: {e}")
            continue
        # Assets referenced by the css, fetched next to it.
        for asset in set(re.findall(r"url\(['\"]?([^'\")]+)['\"]?\)", css)):
            if asset.startswith("data:") or "//" in asset:
                continue
            name = asset.split("?")[0].lstrip("./")
            try:
                (dest / pathlib.Path(name).name).write_bytes(
                    fetch(f"https://csszengarden.com/{d}/{name}"))
                css = css.replace(asset, pathlib.Path(name).name)
            except Exception:
                pass
        # Point the page at the local css, strip the remote alternates.
        html = re.sub(r'<link[^>]*href="[^"]*\.css[^"]*"[^>]*>',
                      f'<link rel="stylesheet" href="/{d}/{d}.css">', html, count=1)
        (dest / f"{d}.css").write_text(css)
        (dest / "index.html").write_text(html)
        rows.append(f'<li><a href="/{d}/">design {d}</a></li>')
        time.sleep(0.5)  # politeness
    (ROOT / "index.html").write_text(
        "<!DOCTYPE html><html><head><title>Zen garden mirror</title></head>"
        f"<body><h1>Designs</h1><ul>{''.join(rows)}</ul></body></html>")
    print(f"mirrored {len(rows)} designs")

main()
