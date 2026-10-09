"""Verify actual shipped cmap/name/license/hash coverage, without the network."""

import argparse
import hashlib
import json
from pathlib import Path

from fontTools.ttLib import TTFont

from importlib.util import module_from_spec, spec_from_file_location

APP = Path(__file__).resolve().parents[1]
spec = spec_from_file_location("builder", APP / "scripts/build-cjk-fonts.py")
builder = module_from_spec(spec)
spec.loader.exec_module(builder)


def points(ranges):
    result = set()
    for item in ranges.split(","):
        bounds = item.removeprefix("U+").split("-")
        result.update(range(int(bounds[0], 16), int(bounds[-1], 16) + 1))
    return result


def check(source_dir):
    public = APP / "public/fonts"
    manifest = json.loads((public / "provenance.json").read_text(encoding="utf-8"))
    stylesheet = APP / "src/styles/cjk-fonts.css"
    assert builder.digest(stylesheet.read_bytes()) == manifest["stylesheet_sha256"]
    total_files = total_bytes = 0
    family_coverage = {}
    for record in manifest["fonts"]:
        source = source_dir / record["file"]
        assert builder.digest(source.read_bytes()) == record["sha256"]
        original = TTFont(source)
        eligible = {point for point in original.getBestCmap() if builder.allowed(point, record["locale"])}
        covered = set()
        covered_variations = []
        for asset in record["assets"]:
            asset_path = public / asset["path"]
            data = asset_path.read_bytes()
            assert len(data) == asset["bytes"]
            assert builder.digest(data) == asset["sha256"]
            font = TTFont(asset_path)
            actual = set(font.getBestCmap())
            assert actual == points(asset["unicode_range"])
            assert len(actual) == asset["codepoints"]
            assert not (covered & actual), asset["path"]
            covered |= actual
            actual_variations = builder.variations(font, actual)
            assert [list(item) for item in actual_variations] == asset["variation_sequences"]
            covered_variations.extend(actual_variations)
            primary_names = [name.toUnicode() for name in font["name"].names
                             if name.nameID in [1, 4, 6, 16, 18, 21]]
            assert primary_names and all(record["original"] not in name for name in primary_names)
            assert any(record["family"] in name for name in primary_names)
            assert any(name.nameID == 0 and "Copyright" in name.toUnicode()
                       for name in font["name"].names), asset["path"]
            assert any(name.nameID == 13 and "Open Font License" in name.toUnicode()
                       for name in font["name"].names), asset["path"]
            assert "font-display: swap" in stylesheet.read_text(encoding="utf-8")
            total_files += 1
            total_bytes += len(data)
        assert covered == eligible, record["file"]
        assert len(covered) == record["covered_codepoints"] == record["eligible_codepoints"]
        assert sorted(covered_variations) == builder.variations(original, eligible)
        assert [list(item) for item in sorted(covered_variations)] == record["variation_sequences"]
        family_coverage[record["family"]] = covered
        assert all(ord(char) in covered for char in ("繁體中文龘𠮷" if record["locale"] == "zh-TW" else "日本語ビートマップ"))
        # Both families retain the source's entire eligible dynamic repertoire;
        # every current UI glyph supported by the upstream face is also shipped.
        current_copy = json.loads((APP / f"messages/{record['locale']}.json").read_text(encoding="utf-8"))
        ui_points = {ord(char) for value in builder.strings(current_copy) for char in value}
        assert not ((ui_points & eligible) - covered)
        assert "SIL OPEN FONT LICENSE Version 1.1" in (public / record["directory"] / "OFL.txt").read_text(encoding="utf-8")
    tw_source = TTFont(source_dir / "tw-regular.woff2").getBestCmap()
    ja_source = TTFont(source_dir / "ja-regular.ttf").getBestCmap()
    all_kana = {point for point in tw_source if any(start <= point <= end for start, end in
                [(0x3040, 0x30FF), (0x31F0, 0x31FF), (0xFF61, 0xFF9F)])}
    assert all_kana - set(ja_source) == builder.KANA_FALLBACK
    assert all_kana <= family_coverage["Illya Pearl"] | family_coverage["Illya Maru"]
    for notice, expected in manifest["notices"].items():
        assert builder.digest((public / notice).read_bytes()) == expected
    print(f"Verified {total_files} WOFF2 files / {total_bytes:,} bytes; full eligible source coverage, "
          "nonoverlapping CSS ranges, source/output hashes, renamed families, copyrights and OFL.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-dir", type=Path, required=True)
    check(parser.parse_args().source_dir)
