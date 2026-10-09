"""Build the checked-in, lazy CJK webfonts from hash-pinned upstream sources.

Requires fonttools[woff]==4.63.0 and brotli==1.2.0. Sources are supplied by the
operator in --source-dir; this script performs no network request. The manifest
records original sources, excluded Latin/kana ranges and every output hash.
"""

from __future__ import annotations

import argparse
import copy
import hashlib
import io
import json
import re
import stat
from pathlib import Path

import fontTools
import brotli
from fontTools import subset
from fontTools.ttLib import TTFont

APP = Path(__file__).resolve().parents[1]
TW_COMMIT = "9ada49ab67e05e68f83b3dde46b260e2bc2a7c39"
JA_COMMIT = "553c872b216d1290e2902a466edcdc9682f0df6a"
SOURCES = [
    {
        "file": "tw-regular.woff2",
        "sha256": "c747f9cffcb41bf586f56f2cc28550dbfed30804968b62f46e6608552cb6d4c1",
        "url": f"https://raw.githubusercontent.com/max32002/TaiwanPearl/{TW_COMMIT}/webfont/TaiwanPearl-Regular.woff2",
        "directory": "taiwan-pearl", "family": "Illya Pearl", "weight": 400,
        "locale": "zh-TW", "style": "Regular", "original": "TaiwanPearl",
    },
    {
        "file": "tw-semibold.woff2",
        "sha256": "0fce3b291a660a1c69cba3ee9938be32de54fec7f7d0fe6e5b359671e1403f72",
        "url": f"https://raw.githubusercontent.com/max32002/TaiwanPearl/{TW_COMMIT}/webfont/TaiwanPearl-SemiBold.woff2",
        "directory": "taiwan-pearl", "family": "Illya Pearl", "weight": 600,
        "locale": "zh-TW", "style": "SemiBold", "original": "TaiwanPearl",
    },
    {
        "file": "ja-regular.ttf",
        "sha256": "a0c0b53543e0993ae2225e629c833f3d51495ad31720694ff112ce4ce11111ef",
        "url": f"https://raw.githubusercontent.com/googlefonts/zen-marugothic/{JA_COMMIT}/fonts/ttf/ZenMaruGothic-Regular.ttf",
        "directory": "zen-maru-gothic", "family": "Illya Maru", "weight": 400,
        "locale": "ja", "style": "Regular", "original": "Zen Maru Gothic",
    },
    {
        "file": "ja-bold.ttf",
        "sha256": "fe24426b9c8b5523a0146a8235c8674eccf0493af354a53ec895c3596d9eb745",
        "url": f"https://raw.githubusercontent.com/googlefonts/zen-marugothic/{JA_COMMIT}/fonts/ttf/ZenMaruGothic-Bold.ttf",
        "directory": "zen-maru-gothic", "family": "Illya Maru", "weight": 700,
        "locale": "ja", "style": "Bold", "original": "Zen Maru Gothic",
    },
]
KANA_FALLBACK = {0x3095, 0x3096, 0x3099, 0x309A, 0x309F, 0x30A0, 0x30FF,
                 *range(0x31F0, 0x3200)}


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def allowed(codepoint: int, locale: str) -> bool:
    # Nunito remains the Latin voice. Kana always reaches the Japanese face,
    # including mixed-language map titles in the English/Chinese interface.
    if codepoint <= 0x024F or 0x1E00 <= codepoint <= 0x1EFF:
        return False
    if locale == "zh-TW" and codepoint not in KANA_FALLBACK and any(start <= codepoint <= end for start, end in
                                [(0x3040, 0x30FF), (0x31F0, 0x31FF), (0xFF61, 0xFF9F)]):
        return False
    return True


def variations(font: TTFont, bases: set[int]):
    return sorted((selector, base, glyph)
                  for table in font["cmap"].tables if table.format == 14
                  for selector, entries in table.uvsDict.items()
                  for base, glyph in entries if base in bases)


def require_plain_output(path: Path):
    """Anchor every existing output ancestor to the actual app checkout."""
    if not path.is_relative_to(APP):
        raise SystemExit(f"Output outside app: {path}")
    for candidate in [path, *path.parents]:
        if candidate == APP:
            break
        if candidate.exists() or candidate.is_symlink():
            info = candidate.lstat()
            if candidate.is_symlink() or getattr(info, "st_file_attributes", 0) & stat.FILE_ATTRIBUTE_REPARSE_POINT:
                raise SystemExit(f"Refused redirected font output: {candidate}")


def strings(value):
    if isinstance(value, str):
        yield value
    elif isinstance(value, dict):
        for child in value.values():
            yield from strings(child)
    elif isinstance(value, list):
        for child in value:
            yield from strings(child)


