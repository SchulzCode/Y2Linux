#!/usr/bin/env python3
"""Check project-owned Markdown links, headings, and knowledge-base catalogs."""

from __future__ import annotations

import re
import sys
from pathlib import Path
from urllib.parse import unquote


LINK = re.compile(r"\]\(([^\n]+?)\)")
HEADING = re.compile(r"^\s{0,3}#{1,6}\s+(.+?)\s*#*$", re.MULTILINE)
FENCE = re.compile(r"^```.*?^```[^\n]*", re.MULTILINE | re.DOTALL)
EXPLICIT_ANCHOR = re.compile(r"(?:id|name)=[\"']([^\"']+)")
EXTERNAL = re.compile(r"^[a-zA-Z][a-zA-Z0-9+.-]*:")


def pages(root: Path) -> list[Path]:
    result = [root / "readme.md", *sorted((root / "docs").rglob("*.md"))]
    if root.name == "Y2Linux":
        result.extend((root / "AGENTS.md", root / "tests/README.md"))
    else:
        result.extend(sorted((root / "Reborn_Y2_Simple_UI_Asset_Pack").rglob("*.md")))
        result.extend(sorted((root / "assets").rglob("*.md")))
    return [p for p in result if p.is_file() and not p.is_symlink()]


def anchors(markdown: str) -> set[str]:
    result = set(EXPLICIT_ANCHOR.findall(markdown))
    seen: dict[str, int] = {}
    for match in HEADING.finditer(FENCE.sub("", markdown)):
        title = re.sub(r"<[^>]*>", "", match.group(1)).lower()
        slug = re.sub(r"[^\w\- ]", "", title).replace(" ", "-")
        suffix = seen.get(slug, 0)
        seen[slug] = suffix + 1
        result.add(f"{slug}-{suffix}" if suffix else slug)
    return result


def check(root: Path) -> tuple[int, int, list[str]]:
    all_pages = pages(root)
    errors: list[str] = []
    link_count = 0
    cache: dict[Path, set[str]] = {}
    for page in all_pages:
        content = page.read_text(encoding="utf-8")
        for match in LINK.finditer(FENCE.sub("", content)):
            target = match.group(1).strip().split(' "', 1)[0].strip("<>")
            if not target or EXTERNAL.match(target):
                continue
            link_count += 1
            path, _, anchor = target.partition("#")
            destination = (page.parent / unquote(path)).resolve() if path else page
            if not destination.exists():
                errors.append(f"{page.relative_to(root)}: missing {target}")
            elif anchor and destination.suffix == ".md":
                if destination not in cache:
                    cache[destination] = anchors(destination.read_text(encoding="utf-8"))
                if unquote(anchor) not in cache[destination]:
                    errors.append(f"{page.relative_to(root)}: missing heading {target}")

    catalog = root / "docs/DOCUMENTATION_CATALOG.md"
    if not catalog.exists():
        errors.append("missing docs/DOCUMENTATION_CATALOG.md")
    else:
        listed: set[Path] = set()
        for line in catalog.read_text(encoding="utf-8").splitlines():
            match = re.match(r"^\| \[[^]]+\]\(([^)]+)\) \|", line)
            if match:
                entry = (catalog.parent / unquote(match.group(1))).resolve()
                if entry in listed:
                    errors.append(f"catalog duplicates {entry}")
                listed.add(entry)
        for missing in set(all_pages) - listed:
            errors.append(f"catalog omits {missing.relative_to(root)}")
        for stale in listed - set(all_pages):
            errors.append(f"catalog has stale entry {stale}")
    return len(all_pages), link_count, errors


def main() -> int:
    linux = Path(__file__).resolve().parents[2]
    roots = [linux]
    reborn = linux.parent / "Y2Reborn"
    if (reborn / "docs").is_dir():
        roots.append(reborn)
    errors = []
    for root in roots:
        page_count, link_count, problem = check(root)
        print(f"{root.name}: {page_count} pages, {link_count} local links, "
              f"{len(problem)} problems")
        errors.extend(f"{root.name}: {entry}" for entry in problem)
    for error in errors:
        print(error, file=sys.stderr)
    return 1 if errors else 0


if __name__ == "__main__":
    raise SystemExit(main())
