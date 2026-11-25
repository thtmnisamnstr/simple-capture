import argparse
import importlib.util
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent
REPO = ROOT.parent

LIMITS = {
    "name.txt": 30,
    "subtitle.txt": 30,
    "description.txt": 4000,
    "keywords.txt": 100,
    "promotional_text.txt": 170,
}
URLS = ("support_url.txt", "marketing_url.txt", "privacy_url.txt")
REVIEW_NOTES_LIMIT = 4000
WHATS_NEW_LIMIT = 4000

FORBIDDEN = re.compile(
    r"\b(?:beta|x264|x265|gifski|price[sd]?|pricing)\b|\$\d"
    r"|(?<![\w-])free(?![\w-]| (?:disk )?space\b)",
    re.IGNORECASE,
)
PLATFORMS = re.compile(r"\b(Windows|Linux|Android|iOS)\b")
MARKDOWN = re.compile(r"(^#|\*\*|__|^\s*[-*] |\[[^\]]+\]\()", re.MULTILINE)
NETWORK_CLAIM = re.compile(r"\bno network(?! connections)", re.IGNORECASE)

AGE_ENUMS = {"NONE", "INFREQUENT_OR_MILD", "FREQUENT_OR_INTENSE", "INFREQUENT", "FREQUENT"}
AGE_OVERRIDES = {
    "ageRatingOverrideV2": {"NONE", "NINE_PLUS", "THIRTEEN_PLUS", "SIXTEEN_PLUS", "EIGHTEEN_PLUS", "UNRATED"},
    "koreaAgeRatingOverride": {"NONE", "ALL", "TWELVE_PLUS", "FIFTEEN_PLUS", "NINETEEN_PLUS"},
}


def text(path: Path) -> str:
    return path.read_text(encoding="utf-8").strip()


def check_locale(loc: Path, problems: list[str], counts: dict[str, int]) -> None:
    for p in sorted(loc.iterdir()):
        if p.name == "release_notes.txt":
            problems.append(f"{p}: What's New comes from the version's section of CHANGELOG.md; delete this file")
        elif not p.name.startswith(".") and p.name not in LIMITS and p.name not in URLS:
            problems.append(f"{p}: unknown file")
    name_words = set()
    for file, limit in LIMITS.items():
        p = loc / file
        if not p.is_file():
            problems.append(f"{p}: missing")
            continue
        t = text(p)
        counts[f"{loc.name}/{file}"] = len(t)
        if not t:
            problems.append(f"{p}: empty")
        if len(t) > limit:
            problems.append(f"{p}: {len(t)} characters, limit {limit}")
        bad = FORBIDDEN.search(t) or PLATFORMS.search(t)
        if bad:
            problems.append(f"{p}: contains {bad.group(0)!r}")
        if MARKDOWN.search(t):
            problems.append(f"{p}: looks like Markdown")
        if NETWORK_CLAIM.search(t):
            problems.append(f"{p}: say the app makes no network connections")
        if file == "name.txt":
            name_words = {w.lower() for w in t.split()}
    kw = loc / "keywords.txt"
    if kw.is_file():
        words = [w.strip() for w in text(kw).split(",")]
        if any(not w for w in words):
            problems.append(f"{kw}: empty keyword")
        if len(set(map(str.lower, words))) != len(words):
            problems.append(f"{kw}: repeated keyword")
        repeated = name_words & {w.lower() for w in words}
        if repeated:
            problems.append(f"{kw}: repeats the app name: {sorted(repeated)}")
    for file in URLS:
        p = loc / file
        if not p.is_file():
            problems.append(f"{p}: missing")
        elif not re.fullmatch(r"https://\S+", text(p)):
            problems.append(f"{p}: not an https URL")