def unicode_range(points: list[int]) -> str:
    ranges = []
    first = last = points[0]
    for point in points[1:]:
        if point == last + 1:
            last = point
        else:
            ranges.append(f"U+{first:X}" if first == last else f"U+{first:X}-{last:X}")
            first = last = point
    ranges.append(f"U+{first:X}" if first == last else f"U+{first:X}-{last:X}")
    return ",".join(ranges)


def rename(font: TTFont, family: str, style: str):
    # TaiwanPearl's embedded OFL reserves its original name. Every derivative
    # uses new family, full and PostScript names; copyright/license stay intact.
    names = {
        1: family, 2: style, 3: f"Illyaverse:{family}:{style}:1", 4: f"{family} {style}",
        6: f"{family.replace(' ', '')}-{style}", 16: family, 17: style,
        18: f"{family} {style}", 21: family, 22: style,
    }
    for record in font["name"].names:
        if record.nameID in names:
            record.string = names[record.nameID].encode(record.getEncoding(), errors="replace")
    if "DSIG" in font:
        del font["DSIG"]


def generate(source_dir: Path, chunk_size: int):
    if fontTools.__version__ != "4.63.0":
        raise SystemExit(f"Use pinned fonttools 4.63.0, got {fontTools.__version__}")
    if brotli.__version__ != "1.2.0":
        raise SystemExit(f"Use pinned brotli 1.2.0, got {brotli.__version__}")
    # Validate all inputs before writing any asset.
    for source in SOURCES:
        actual = digest((source_dir / source["file"]).read_bytes())
        if actual != source["sha256"]:
            raise SystemExit(f"Source hash mismatch for {source['file']}: {actual}")
    public = APP / "public/fonts"
    require_plain_output(public)
    for directory in ["taiwan-pearl", "zen-maru-gothic"]:
        require_plain_output(public / directory)
    public.mkdir(parents=True, exist_ok=True)
    previous_path = public / "provenance.json"
    previous = json.loads(previous_path.read_text(encoding="utf-8")) if previous_path.exists() else {"fonts": []}
    previous_assets = {asset["path"]: asset for font in previous["fonts"] for asset in font["assets"]}
    previous_sources = {asset["path"]: font for font in previous["fonts"] for asset in font["assets"]}
    css = ["/* Generated by scripts/build-cjk-fonts.py; see /fonts/provenance.json. */"]
    manifest = {"generator": "fonttools 4.63.0 / brotli 1.2.0", "chunk_size": chunk_size,
                "license": "SIL Open Font License 1.1", "latin_excluded": ["U+0000-024F", "U+1E00-1EFF"],
                "taiwan_kana_excluded": ["U+3040-30FF", "U+31F0-31FF", "U+FF61-FF9F"],
                "taiwan_kana_fallback": [f"U+{point:X}" for point in sorted(KANA_FALLBACK)],
                "fonts": []}
    for source in SOURCES:
        original = TTFont(source_dir / source["file"], recalcTimestamp=False)
        decoded_source = original
        (public / source["directory"]).mkdir(parents=True, exist_ok=True)
        if "FFTM" in original:
            del original["FFTM"]  # FontForge's editing timestamp is not a webfont table.
        eligible = {point for point in original.getBestCmap() if allowed(point, source["locale"])}
        copy_text = json.loads((APP / f"messages/{source['locale']}.json").read_text(encoding="utf-8"))
        common = eligible & {ord(char) for text in strings(copy_text) for char in text}
        # Include script basics even when not present in the current translation.
        if source["locale"] == "ja":
            common |= {point for point in eligible if 0x3000 <= point <= 0x30FF or 0xFF61 <= point <= 0xFF9F}
        common |= {point for point in eligible if 0x3000 <= point <= 0x303F}
        groups = [("common", sorted(common))]
        fallback_kana = eligible & KANA_FALLBACK if source["locale"] == "zh-TW" else set()
        fallback_kana -= common
        if fallback_kana:
            groups.append(("kana-fallback", sorted(fallback_kana)))
        remainder = sorted(eligible - common - fallback_kana)
        groups += [(f"{index // chunk_size:03d}", remainder[index:index + chunk_size])
                   for index in range(0, len(remainder), chunk_size)]
        font_record = {**source, "source_codepoints": len(original.getBestCmap()),
                       "eligible_codepoints": len(eligible), "common_codepoints": len(common),
                       "variation_sequences": variations(original, eligible),
                       "covered_codepoints": 0, "assets": []}
        written = set()
        for suffix, points in groups:
            filename = f"{source['weight']}-{suffix}.woff2"
            target = public / source["directory"] / filename
            require_plain_output(target)
            asset_path = f"{source['directory']}/{filename}"
            expected_variations = variations(original, set(points))
            cached = previous_assets.get(asset_path)
            prior_source = previous_sources.get(asset_path, {})
            reusable = (cached and prior_source.get("sha256") == source["sha256"]
                        and prior_source.get("family") == source["family"]
                        and prior_source.get("weight") == source["weight"]
                        and target.is_file() and not target.is_symlink()
                        and digest(target.read_bytes()) == cached["sha256"])
            if reusable:
                font = TTFont(target, recalcTimestamp=False)
                reusable = set(font.getBestCmap()) == set(points) and variations(font, set(points)) == expected_variations
            if not reusable:
                # Decode source WOFF2 outlines once, only when a chunk needs
                # rebuilding. Known previous chunks are reused only after their
                # bytes, actual cmap and UVS mapping match the pinned source.
                if decoded_source.flavor == "woff2":
                    decoded = io.BytesIO()
                    decoded_source.flavor = None
                    decoded_source.save(decoded, reorderTables=True)
                    decoded_source = TTFont(io.BytesIO(decoded.getvalue()), recalcTimestamp=False)
                font = copy.deepcopy(decoded_source)
                rename(font, source["family"], source["style"])
                options = subset.Options()
                options.flavor = "woff2"
                options.recalc_timestamp = False
                options.canonical_order = True
                options.name_IDs = ["*"]
                options.name_languages = ["*"]
                options.layout_features = ["*"]
                reducer = subset.Subsetter(options=options)
                reducer.populate(unicodes=set(points) | {selector for selector, _, _ in expected_variations})
                reducer.subset(font)
                font.flavor = "woff2"
                font.save(target, reorderTables=True)
            actual_points = set(font.getBestCmap())
            if actual_points != set(points) or written & actual_points:
                raise SystemExit(f"Overlapping/incomplete cmap in {target}")
            written |= actual_points
            data = target.read_bytes()
            ranges = unicode_range(points)
            if variations(font, set(points)) != expected_variations:
                raise SystemExit(f"Lost source variation mapping in {target}")
            asset = {"path": asset_path, "bytes": len(data), "variation_sequences": expected_variations,
                     "sha256": digest(data), "unicode_range": ranges, "codepoints": len(points)}
            font_record["assets"].append(asset)
            css.append(f"@font-face {{\n  font-family: '{source['family']}';\n  font-style: normal;\n"
                       f"  font-weight: {source['weight']};\n  font-display: swap;\n"
                       f"  src: url('/fonts/{source['directory']}/{filename}') format('woff2');\n"
                       f"  unicode-range: {ranges};\n}}")
        if written != eligible:
            raise SystemExit(f"Incomplete repertoire for {source['file']}")
        font_record["covered_codepoints"] = len(written)
        manifest["fonts"].append(font_record)
        print(f"{source['family']} {source['weight']}: {len(written)} codepoints, "
              f"{len(groups)} chunks, common {font_record['assets'][0]['bytes']} bytes", flush=True)
    stylesheet = APP / "src/styles/cjk-fonts.css"
    stylesheet.write_text("\n\n".join(css) + "\n", encoding="utf-8", newline="\n")
    manifest["stylesheet_sha256"] = digest(stylesheet.read_bytes())
    manifest["translation_sha256"] = {locale: digest((APP / f"messages/{locale}.json").read_bytes())
                                       for locale in ["zh-TW", "ja"]}
    manifest["notices"] = {str(notice.relative_to(public)).replace("\\", "/"): digest(notice.read_bytes())
                           for directory in ["taiwan-pearl", "zen-maru-gothic"]
                           for notice in sorted((public / directory).glob("*.txt"))}
    current_assets = {asset["path"] for font in manifest["fonts"] for asset in font["assets"]}
    # Delete only previous manifest-owned stale chunks whose bytes still match.
    # A changed file or a symlink is preserved and stops cleanup.
    for old_path, old_asset in previous_assets.items():
        if old_path in current_assets:
            continue
        stale = public / old_path
        require_plain_output(stale)
        if (not re.fullmatch(r"(taiwan-pearl|zen-maru-gothic)/(400|600|700)-(common|kana-fallback|\d{3})\.woff2", old_path)
                or stale.parent.resolve() not in {(public / directory).resolve() for directory in ["taiwan-pearl", "zen-maru-gothic"]}):
            raise SystemExit(f"Invalid previous generated asset path: {old_path}")
        if stale.is_symlink() or (stale.exists() and digest(stale.read_bytes()) != old_asset["sha256"]):
            raise SystemExit(f"Preserved changed stale asset: {old_path}")
        if stale.exists():
            stale.unlink()
    (public / "provenance.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n",
                                          encoding="utf-8", newline="\n")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-dir", type=Path, required=True)
    parser.add_argument("--chunk-size", type=int, default=512)
    args = parser.parse_args()
    if args.chunk_size != 512:
        parser.error("The checked-in delivery contract uses 512-codepoint remainder chunks")
    generate(args.source_dir, args.chunk_size)