def check_json(problems: list[str], counts: dict[str, int]) -> None:
    info = json.loads((ROOT / "app_info.json").read_text(encoding="utf-8"))
    for key in ("primaryCategory", "secondaryCategory", "copyright", "contentRightsDeclaration"):
        if not info.get(key):
            problems.append(f"app_info.json: {key} missing")
    if info.get("contentRightsDeclaration") not in (
        "DOES_NOT_USE_THIRD_PARTY_CONTENT",
        "USES_THIRD_PARTY_CONTENT",
    ):
        problems.append("app_info.json: contentRightsDeclaration has an unknown value")

    age = json.loads((ROOT / "age_rating.json").read_text(encoding="utf-8"))
    for key, value in age.items():
        if value is None or isinstance(value, bool):
            continue
        allowed = AGE_OVERRIDES.get(key, AGE_ENUMS)
        if value not in allowed:
            problems.append(f"age_rating.json: {key} has an unknown value {value!r}")

    review = json.loads((ROOT / "review.json").read_text(encoding="utf-8"))
    notes = review.get("notes", "")
    counts["review.json notes"] = len(notes)
    if not notes:
        problems.append("review.json: notes missing")
    if len(notes) > REVIEW_NOTES_LIMIT:
        problems.append(f"review.json: notes are {len(notes)} characters, limit {REVIEW_NOTES_LIMIT}")
    for key in ("contactFirstName", "contactLastName", "contactPhone", "contactEmail"):
        if key in review:
            problems.append(f"review.json: {key} is personal data; enter it in App Store Connect")
    if re.search(r"[\w.+-]+@[\w-]+\.[\w.]+", notes):
        problems.append("review.json: notes contain an email address")
    if NETWORK_CLAIM.search(notes):
        problems.append("review.json: notes should say the app makes no network connections")


def workspace_version() -> str:
    cargo = (REPO / "Cargo.toml").read_text(encoding="utf-8")
    m = re.search(r"^\[workspace\.package\][^\[]*?^version\s*=\s*\"([^\"]+)\"", cargo, re.MULTILINE | re.DOTALL)
    if not m:
        raise SystemExit("Cargo.toml: no [workspace.package] version")
    return m.group(1)


def load_asc():
    spec = importlib.util.spec_from_file_location("asc", REPO / "scripts" / "asc" / "asc.py")
    asc = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(asc)
    return asc


def whats_new_problems(t: str) -> list[str]:
    problems = []
    if len(t) > WHATS_NEW_LIMIT:
        problems.append(f"{len(t)} characters as plain text, limit {WHATS_NEW_LIMIT}")
    bad = sorted({m.group(0) for rule in (FORBIDDEN, NETWORK_CLAIM) for m in rule.finditer(t)})
    if bad:
        problems.append(f"contains {', '.join(map(repr, bad))}, which the store listing must not")
    return problems


def check_whats_new(version: str, problems: list[str], counts: dict[str, int]) -> str:
    asc = load_asc()
    changelog = REPO / "CHANGELOG.md"
    versions = asc.changelog_versions(changelog.read_text(encoding="utf-8"))
    if versions and version == min(versions, key=lambda v: tuple(map(int, v.split(".")))):
        return f"What's New not checked: {version} is CHANGELOG.md's oldest version, the first store one"
    where = f"CHANGELOG.md ## {version} (What's New)"
    try:
        t = asc.whats_new(str(changelog), version, limit=None)
    except asc.Fail as e:
        problems.append(f"{where}: {e}")
        return ""
    counts["What's New"] = len(t)
    problems.extend(f"{where}: {p}" for p in whats_new_problems(t))
    return ""


def main() -> int:
    ap = argparse.ArgumentParser(description="Checks the Mac App Store listing in appstore/.")
    ap.add_argument("--version", help="the version being released (default: Cargo.toml's)")
    args = ap.parse_args()
    problems: list[str] = []
    counts: dict[str, int] = {}
    locales = sorted(p for p in (ROOT / "metadata").iterdir() if p.is_dir())
    if not locales:
        problems.append("metadata: no locales")
    for loc in locales:
        check_locale(loc, problems, counts)
    check_json(problems, counts)
    note = check_whats_new((args.version or workspace_version()).removeprefix("v"), problems, counts)
    for key, n in counts.items():
        print(f"{n:5d}  {key}")
    if note:
        print(note)
    for p in problems:
        print(f"error: {p}", file=sys.stderr)
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
