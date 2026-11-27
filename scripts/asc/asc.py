#!/usr/bin/env python3
"""App Store Connect API client for the Mac App Store edition (docs/RELEASING.md).

Standard library only, plus the openssl CLI (ES256 tokens, keys, CSRs, .p12 files) and
`xcrun altool` (upload). Run it with `python3 -I`:

  setup           the owner, once: bundle ID, certificates, profile and the CI secrets
  preflight       before a build: prints "go", or "skip: <why>", or fails when the version
                  cannot take a new build
  next-build      print the next free CFBundleVersion
  upload          upload a signed .pkg
  wait-build      wait until App Store Connect has processed a build
  prepare         version, build, metadata, screenshots, price and availability
  submit          submit the version for App Review
  check-metadata  validate appstore/ offline

The API key comes from --key-file/--key-id/--issuer-id or ASC_KEY_FILE (a path) or ASC_KEY_P8
(the key text), ASC_KEY_ID and ASC_ISSUER_ID; the app from --app-id, ASC_APP_ID or a lookup by
bundle ID. Key material, passwords and review contact details are never printed. --dry-run
prints the requests that would change something; it reads from the API only when a key is
given, and otherwise plans against an empty account.

What's New for an update is the version's "## <version>" section of CHANGELOG.md (--changelog)
as plain text, without the blocks about other editions; preflight checks it with
appstore/check.py's rules. An app's first version has none.
"""

import argparse
import base64
import hashlib
import importlib.util
import json
import os
import re
import secrets
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request

API = "https://api.appstoreconnect.apple.com"
USER_AGENT = "simple-capture-asc/1"
BUNDLE_ID = "com.simplecapture.desktop"
APP_NAME = "simple-capture"
REPO_ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
TOKEN_TTL = 1140
TOKEN_BACKDATE = 15
TOKEN_REFRESH = 600
WHATS_NEW_LIMIT = 4000
CHANGELOG_HEADING = r"^##\s+\[?v?(%s)\]?(?=[\s(]|$)"
OTHER_EDITION = re.compile(r"\b(?:Linux|Android|iOS|(?i:gifski|x264|x265))\b"
                           r"|\b(?i:on|for|in|to) Windows\b"
                           r"|\bWindows(?:[:,]| and Linux| installer| build| setup| edition)")

DESKTOP_SIZES = {(1280, 800), (1440, 900), (2560, 1600), (2880, 1800)}
MAX_SCREENSHOTS = 10
EDITABLE = {"PREPARE_FOR_SUBMISSION", "DEVELOPER_REJECTED", "REJECTED", "METADATA_REJECTED",
            "INVALID_BINARY", "READY_FOR_REVIEW"}
IN_REVIEW = {"WAITING_FOR_REVIEW", "IN_REVIEW", "WAITING_FOR_EXPORT_COMPLIANCE"}
EDITABLE_INFO = {"PREPARE_FOR_SUBMISSION", "DEVELOPER_REJECTED", "REJECTED", "READY_FOR_REVIEW"}
PRIVATE_ATTRS = {"contactFirstName", "contactLastName", "contactPhone", "contactEmail",
                 "demoAccountName", "demoAccountPassword", "csrContent", "profileContent",
                 "certificateContent"}
MAS_SECRETS = ("MAS_APP_CERT_P12", "MAS_INSTALLER_CERT_P12", "MAS_PROVISION_PROFILE",
               "MAS_CERT_PASSWORD")
REVIEW_CONTACT_ENV = {"contactFirstName": "ASC_REVIEW_CONTACT_FIRST_NAME",
                      "contactLastName": "ASC_REVIEW_CONTACT_LAST_NAME",
                      "contactPhone": "ASC_REVIEW_CONTACT_PHONE",
                      "contactEmail": "ASC_REVIEW_CONTACT_EMAIL"}

TEXT_FIELDS = {
    "name": ("info", "name", 30),
    "subtitle": ("info", "subtitle", 30),
    "privacy_url": ("info", "privacyPolicyUrl", None),
    "description": ("version", "description", 4000),
    "keywords": ("version", "keywords", 100),
    "promotional_text": ("version", "promotionalText", 170),
    "support_url": ("version", "supportUrl", None),
    "marketing_url": ("version", "marketingUrl", None),
}
URL_FIELDS = {"privacyPolicyUrl", "supportUrl", "marketingUrl"}
APP_INFO_KEYS = {
    "primaryCategory": "primaryCategory", "primary_category": "primaryCategory",
    "secondaryCategory": "secondaryCategory", "secondary_category": "secondaryCategory",
    "copyright": "copyright",
    "contentRightsDeclaration": "contentRightsDeclaration",
    "content_rights": "contentRightsDeclaration",
    "primaryLocale": "primaryLocale", "primary_locale": "primaryLocale",
    "bundleId": "bundleId", "bundle_id": "bundleId",
    "platform": "platform",
}
REVIEW_KEYS = {
    "notes": "notes",
    "demoAccountRequired": "demoAccountRequired", "demo_account_required": "demoAccountRequired",
    "demoAccountName": "demoAccountName", "demo_account_name": "demoAccountName",
    "demoAccountPassword": "demoAccountPassword",
    "demo_account_password": "demoAccountPassword",
}


class Fail(Exception):
    pass


class ApiError(Fail):
    def __init__(self, method, path, status, raw):
        self.status = status
        self.errors = []
        try:
            self.errors = json.loads(raw.decode("utf-8", "replace")).get("errors") or []
        except ValueError:
            pass
        detail = "; ".join(
            "%s %s: %s" % (e.get("code", ""), e.get("title", ""), e.get("detail", ""))
            for e in self.errors) or raw.decode("utf-8", "replace")[:300]
        super().__init__("%s %s: HTTP %s: %s" % (method, path, status, detail))


def log(msg):
    print(msg, file=sys.stderr, flush=True)


def b64url(data):
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode("ascii")


def _der_len(der, i):
    if i >= len(der):
        raise ValueError("truncated DER")
    n = der[i]
    i += 1
    if n < 0x80:
        return n, i
    k = n & 0x7F
    if k == 0 or k > 2 or i + k > len(der):
        raise ValueError("bad DER length")
    return int.from_bytes(der[i:i + k], "big"), i + k


def der_to_raw(der, size=32):
    """Converts a DER ECDSA signature (SEQUENCE of INTEGER r, s) to JWS's fixed-size r||s."""
    if len(der) < 8 or der[0] != 0x30:
        raise ValueError("not a DER ECDSA signature")
    n, i = _der_len(der, 1)
    if i + n != len(der):
        raise ValueError("bad DER sequence length")
    out = b""
    for _ in range(2):
        if i >= len(der) or der[i] != 0x02:
            raise ValueError("expected a DER INTEGER")
        n, i = _der_len(der, i + 1)
        value = der[i:i + n]
        if len(value) != n or n == 0:
            raise ValueError("truncated DER INTEGER")
        if value[0] & 0x80:
            raise ValueError("negative DER INTEGER")
        i += n
        value = value.lstrip(b"\0")
        if len(value) > size:
            raise ValueError("DER INTEGER longer than %d bytes" % size)
        out += value.rjust(size, b"\0")
    if i != len(der):
        raise ValueError("trailing bytes after the DER signature")
    return out


def openssl_sign(key_pem, data):
    """Signs data with ECDSA P-256/SHA-256 through the openssl CLI; returns the DER signature."""
    if len(key_pem) > 16384:
        raise Fail("the API key is too large to be an App Store Connect .p8 key")
    # The key goes through a pipe, so it never touches the disk.
    r, w = os.pipe()
    try:
        os.write(w, key_pem)
        os.close(w)
        w = -1
        proc = subprocess.run(["openssl", "dgst", "-sha256", "-sign", "/dev/fd/%d" % r],
                              input=data, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                              pass_fds=(r,))
    finally:
        if w >= 0:
            os.close(w)
        os.close(r)
    if proc.returncode != 0 or not proc.stdout:
        raise Fail("openssl could not sign with the API key (is it an App Store Connect .p8?)")
    return proc.stdout


def make_jwt(key_id, issuer, key_pem, now=None, ttl=TOKEN_TTL):
    """Returns an ES256 App Store Connect API token. iat is backdated and exp stays under Apple's
    20-minute limit, so a clock that runs a little fast still gets a token Apple accepts."""
    iat = int(time.time() if now is None else now) - TOKEN_BACKDATE
    header = {"alg": "ES256", "kid": key_id, "typ": "JWT"}
    payload = {"iss": issuer, "iat": iat, "exp": iat + min(ttl, TOKEN_TTL),
               "aud": "appstoreconnect-v1"}
    signing_input = ".".join(b64url(json.dumps(part, separators=(",", ":")).encode())
                             for part in (header, payload))
    signature = der_to_raw(openssl_sign(key_pem, signing_input.encode("ascii")))
    return signing_input + "." + b64url(signature)


class Key:
    """An App Store Connect team API key; repr never shows the key material."""

    def __init__(self, key_id, issuer, pem):
        self.key_id = key_id
        self.issuer = issuer
        self.pem = pem

    def __repr__(self):
        return "Key(%s)" % self.key_id


def load_key(key_id=None, issuer=None, key_file=None, env=True, required=True):
    """Reads an API key from the arguments or, with env, ASC_KEY_ID/ASC_ISSUER_ID/ASC_KEY_*."""
    environ = os.environ if env else {}
    key_id = key_id or environ.get("ASC_KEY_ID") or ""
    issuer = issuer or environ.get("ASC_ISSUER_ID") or ""
    key_file = key_file or environ.get("ASC_KEY_FILE") or ""
    pem = b""
    if key_file:
        try:
            with open(os.path.expanduser(key_file), "rb") as f:
                pem = f.read()
        except OSError as e:
            raise Fail("cannot read the API key file %s: %s" % (key_file, e.strerror))
        if not key_id:
            m = re.match(r"^(?:AuthKey|ApiKey)_([A-Z0-9]+)\.p8$", os.path.basename(key_file))
            key_id = m.group(1) if m else ""
    elif environ.get("ASC_KEY_P8"):
        pem = environ["ASC_KEY_P8"].encode("utf-8")
    if not (key_id or issuer or pem):
        if required:
            raise Fail("no App Store Connect API key: pass --key-file, --key-id and --issuer-id "
                       "(or set ASC_KEY_P8 or ASC_KEY_FILE, ASC_KEY_ID and ASC_ISSUER_ID)")
        return None
    if b"PRIVATE KEY-----" not in pem:
        raise Fail("the API key is missing or is not a .p8 private key")
    if not re.match(r"^[A-Z0-9]{8,12}$", key_id):
        raise Fail("the API Key ID should be about 10 letters and digits, like 2X9R4HXF34")
    if not re.match(r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-"
                    r"[0-9a-fA-F]{12}$", issuer):
        raise Fail("the Issuer ID should be a UUID (shown above the keys list in App Store "
                   "Connect)")
    return Key(key_id, issuer, pem)


def redact(value):
    if isinstance(value, dict):
        return {k: ("<redacted>" if k in PRIVATE_ATTRS and v not in (None, "") else redact(v))
                for k, v in value.items()}
    if isinstance(value, list):
        return [redact(v) for v in value]
    return value


def attrs(obj):
    return (obj or {}).get("attributes") or {}


def rel_id(obj, name):
    data = ((obj or {}).get("relationships") or {}).get(name) or {}
    data = data.get("data")
    return data.get("id") if isinstance(data, dict) else None


def ref(type_, id_):
    return {"data": {"type": type_, "id": id_}}


def changed(current, desired):
    return {k: v for k, v in desired.items() if current.get(k) != v}


class Api:
    """A JSON:API client for api.appstoreconnect.apple.com with token refresh and retries."""

    def __init__(self, key=None, dry_run=False):
        self.key = key
        self.dry_run = dry_run
        self._token = None
        self._token_at = 0.0
        self._fake = 0

    @property
    def offline(self):
        return self.dry_run and self.key is None

    def token(self):
        if self._token is None or time.time() - self._token_at > TOKEN_REFRESH:
            self._token = make_jwt(self.key.key_id, self.key.issuer, self.key.pem)
            self._token_at = time.time()
        return self._token

    def url(self, path, params=None):
        url = path if path.startswith("https://") else API + path
        if params:
            query = urllib.parse.urlencode(params, safe="[],")
            url += ("&" if "?" in url else "?") + query
        return url

    def call(self, method, path, params=None, body=None, missing_ok=False):
        url = self.url(path, params)
        if not url.startswith(API + "/"):
            raise Fail("refusing to send the API token to %s" % url.split("?")[0])
        short = url[len(API):]
        if self.dry_run and (method != "GET" or self.key is None):
            return self._dry(method, short, body)
        if self.key is None:
            raise Fail("no App Store Connect API key")
        data = json.dumps(body).encode("utf-8") if body is not None else None
        for attempt in range(6):
            req = urllib.request.Request(url, data=data, method=method, headers={
                "Authorization": "Bearer " + self.token(),
                "Content-Type": "application/json",
                "Accept": "application/json",
                "User-Agent": USER_AGENT,
            })
            try:
                with urllib.request.urlopen(req, timeout=120) as resp:
                    raw = resp.read()
                return json.loads(raw) if raw else {}
            except urllib.error.HTTPError as e:
                raw = e.read()
                if e.code == 404 and missing_ok:
                    return None
                retry = e.code == 429 or (e.code >= 500 and method == "GET")
                if retry and attempt < 5:
                    time.sleep(min(60, 2 ** (attempt + 1)))
                    continue
                raise ApiError(method, short.split("?")[0], e.code, raw)
            except (urllib.error.URLError, OSError) as e:
                if method == "GET" and attempt < 5:
                    time.sleep(min(60, 2 ** (attempt + 1)))
                    continue
                raise Fail("%s %s: %s" % (method, short.split("?")[0], e))
        raise Fail("%s %s: gave up after retries" % (method, short.split("?")[0]))

    def _dry(self, method, short, body):
        log("DRY RUN  %s %s" % (method, short))
        if body is not None:
            log("         " + json.dumps(redact(body), ensure_ascii=False)[:2000])
        if method == "GET":
            return {"data": None}
        if method == "POST":
            self._fake += 1
            data = dict(body.get("data") or {})
            data["id"] = "dry-run-%d" % self._fake
            return {"data": data}
        return {"data": (body or {}).get("data")}

    def get(self, path, params=None):
        doc = self.call("GET", path, params, missing_ok=True)
        return (doc or {}).get("data")

    def iter_all(self, path, params=None, included=None, max_pages=50):
        doc = self.call("GET", path, params, missing_ok=True)
        pages = 0
        while doc:
            data = doc.get("data")
            if isinstance(data, list):
                for item in data:
                    yield item
            elif data:
                yield data
            if included is not None:
                included.extend(doc.get("included") or [])
            pages += 1
            nxt = (doc.get("links") or {}).get("next")
            if not nxt or pages >= max_pages:
                return
            doc = self.call("GET", nxt)

    def get_all(self, path, params=None, included=None, max_pages=50):
        return list(self.iter_all(path, params, included, max_pages))

    def post(self, path, body):
        return (self.call("POST", path, body=body) or {}).get("data") or {}

    def patch(self, path, body):
        return (self.call("PATCH", path, body=body) or {}).get("data") or {}

    def delete(self, path):
        self.call("DELETE", path)


class Shot:
    def __init__(self, path, size):
        self.path = path
        self.name = os.path.basename(path)
        self.size = size
        with open(path, "rb") as f:
            self.data = f.read()
        self.md5 = hashlib.md5(self.data).hexdigest()


class Metadata:
    """The store listing in appstore/: per-locale text, app info, age rating, review notes and
    screenshots, already mapped onto App Store Connect attribute names."""

    def __init__(self):
        self.locales = {}
        self.app_info = {}
        self.age_rating = {}
        self.review = {}
        self.screenshots = {}


def png_size(path):
    with open(path, "rb") as f:
        head = f.read(24)
    if len(head) < 24 or head[:8] != b"\x89PNG\r\n\x1a\n" or head[12:16] != b"IHDR":
        return None
    return int.from_bytes(head[16:20], "big"), int.from_bytes(head[20:24], "big")


def _read_json(path, problems):
    try:
        with open(path, encoding="utf-8") as f:
            value = json.load(f)
    except ValueError as e:
        problems.append("%s: not valid JSON (%s)" % (path, e))
        return None
    if not isinstance(value, dict):
        problems.append("%s: expected a JSON object" % path)
        return None
    return {k: v for k, v in value.items() if not k.startswith("_")}


def load_metadata(root, bundle_id=BUNDLE_ID):
    """Loads and validates appstore/; raises Fail listing every problem found."""
    meta = Metadata()
    problems = []
    if not os.path.isdir(root):
        raise Fail("no metadata folder %s" % root)
    mroot = os.path.join(root, "metadata")
    for loc in sorted(os.listdir(mroot)) if os.path.isdir(mroot) else []:
        ldir = os.path.join(mroot, loc)
        if loc.startswith(".") or not os.path.isdir(ldir):
            continue
        entry = {"info": {}, "version": {}}
        for fn in sorted(os.listdir(ldir)):
            if fn.startswith("."):
                continue
            stem, ext = os.path.splitext(fn)
            path = os.path.join(ldir, fn)
            if fn == "release_notes.txt":
                problems.append("%s: not used; What's New comes from the version's section of "
                                "CHANGELOG.md, so delete this file" % path)
                continue
            if ext != ".txt" or stem not in TEXT_FIELDS:
                problems.append("%s: unknown file (expected one of %s)" % (
                    path, ", ".join(sorted(s + ".txt" for s in TEXT_FIELDS))))
                continue
            scope, attr, limit = TEXT_FIELDS[stem]
            with open(path, encoding="utf-8") as f:
                text = f.read().strip()
            if not text:
                continue
            if limit is not None and len(text) > limit:
                problems.append("%s: %d characters, the limit is %d" % (path, len(text), limit))
            if attr in URL_FIELDS and not re.match(r"^https?://\S+$", text):
                problems.append("%s: not a single http(s) URL" % path)
            if attr in ("name", "subtitle", "keywords") and "\n" in text:
                problems.append("%s: must be one line" % path)
            entry[scope][attr] = text
        meta.locales[loc] = entry
    if not meta.locales:
        problems.append("%s: no locale folders (like metadata/en-US)" % mroot)

    path = os.path.join(root, "app_info.json")
    if os.path.exists(path):
        raw = _read_json(path, problems) or {}
        for k, v in raw.items():
            key = APP_INFO_KEYS.get(k)
            if key is None:
                problems.append("%s: unknown key %r" % (path, k))
            elif v is not None:
                meta.app_info[key] = v
        if meta.app_info.pop("platform", "MAC_OS") != "MAC_OS":
            problems.append("%s: platform must be MAC_OS" % path)
        if meta.app_info.pop("bundleId", bundle_id) != bundle_id:
            problems.append("%s: bundleId is not %s" % (path, bundle_id))
        primary = meta.app_info.pop("primaryLocale", None)
        if primary and primary not in meta.locales:
            problems.append("%s: primaryLocale %s has no metadata folder" % (path, primary))
        rights = meta.app_info.get("contentRightsDeclaration")
        if rights not in (None, "DOES_NOT_USE_THIRD_PARTY_CONTENT", "USES_THIRD_PARTY_CONTENT"):
            problems.append("%s: contentRightsDeclaration %r is not a valid value" % (path, rights))
        for k in ("primaryCategory", "secondaryCategory", "copyright"):
            if k in meta.app_info and not isinstance(meta.app_info[k], str):
                problems.append("%s: %s must be a string" % (path, k))

    path = os.path.join(root, "age_rating.json")
    if os.path.exists(path):
        raw = _read_json(path, problems) or {}
        for k, v in raw.items():
            if not re.match(r"^[a-z][A-Za-z0-9]*$", k):
                problems.append("%s: %r is not an age-rating attribute name" % (path, k))
            elif not (v is None or isinstance(v, bool)
                      or (isinstance(v, str) and re.match(r"^[A-Z0-9_]+$", v))
                      or (isinstance(v, str) and k == "developerAgeRatingInfoUrl")):
                problems.append("%s: %s has an unexpected value %r" % (path, k, v))
            else:
                meta.age_rating[k] = v

    path = os.path.join(root, "review.json")
    if os.path.exists(path):
        raw = _read_json(path, problems) or {}
        for k, v in raw.items():
            key = REVIEW_KEYS.get(k)
            if k.lower().startswith("contact"):
                problems.append("%s: %s is personal data; put review contact details in the "
                                "ASC_REVIEW_CONTACT_* secrets, not in the repository" % (path, k))
            elif key is None:
                problems.append("%s: unknown key %r" % (path, k))
            elif v is not None:
                meta.review[key] = v
        if "demoAccountRequired" in meta.review and \
                not isinstance(meta.review["demoAccountRequired"], bool):
            problems.append("%s: demoAccountRequired must be true or false" % path)
        if len(meta.review.get("notes") or "") > 4000:
            problems.append("%s: notes are over 4000 characters" % path)

    sroot = os.path.join(root, "screenshots")
    for loc in sorted(os.listdir(sroot)) if os.path.isdir(sroot) else []:
        ldir = os.path.join(sroot, loc)
        if loc.startswith(".") or not os.path.isdir(ldir):
            continue
        if loc not in meta.locales:
            if re.match(r"^[a-z]{2,3}(-[A-Za-z0-9]{2,4}){0,2}$", loc):
                problems.append("%s: no metadata/%s folder for these screenshots" % (ldir, loc))
            continue
        shots = []
        for fn in sorted(os.listdir(ldir)):
            path = os.path.join(ldir, fn)
            ext = os.path.splitext(fn)[1].lower()
            if fn.startswith(".") or os.path.isdir(path) or ext in (".md", ".txt"):
                continue
            if ext != ".png":
                problems.append("%s: screenshots must be .png files" % path)
                continue
            size = png_size(path)
            if size is None:
                problems.append("%s: not a PNG image" % path)
            elif size not in DESKTOP_SIZES:
                problems.append("%s: %dx%d; Mac screenshots must be one of %s" % (
                    path, size[0], size[1],
                    ", ".join("%dx%d" % s for s in sorted(DESKTOP_SIZES))))
            else:
                shots.append(Shot(path, size))
        if len(shots) > MAX_SCREENSHOTS:
            problems.append("%s: %d screenshots; the App Store takes at most %d" % (
                ldir, len(shots), MAX_SCREENSHOTS))
        if shots:
            meta.screenshots[loc] = shots
    if problems:
        raise Fail("appstore metadata problems:\n  " + "\n  ".join(problems))
    return meta


def changelog_section(text, version):
    """Returns the body of the "## <version>" section (a suffix like "(unreleased)" is fine), or
    None when CHANGELOG.md has no such section."""
    heading = re.compile(CHANGELOG_HEADING % re.escape(version))
    lines = text.splitlines()
    for i, line in enumerate(lines):
        if heading.match(line):
            body = []
            for nxt in lines[i + 1:]:
                if re.match(r"#{1,2}\s", nxt):
                    break
                body.append(nxt)
            return "\n".join(body).strip()
    return None


def changelog_versions(text):
    heading = re.compile(CHANGELOG_HEADING % r"\d+(?:\.\d+)*")
    return [m.group(1) for m in map(heading.match, text.splitlines()) if m]


def _md_blocks(md):
    blocks = []
    for line in md.splitlines():
        s = line.strip()
        head = re.match(r"(#{1,6})\s", s)
        item = re.match(r"( *)(?:[-*+]|\d+[.)])\s", line.expandtabs(4))
        if not s:
            blocks.append(["blank", 0, [line]])
        elif head:
            blocks.append(["head", len(head.group(1)), [line]])
        elif item:
            blocks.append(["item", len(item.group(1)), [line]])
        elif blocks and blocks[-1][0] in ("item", "para"):
            blocks[-1][2].append(line)
        else:
            blocks.append(["para", 0, [line]])
    return blocks


def drop_other_editions(md):
    """Leaves out the headings, list items and paragraphs of a CHANGELOG section that match
    OTHER_EDITION, and the headings that leaves empty. Returns (Markdown, blocks left out)."""
    kept, dropped = [], 0
    in_head = in_item = None
    for block in _md_blocks(re.sub(r"(?s)<!--.*?-->", "", md)):
        kind, level, lines = block
        text = " ".join(line.strip() for line in lines)
        if kind == "head":
            in_item = None
            if in_head is not None and level <= in_head:
                in_head = None
        elif kind == "para" or (kind == "item" and in_item is not None and level <= in_item):
            in_item = None
        if kind != "blank":
            if in_head is not None or in_item is not None:
                dropped += 1
                continue
            if OTHER_EDITION.search(text) or (
                    kind == "head" and re.fullmatch(r"#+\s+Windows", text)):
                in_head = level if kind == "head" else None
                in_item = level if kind == "item" else None
                dropped += 1
                continue
        kept.append(block)
    out = []
    for block in reversed(kept):
        nxt = next((b for b in reversed(out) if b[0] != "blank"), None)
        if block[0] == "head" and (nxt is None or nxt[0] == "head" and nxt[1] <= block[1]):
            continue
        out.append(block)
    return "\n".join(line for _, _, lines in reversed(out) for line in lines).strip(), dropped


def _md_inline(text):
    parts = re.split(r"(`+)(.+?)\1", text)
    out = []
    for i, part in enumerate(parts):
        if i % 3 == 2:
            out.append(part)
        elif i % 3 == 0:
            part = re.sub(r"!?\[([^\]]*)\]\([^)]*\)", r"\1", part)
            part = re.sub(r"\[([^\]]+)\]\[[^\]]*\]", r"\1", part)
            part = re.sub(r"<((?:https?|mailto):[^>\s]+)>", r"\1", part)
            part = re.sub(r"\*\*(.+?)\*\*", r"\1", part)
            part = re.sub(r"(?<![\w*\\])\*(?=\S)(.+?)(?<=[^\s\\])\*(?![\w*])", r"\1", part)
            part = re.sub(r"\\([\\`*_{}\[\]()#+\-.!<>|])", r"\1", part)
            out.append(part.replace("`", ""))
    return "".join(out)


def markdown_to_text(md):
    """Turns CHANGELOG Markdown into App Store plain text: headings become plain lines, bullets
    "• ", wrapped lines are joined, links keep their text, and emphasis and code marks go."""
    blocks = []
    joinable = gap = False
    for line in re.sub(r"(?s)<!--.*?-->", "", md).splitlines():
        s = line.strip()
        head = re.match(r"#{1,6}\s+(.*?)(?:\s+#+)?$", s)
        item = re.match(r"( *)(?:[-*+]|(\d+[.)]))\s+(.*)$", line.expandtabs(4))
        if not s or re.fullmatch(r"([-*_])( *\1){2,}", s) or re.match(r"\[[^\]]+\]:\s", s):
            joinable, gap = False, True
        elif head:
            blocks.append(["head", head.group(1), gap])
            joinable = gap = False
        elif item:
            mark = item.group(2) + " " if item.group(2) else "• "
            blocks.append(["item", "  " * (len(item.group(1)) // 2) + mark + item.group(3), gap])
            joinable, gap = True, False
        elif joinable:
            blocks[-1][1] += " " + s
        else:
            blocks.append(["para", s, gap])
            joinable, gap = True, False
    out = ""
    prev = None
    for kind, text, gap in blocks:
        if prev:
            out += "\n" if prev == "head" or (prev == kind == "item" and not gap) else "\n\n"
        out += _md_inline(text)
        prev = kind
    return out.strip()


def whats_new(path, version, limit=WHATS_NEW_LIMIT):
    """What's New for an update: the version's CHANGELOG.md section as plain text without the
    blocks about other editions, cut after a whole line when it is over limit (None: never)."""
    try:
        with open(path, encoding="utf-8") as f:
            section = changelog_section(f.read(), version)
    except OSError as e:
        raise Fail("cannot read %s: %s" % (path, e.strerror))
    if not markdown_to_text(section or ""):
        raise Fail("%s has no \"## %s\" section; an update's What's New comes from it" % (
            path, version))
    section, dropped = drop_other_editions(section)
    text = markdown_to_text(section)
    if dropped:
        log("What's New: left out %d block%s of the %s section about other editions" % (
            dropped, "" if dropped == 1 else "s", version))
    if not text:
        raise Fail("the \"## %s\" section of %s is only about other editions; What's New "
                   "needs a line about the Mac App Store edition" % (version, path))
    if limit is not None and len(text) > limit:
        cut = text[:limit - 2]
        nl = cut.rfind("\n")
        if nl > limit // 2:
            cut = cut[:nl]
        log("What's New: the %s section is %d characters; cut to %d" % (version, len(text),
                                                                        limit))
        text = cut.rstrip() + "\n…"
    return text


def whats_new_problems(text):
    path = os.path.join(REPO_ROOT, "appstore", "check.py")
    spec = importlib.util.spec_from_file_location("appstore_check", path)
    check = importlib.util.module_from_spec(spec)
    try:
        spec.loader.exec_module(check)
    except OSError as e:
        raise Fail("cannot read %s: %s" % (path, e.strerror))
    return check.whats_new_problems(text)


def review_contact_from_env():
    return {attr: os.environ[env].strip() for attr, env in REVIEW_CONTACT_ENV.items()
            if os.environ.get(env, "").strip()}


def lead_int(version):
    m = re.match(r"^(\d+)", version or "")
    return int(m.group(1)) if m else None


def version_state(version):
    a = attrs(version)
    return a.get("appVersionState") or a.get("appStoreState")


def find_app(api, bundle_id):
    for app in api.iter_all("/v1/apps", {"filter[bundleId]": bundle_id, "limit": 200}):
        if attrs(app).get("bundleId") == bundle_id:
            return app
    return None


def resolve_app_id(api, args):
    app_id = args.app_id or os.environ.get("ASC_APP_ID", "")
    if app_id:
        if not re.match(r"^\d+$", app_id):
            raise Fail("the app ID is the app's numeric Apple ID (App Information in App Store "
                       "Connect)")
        return app_id
    app = find_app(api, args.bundle_id)
    if app:
        return app["id"]
    if api.offline:
        return "APP_ID"
    raise Fail("App Store Connect has no app with bundle ID %s; create it in App Store Connect "
               "(Apps, +, New App) first" % args.bundle_id)


def find_builds(api, app_id, version, build):
    return api.get_all("/v1/builds", {
        "filter[app]": app_id,
        "filter[version]": build,
        "filter[preReleaseVersion.version]": version,
        "filter[preReleaseVersion.platform]": "MAC_OS",
        "limit": 10,
    })


def upload_errors(api, app_id, version, build):
    try:
        uploads = api.get_all("/v1/apps/%s/buildUploads" % app_id, {
            "filter[cfBundleVersion]": build,
            "filter[cfBundleShortVersionString]": version,
            "filter[platform]": "MAC_OS",
            "limit": 10,
        })
    except ApiError:
        return None
    for upload in uploads:
        state = attrs(upload).get("state") or {}
        if state.get("state") == "FAILED":
            return ["%s: %s" % (e.get("code"), e.get("description"))
                    for e in state.get("errors") or []] or ["the upload failed"]
    return None


def mac_versions(api, app_id):
    return api.get_all("/v1/apps/%s/appStoreVersions" % app_id,
                       {"filter[platform]": "MAC_OS", "limit": 200})


def find_version(api, app_id, version_string):
    for v in mac_versions(api, app_id):
        if attrs(v).get("versionString") == version_string:
            return v
    return None


def is_first_version(versions, version_string):
    """True while no other macOS version has left the editable states, that is while the app
    has never been out: App Store Connect takes What's New only after that."""
    return all(version_state(v) in EDITABLE for v in versions
               if attrs(v).get("versionString") != version_string)


def attached_build_id(api, version_id):
    build = api.get("/v1/appStoreVersions/%s/build" % version_id)
    return build.get("id") if build else None


def open_submissions(api, app_id, states):
    return api.get_all("/v1/reviewSubmissions", {
        "filter[app]": app_id,
        "filter[platform]": "MAC_OS",
        "filter[state]": ",".join(states),
        "include": "appStoreVersionForReview",
        "limit": 50,
    })


def wait_for(what, check, timeout, interval):
    deadline = time.time() + timeout
    while True:
        done = check()
        if done:
            return done
        if time.time() > deadline:
            raise Fail("timed out waiting for %s" % what)
        time.sleep(interval)


def cancel_review(api, app_id, version):
    for sub in open_submissions(api, app_id, ["WAITING_FOR_REVIEW", "IN_REVIEW"]):
        if rel_id(sub, "appStoreVersionForReview") == version["id"]:
            log("canceling review submission %s" % sub["id"])
            api.patch("/v1/reviewSubmissions/%s" % sub["id"], {"data": {
                "type": "reviewSubmissions", "id": sub["id"], "attributes": {"canceled": True}}})
    if api.dry_run:
        return version

    def editable():
        v = api.get("/v1/appStoreVersions/%s" % version["id"])
        return v if version_state(v) in EDITABLE else None

    return wait_for("the canceled version to become editable", editable, 900, 15)


def ensure_version(api, app_id, versions, version_string, build_id, replace_waiting,
                   copyright_text):
    """Finds, renames or creates the editable MAC_OS version; None when it is already in review
    with this build."""
    mine = [v for v in versions if attrs(v).get("versionString") == version_string]
    if mine:
        v = mine[0]
        state = version_state(v)
        if state in IN_REVIEW:
            if attached_build_id(api, v["id"]) == build_id:
                log("version %s is %s with this build already; nothing to prepare" % (
                    version_string, state))
                return None
            if not replace_waiting:
                raise Fail("version %s is %s with another build; pass --replace-waiting to "
                           "cancel that submission and attach this build" % (version_string,
                                                                              state))
            v = cancel_review(api, app_id, v)
        elif state not in EDITABLE and not api.offline:
            raise Fail("version %s is already %s; release a new version number instead" % (
                version_string, state))
        log("using version %s (%s)" % (version_string, version_state(v)))
    else:
        editable = [v for v in versions if version_state(v) in EDITABLE]
        if editable:
            v = editable[0]
            log("renaming the editable version %s to %s" % (attrs(v).get("versionString"),
                                                           version_string))
            v = api.patch("/v1/appStoreVersions/%s" % v["id"], {"data": {
                "type": "appStoreVersions", "id": v["id"],
                "attributes": {"versionString": version_string}}}) or v
        else:
            log("creating version %s" % version_string)
            body_attrs = {"platform": "MAC_OS", "versionString": version_string,
                          "releaseType": "AFTER_APPROVAL"}
            if copyright_text:
                body_attrs["copyright"] = copyright_text
            v = api.post("/v1/appStoreVersions", {"data": {
                "type": "appStoreVersions", "attributes": body_attrs,
                "relationships": {"app": ref("apps", app_id)}}})
    desired = {"releaseType": "AFTER_APPROVAL"}
    if copyright_text:
        desired["copyright"] = copyright_text
    diff = changed(attrs(v), desired)
    if diff:
        log("version %s: setting %s" % (version_string, ", ".join(sorted(diff))))
        api.patch("/v1/appStoreVersions/%s" % v["id"], {"data": {
            "type": "appStoreVersions", "id": v["id"], "attributes": diff}})
    return v


def pick_app_info(api, app_id):
    infos = api.get_all("/v1/apps/%s/appInfos" % app_id,
                        {"include": "primaryCategory,secondaryCategory", "limit": 50})
    for info in infos:
        if attrs(info).get("state") in EDITABLE_INFO or \
                attrs(info).get("appStoreState") in EDITABLE:
            return info
    if infos:
        return infos[0]
    if api.offline:
        return {"id": "APP_INFO_ID", "attributes": {}}
    raise Fail("the app has no app info record")


def sync_categories(api, info, app_info):
    desired = {k: app_info[k] for k in ("primaryCategory", "secondaryCategory") if k in app_info}
    if not desired:
        return
    valid = {c["id"] for c in api.iter_all("/v1/appCategories", {
        "filter[platforms]": "MAC_OS", "exists[parent]": "false", "limit": 200})}
    bad = [v for v in desired.values() if valid and v not in valid]
    if bad:
        raise Fail("unknown macOS App Store categories %s; valid: %s" % (
            ", ".join(bad), ", ".join(sorted(valid))))
    rels = {k: ref("appCategories", v) for k, v in desired.items() if rel_id(info, k) != v}
    if rels:
        log("app info: setting %s" % ", ".join(sorted(rels)))
        api.patch("/v1/appInfos/%s" % info["id"], {"data": {
            "type": "appInfos", "id": info["id"], "relationships": rels}})


def sync_info_localizations(api, info, meta):
    existing = {attrs(l).get("locale"): l for l in api.iter_all(
        "/v1/appInfos/%s/appInfoLocalizations" % info["id"], {"limit": 200})}
    for loc, entry in sorted(meta.locales.items()):
        desired = entry["info"]
        if not desired:
            continue
        cur = existing.get(loc)
        if cur:
            diff = changed(attrs(cur), desired)
            if diff:
                log("app info %s: setting %s" % (loc, ", ".join(sorted(diff))))
                api.patch("/v1/appInfoLocalizations/%s" % cur["id"], {"data": {
                    "type": "appInfoLocalizations", "id": cur["id"], "attributes": diff}})
        else:
            if "name" not in desired:
                raise Fail("metadata/%s needs a name.txt to add that locale" % loc)
            log("app info %s: adding the locale" % loc)
            body_attrs = dict(desired, locale=loc)
            api.post("/v1/appInfoLocalizations", {"data": {
                "type": "appInfoLocalizations", "attributes": body_attrs,
                "relationships": {"appInfo": ref("appInfos", info["id"])}}})


def sync_age_rating(api, info, age_rating):
    if not age_rating:
        return
    decl = api.get("/v1/appInfos/%s/ageRatingDeclaration" % info["id"])
    if decl is None:
        if not api.offline:
            raise Fail("the app info has no age rating declaration")
        decl = {"id": "AGE_RATING_ID", "attributes": {}}
    diff = changed(attrs(decl), age_rating)
    if diff:
        log("age rating: setting %s" % ", ".join(sorted(diff)))
        api.patch("/v1/ageRatingDeclarations/%s" % decl["id"], {"data": {
            "type": "ageRatingDeclarations", "id": decl["id"], "attributes": diff}})


def sync_content_rights(api, app_id, value):
    if not value:
        return
    app = api.get("/v1/apps/%s" % app_id) or {}
    if attrs(app).get("contentRightsDeclaration") != value:
        log("app: setting contentRightsDeclaration")
        api.patch("/v1/apps/%s" % app_id, {"data": {
            "type": "apps", "id": app_id, "attributes": {"contentRightsDeclaration": value}}})


def ensure_free_price(api, app_id):
    schedule = api.get("/v1/apps/%s/appPriceSchedule" % app_id)
    if schedule and api.get_all("/v1/appPriceSchedules/%s/manualPrices" % schedule["id"],
                                {"limit": 1}, max_pages=1):
        log("price: already set, left as it is")
        return
    free = None
    for point in api.iter_all("/v1/apps/%s/appPricePoints" % app_id,
                              {"filter[territory]": "USA", "limit": 200}):
        try:
            if float(attrs(point).get("customerPrice")) == 0:
                free = point["id"]
                break
        except (TypeError, ValueError):
            continue
    if free is None:
        if not api.offline:
            raise Fail("found no free (0.00) price point for the USA")
        free = "FREE_PRICE_POINT_ID"
    log("price: free in every territory (base territory USA)")
    api.post("/v1/appPriceSchedules", {
        "data": {
            "type": "appPriceSchedules",
            "attributes": {},
            "relationships": {
                "app": ref("apps", app_id),
                "baseTerritory": ref("territories", "USA"),
                "manualPrices": {"data": [{"type": "appPrices", "id": "${price-usa}"}]},
            },
        },
        "included": [{
            "type": "appPrices",
            "id": "${price-usa}",
            "attributes": {"startDate": None, "endDate": None},
            "relationships": {"appPricePoint": ref("appPricePoints", free)},
        }],
    })


def ensure_availability(api, app_id):
    current = api.get("/v1/apps/%s/appAvailabilityV2" % app_id)
    if current:
        available = sum(1 for t in api.iter_all(
            "/v2/appAvailabilities/%s/territoryAvailabilities" % current["id"], {"limit": 200})
            if attrs(t).get("available"))
        if available:
            log("availability: already set (%d territories), left as it is" % available)
        else:
            log("warning: the app is available in no territory; set it in App Store Connect "
                "(Pricing and Availability)")
        return
    territories = [t["id"] for t in api.iter_all("/v1/territories", {"limit": 200})]
    if not territories:
        if not api.offline:
            raise Fail("App Store Connect listed no territories")
        territories = ["USA"]
    log("availability: all %d territories, and new ones as they come" % len(territories))
    api.post("/v2/appAvailabilities", {
        "data": {
            "type": "appAvailabilities",
            "attributes": {"availableInNewTerritories": True},
            "relationships": {
                "app": ref("apps", app_id),
                "territoryAvailabilities": {"data": [
                    {"type": "territoryAvailabilities", "id": "${t-%s}" % t}
                    for t in territories]},
            },
        },
        "included": [{
            "type": "territoryAvailabilities",
            "id": "${t-%s}" % t,
            "attributes": {"available": True},
            "relationships": {"territory": ref("territories", t)},
        } for t in territories],
    })


def sync_version_localizations(api, version_id, meta, notes):
    existing = {attrs(l).get("locale"): l for l in api.iter_all(
        "/v1/appStoreVersions/%s/appStoreVersionLocalizations" % version_id, {"limit": 200})}
    ids = {}
    for loc, entry in sorted(meta.locales.items()):
        desired = dict(entry["version"])
        if notes:
            desired["whatsNew"] = notes
        cur = existing.get(loc)
        if cur:
            ids[loc] = cur["id"]
            diff = changed(attrs(cur), desired)
            if diff:
                log("version %s: setting %s" % (loc, ", ".join(sorted(diff))))
                api.patch("/v1/appStoreVersionLocalizations/%s" % cur["id"], {"data": {
                    "type": "appStoreVersionLocalizations", "id": cur["id"],
                    "attributes": diff}})
        elif desired or loc in meta.screenshots:
            log("version %s: adding the locale" % loc)
            created = api.post("/v1/appStoreVersionLocalizations", {"data": {
                "type": "appStoreVersionLocalizations",
                "attributes": dict(desired, locale=loc),
                "relationships": {"appStoreVersion": ref("appStoreVersions", version_id)}}})
            ids[loc] = created.get("id")
    return ids


def sync_review_detail(api, version_id, review):
    desired = dict(review)
    desired.update(review_contact_from_env())
    if not desired:
        return
    cur = api.get("/v1/appStoreVersions/%s/appStoreReviewDetail" % version_id)
    if cur:
        diff = changed(attrs(cur), desired)
        if diff:
            log("review details: setting %s" % ", ".join(sorted(diff)))
            api.patch("/v1/appStoreReviewDetails/%s" % cur["id"], {"data": {
                "type": "appStoreReviewDetails", "id": cur["id"], "attributes": diff}})
    else:
        log("review details: adding %s" % ", ".join(sorted(desired)))
        api.post("/v1/appStoreReviewDetails", {"data": {
            "type": "appStoreReviewDetails", "attributes": desired,
            "relationships": {"appStoreVersion": ref("appStoreVersions", version_id)}}})


def put_parts(operations, data):
    for op in operations:
        offset, length = int(op.get("offset") or 0), int(op.get("length") or len(data))
        headers = {h["name"]: h["value"] for h in op.get("requestHeaders") or []}
        req = urllib.request.Request(op["url"], data=data[offset:offset + length],
                                     method=op.get("method") or "PUT", headers=headers)
        for attempt in range(4):
            try:
                with urllib.request.urlopen(req, timeout=300) as resp:
                    resp.read()
                break
            except (urllib.error.URLError, OSError) as e:
                if attempt == 3:
                    raise Fail("uploading a screenshot part failed: %s" % e)
                time.sleep(2 ** (attempt + 1))


def upload_screenshot(api, set_id, shot):
    log("screenshots: uploading %s" % shot.name)
    res = api.post("/v1/appScreenshots", {"data": {
        "type": "appScreenshots",
        "attributes": {"fileName": shot.name, "fileSize": len(shot.data)},
        "relationships": {"appScreenshotSet": ref("appScreenshotSets", set_id)}}})
    if api.dry_run:
        log("DRY RUN  PUT %d bytes to the upload URLs App Store Connect returns" % len(shot.data))
    else:
        put_parts(attrs(res).get("uploadOperations") or [], shot.data)
    api.patch("/v1/appScreenshots/%s" % res["id"], {"data": {
        "type": "appScreenshots", "id": res["id"],
        "attributes": {"uploaded": True, "sourceFileChecksum": shot.md5}}})
    if api.dry_run:
        return res["id"]

    def processed():
        state = attrs(api.get("/v1/appScreenshots/%s" % res["id"])).get(
            "assetDeliveryState") or {}
        if state.get("state") == "FAILED":
            raise Fail("App Store Connect rejected %s: %s" % (shot.name, "; ".join(
                "%s %s" % (e.get("code"), e.get("description"))
                for e in state.get("errors") or [])))
        return state.get("state") == "COMPLETE"

    wait_for("%s to process" % shot.name, processed, 600, 5)
    return res["id"]


def sync_screenshots(api, loc_id, loc, shots):
    sets = api.get_all("/v1/appStoreVersionLocalizations/%s/appScreenshotSets" % loc_id,
                       {"filter[screenshotDisplayType]": "APP_DESKTOP", "limit": 10})
    if sets:
        set_id = sets[0]["id"]
    else:
        set_id = api.post("/v1/appScreenshotSets", {"data": {
            "type": "appScreenshotSets",
            "attributes": {"screenshotDisplayType": "APP_DESKTOP"},
            "relationships": {"appStoreVersionLocalization": ref(
                "appStoreVersionLocalizations", loc_id)}}})["id"]
    current = api.get_all("/v1/appScreenshotSets/%s/appScreenshots" % set_id, {"limit": 50})
    reusable = {}
    stale = []
    for s in current:
        a = attrs(s)
        state = (a.get("assetDeliveryState") or {}).get("state")
        key = a.get("sourceFileChecksum")
        if state in ("COMPLETE", "UPLOAD_COMPLETE") and key and key not in reusable:
            reusable[key] = s["id"]
        else:
            stale.append(s["id"])
    wanted = [reusable.pop(shot.md5, None) for shot in shots]
    stale.extend(reusable.values())
    for sid in stale:
        log("screenshots %s: removing an old or changed screenshot" % loc)
        api.delete("/v1/appScreenshots/%s" % sid)
    order = [sid or upload_screenshot(api, set_id, shot) for sid, shot in zip(wanted, shots)]
    remaining = [s["id"] for s in current if s["id"] not in stale]
    if order != remaining[:len(order)] or len(remaining) != len(order):
        api.patch("/v1/appScreenshotSets/%s/relationships/appScreenshots" % set_id, {
            "data": [{"type": "appScreenshots", "id": sid} for sid in order]})
    log("screenshots %s: %d in place" % (loc, len(order)))


def cmd_check_metadata(args):
    meta = load_metadata(args.metadata, args.bundle_id)
    shots = sum(len(v) for v in meta.screenshots.values())
    log("appstore metadata OK: locales %s, %d screenshots, review details %s" % (
        ", ".join(sorted(meta.locales)), shots, "yes" if meta.review else "none"))
    return 0


def cmd_preflight(api, args):
    app_id = resolve_app_id(api, args)
    versions = mac_versions(api, app_id)
    mine = next((v for v in versions if attrs(v).get("versionString") == args.version), None)
    state = version_state(mine) if mine else None
    if mine is None:
        busy = [v for v in versions if version_state(v) in IN_REVIEW]
        if busy:
            raise Fail("version %s is %s; App Store Connect takes no new version until that "
                       "review ends or is canceled" % (attrs(busy[0]).get("versionString"),
                                                       version_state(busy[0])))
    elif state in IN_REVIEW and not args.replace_waiting:
        if args.skip_if_submitted:
            print("skip: version %s is %s already; run the workflow with replace_waiting=true "
                  "to replace its build" % (args.version, state))
            return 0
        raise Fail("version %s is %s; pass --replace-waiting (replace_waiting=true) to cancel "
                   "that submission and replace its build, or wait for the review" % (
                       args.version, state))
    elif state not in EDITABLE and state not in IN_REVIEW:
        if args.skip_if_submitted:
            print("skip: version %s is %s already; nothing to upload" % (args.version, state))
            return 0
        raise Fail("version %s is already %s; App Store Connect takes no new build for it, so "
                   "release a new version number" % (args.version, state))
    if is_first_version(versions, args.version):
        log("version %s is the app's first, so it has no What's New" % args.version)
    else:
        text = whats_new(args.changelog, args.version, limit=None)
        problems = whats_new_problems(text)
        if problems:
            raise Fail("What's New for %s, from %s, breaks the store listing's rules "
                       "(appstore/check.py):\n  %s" % (args.version, args.changelog,
                                                       "\n  ".join(problems)))
        log("What's New: %d characters from %s" % (len(text), args.changelog))
    log("version %s: %s" % (args.version, state or "not in App Store Connect yet"))
    print("go")
    return 0


def cmd_next_build(api, args):
    numbers = []
    if not api.offline:
        app_id = resolve_app_id(api, args)
        for b in api.iter_all("/v1/builds", {"filter[app]": app_id, "sort": "-uploadedDate",
                                             "limit": 200}, max_pages=5):
            numbers.append(lead_int(attrs(b).get("version")))
        try:
            for u in api.iter_all("/v1/apps/%s/buildUploads" % app_id,
                                  {"sort": "-uploadedDate", "limit": 200}, max_pages=5):
                numbers.append(lead_int(attrs(u).get("cfBundleVersion")))
        except ApiError:
            pass
    print(max([args.at_least] + [n + 1 for n in numbers if n is not None]))
    return 0


ALTOOL_FAILED = re.compile(r"ERROR ITMS-\d+|UPLOAD FAILED|^\s*\*\*\* Error")
ALTOOL_SUCCEEDED = re.compile(r"No errors uploading|UPLOAD SUCCEEDED")
ALTOOL_BAD_OPTION = re.compile(
    r"(?i)(unknown|unrecognized|unexpected|invalid)\s+(option|argument)[^\n]*--api")


def altool_json(text):
    decoder = json.JSONDecoder()
    for m in re.finditer(r"(?m)^[ \t]*\{", text):
        try:
            doc, _ = decoder.raw_decode(text, m.end() - 1)
        except ValueError:
            continue
        if isinstance(doc, dict):
            return doc
    return None


def altool_errors(returncode, stdout, stderr):
    """Xcode 26's altool can exit 0 after a failed upload, so success also needs its JSON result
    on stdout or a success line."""
    doc = altool_json(stdout)
    errors = []
    if doc is not None:
        errors = [str(e.get("message") or e) if isinstance(e, dict) else str(e)
                  for e in doc.get("product-errors") or []]
    both = stdout + "\n" + stderr
    if not errors:
        errors = [line.strip() for line in both.splitlines() if ALTOOL_FAILED.search(line)][:5]
    if not errors and doc is None and not (returncode == 0 and ALTOOL_SUCCEEDED.search(both)):
        errors = ["altool printed no JSON result"]
    if not errors and returncode != 0:
        errors = ["altool exit %d" % returncode]
    return errors


def altool_upload_cmd(pkg, app_id, args, key_id, issuer, modern):
    """Xcode 26's altool documents --api-key/--api-issuer, older ones --apiKey/--apiIssuer."""
    return ["xcrun", "altool", "--upload-package", pkg, "-t", "macos",
            "--apple-id", app_id, "--bundle-id", args.bundle_id, "--bundle-version", args.build,
            "--bundle-short-version-string", args.version, "--output-format", "json"] + (
        ["--api-key", key_id, "--api-issuer", issuer] if modern
        else ["--apiKey", key_id, "--apiIssuer", issuer])


def cmd_upload(api, args):
    pkg = os.path.abspath(args.pkg)
    if not pkg.endswith(".pkg") or not os.path.isfile(pkg):
        raise Fail("no .pkg at %s" % args.pkg)
    key = api.key
    app_id = resolve_app_id(api, args)
    helptext = ""
    if shutil.which("xcrun"):
        probe = subprocess.run(["xcrun", "altool", "--help"], stdout=subprocess.PIPE,
                               stderr=subprocess.STDOUT, text=True)
        helptext = probe.stdout
    if not api.dry_run and (not helptext.strip() or "unable to find utility" in helptext):
        raise Fail("xcrun altool is not available (it comes with Xcode, not the Command Line "
                   "Tools)")
    modern = "--api-key" in helptext or "--apiKey" not in helptext
    key_id = key.key_id if key else "KEY_ID"
    issuer = key.issuer if key else "ISSUER_ID"
    if api.dry_run:
        log("DRY RUN  " + " ".join(altool_upload_cmd(pkg, app_id, args, key_id, issuer,
                                                     modern)))
        return 0
    if key is None:
        raise Fail("no App Store Connect API key")
    keydir = tempfile.mkdtemp(prefix="asc-keys-")
    try:
        path = os.path.join(keydir, "AuthKey_%s.p8" % key.key_id)
        fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(fd, "wb") as f:
            f.write(key.pem)
        env = {k: v for k, v in os.environ.items() if not k.startswith("ASC_KEY")}
        env["API_PRIVATE_KEYS_DIR"] = keydir

        def run(modern):
            return subprocess.run(altool_upload_cmd(pkg, app_id, args, key_id, issuer, modern),
                                  env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                  text=True)

        log("uploading %s (%s build %s)" % (os.path.basename(pkg), args.version, args.build))
        proc = run(modern)
        if proc.returncode != 0 and ALTOOL_BAD_OPTION.search(proc.stdout + proc.stderr):
            log("altool did not take the %s options; trying the %s ones" % (
                "--api-key" if modern else "--apiKey", "--apiKey" if modern else "--api-key"))
            proc = run(not modern)
    finally:
        shutil.rmtree(keydir, ignore_errors=True)
    for name, text in (("stdout", proc.stdout), ("stderr", proc.stderr)):
        if text.strip():
            log("altool %s:\n%s" % (name, text.strip()))
    errors = altool_errors(proc.returncode, proc.stdout, proc.stderr)
    if errors:
        raise Fail("the upload failed: %s" % "; ".join(errors))
    log("uploaded; App Store Connect processes it next (wait-build)")
    return 0


def cmd_wait_build(api, args):
    app_id = resolve_app_id(api, args)
    if api.offline:
        log("DRY RUN  would poll GET /v1/builds for %s build %s until VALID" % (
            args.version, args.build))
        return 0
    last = None

    def ready():
        nonlocal last
        builds = find_builds(api, app_id, args.version, args.build)
        if builds:
            state = attrs(builds[0]).get("processingState")
            if state in ("FAILED", "INVALID"):
                errors = upload_errors(api, app_id, args.version, args.build) or []
                raise Fail("build %s is %s%s" % (args.build, state,
                                                 (": " + "; ".join(errors)) if errors else ""))
            if state == "VALID":
                return builds[0]
        else:
            errors = upload_errors(api, app_id, args.version, args.build)
            if errors:
                raise Fail("App Store Connect rejected the upload: %s" % "; ".join(errors))
            state = "not listed yet"
        if state != last:
            log("build %s: %s" % (args.build, state))
            last = state
        return None

    build = wait_for("build %s to process" % args.build, ready, args.timeout, args.interval)
    log("build %s is VALID (%s)" % (args.build, build["id"]))
    print(build["id"])
    return 0


def cmd_prepare(api, args):
    meta = load_metadata(args.metadata, args.bundle_id)
    app_id = resolve_app_id(api, args)
    builds = find_builds(api, app_id, args.version, args.build)
    if builds:
        build = builds[0]
    elif api.offline:
        build = {"id": "BUILD_ID", "attributes": {"processingState": "VALID"}}
    else:
        raise Fail("App Store Connect has no build %s of %s yet (run wait-build first)" % (
            args.build, args.version))
    if attrs(build).get("processingState") != "VALID":
        raise Fail("build %s is %s, not VALID" % (args.build,
                                                   attrs(build).get("processingState")))
    versions = mac_versions(api, app_id)
    if is_first_version(versions, args.version):
        notes = None
        log("version %s is the app's first, so it has no What's New" % args.version)
    else:
        notes = whats_new(args.changelog, args.version)
    version = ensure_version(api, app_id, versions, args.version, build["id"],
                             args.replace_waiting, meta.app_info.get("copyright"))
    if version is None:
        return 0
    if attached_build_id(api, version["id"]) != build["id"]:
        log("version %s: attaching build %s" % (args.version, args.build))
        api.call("PATCH", "/v1/appStoreVersions/%s/relationships/build" % version["id"],
                 body=ref("builds", build["id"]))
    if attrs(build).get("usesNonExemptEncryption") is None:
        log("build %s: declaring no non-exempt encryption" % args.build)
        api.patch("/v1/builds/%s" % build["id"], {"data": {
            "type": "builds", "id": build["id"],
            "attributes": {"usesNonExemptEncryption": False}}})
    info = pick_app_info(api, app_id)
    sync_categories(api, info, meta.app_info)
    sync_info_localizations(api, info, meta)
    sync_age_rating(api, info, meta.age_rating)
    sync_content_rights(api, app_id, meta.app_info.get("contentRightsDeclaration"))
    ensure_free_price(api, app_id)
    ensure_availability(api, app_id)
    loc_ids = sync_version_localizations(api, version["id"], meta, notes)
    sync_review_detail(api, version["id"], meta.review)
    if not args.skip_screenshots:
        for loc, shots in sorted(meta.screenshots.items()):
            if loc_ids.get(loc):
                sync_screenshots(api, loc_ids[loc], loc, shots)
    log("version %s is ready for review. App Privacy has no API: check it is published in "
        "App Store Connect (App Privacy) before submitting." % args.version)
    return 0


def ensure_item(api, submission_id, version_id):
    for item in api.iter_all("/v1/reviewSubmissions/%s/items" % submission_id,
                             {"include": "appStoreVersion", "limit": 200}):
        if rel_id(item, "appStoreVersion") == version_id and \
                attrs(item).get("state") != "REMOVED":
            return
    api.post("/v1/reviewSubmissionItems", {"data": {
        "type": "reviewSubmissionItems",
        "relationships": {
            "reviewSubmission": ref("reviewSubmissions", submission_id),
            "appStoreVersion": ref("appStoreVersions", version_id),
        }}})


def cmd_submit(api, args):
    app_id = resolve_app_id(api, args)
    version = find_version(api, app_id, args.version)
    if version is None:
        if not api.offline:
            raise Fail("no MAC_OS version %s in App Store Connect (run prepare first)" %
                       args.version)
        version = {"id": "VERSION_ID", "attributes": {"appVersionState": "PREPARE_FOR_SUBMISSION"}}
    state = version_state(version)
    if state not in EDITABLE:
        log("version %s is %s; nothing to submit" % (args.version, state))
        return 0
    if not api.offline and attached_build_id(api, version["id"]) is None:
        raise Fail("version %s has no build attached (run prepare first)" % args.version)

    def settled():
        busy = open_submissions(api, app_id, ["CANCELING", "COMPLETING"])
        return not busy

    if not api.offline:
        wait_for("earlier review submissions to settle", settled, 600, 15)
    reuse = None
    for sub in open_submissions(api, app_id, ["WAITING_FOR_REVIEW", "IN_REVIEW",
                                              "UNRESOLVED_ISSUES", "READY_FOR_REVIEW"]):
        sub_state = attrs(sub).get("state")
        for_us = rel_id(sub, "appStoreVersionForReview") in (None, version["id"])
        if sub_state in ("WAITING_FOR_REVIEW", "IN_REVIEW"):
            if rel_id(sub, "appStoreVersionForReview") == version["id"]:
                log("version %s is already %s" % (args.version, sub_state))
                return 0
            raise Fail("another macOS submission is %s; wait for it or cancel it in App Store "
                       "Connect" % sub_state)
        if for_us and reuse is None:
            reuse = sub
    if reuse:
        sub_id = reuse["id"]
        log("reusing review submission %s (%s)" % (sub_id, attrs(reuse).get("state")))
    else:
        sub_id = api.post("/v1/reviewSubmissions", {"data": {
            "type": "reviewSubmissions", "attributes": {"platform": "MAC_OS"},
            "relationships": {"app": ref("apps", app_id)}}})["id"]
        log("created review submission %s" % sub_id)
    ensure_item(api, sub_id, version["id"])
    api.patch("/v1/reviewSubmissions/%s" % sub_id, {"data": {
        "type": "reviewSubmissions", "id": sub_id, "attributes": {"submitted": True}}})
    log("submitted version %s for App Review" % args.version)
    return 0


class Gh:
    """Sets GitHub Actions secrets with the gh CLI; values go to gh's stdin only."""

    def __init__(self, repo, dry_run):
        self.repo = repo
        self.dry_run = dry_run

    def _args(self):
        return ["--repo", self.repo] if self.repo else []

    def names(self):
        proc = subprocess.run(["gh", "secret", "list", "--json", "name"] + self._args(),
                              stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        if proc.returncode != 0:
            raise Fail("gh secret list failed: %s" % proc.stderr.strip())
        return {s["name"] for s in json.loads(proc.stdout or "[]")}

    def set(self, name, value):
        data = value if isinstance(value, bytes) else value.encode("utf-8")
        if not data:
            raise Fail("refusing to set %s to an empty value" % name)
        if self.dry_run:
            log("DRY RUN  gh secret set %s (%d bytes, not sent)" % (name, len(data)))
            return
        proc = subprocess.run(["gh", "secret", "set", name] + self._args(), input=data,
                              stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        if proc.returncode != 0:
            raise Fail("gh secret set %s failed: %s" % (
                name, proc.stderr.decode("utf-8", "replace").strip()))
        log("set secret %s" % name)


def openssl(*args, env=None, data=None):
    proc = subprocess.run(["openssl"] + list(args), input=data, env=env,
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if proc.returncode != 0:
        raise Fail("openssl %s failed: %s" % (args[0], proc.stderr.decode(
            "utf-8", "replace").strip()[-300:]))
    return proc.stdout


def ensure_bundle_id(api, bundle_id):
    for b in api.iter_all("/v1/bundleIds", {"filter[identifier]": bundle_id, "limit": 200}):
        if attrs(b).get("identifier") == bundle_id:
            log("bundle ID %s is registered (%s)" % (bundle_id, attrs(b).get("platform")))
            return b
    log("registering bundle ID %s for macOS" % bundle_id)
    return api.post("/v1/bundleIds", {"data": {"type": "bundleIds", "attributes": {
        "identifier": bundle_id, "name": APP_NAME, "platform": "MAC_OS"}}})


def cert_common_name(pem_path):
    subject = openssl("x509", "-in", pem_path, "-noout", "-subject").decode("utf-8", "replace")
    m = re.search(r"CN\s*=\s*([^,/\n]+)", subject)
    return m.group(1).strip() if m else None


def x509_name(pem, which):
    out = openssl("x509", "-noout", "-" + which, "-nameopt", "RFC2253", data=pem)
    return out.decode("utf-8", "replace").split("=", 1)[-1].strip()


def issuer_pem(pem_path):
    """Fetches the certificate's Apple intermediate (from its AIA URL) for the .p12 chain."""
    with open(pem_path, "rb") as f:
        leaf = f.read()
    text = openssl("x509", "-noout", "-text", data=leaf).decode("utf-8", "replace")
    m = re.search(r"CA Issuers - URI:(\S+)", text)
    if not m or not re.match(r"^https?://(certs|www)\.apple\.com/", m.group(1)):
        return None
    try:
        with urllib.request.urlopen(m.group(1), timeout=30) as resp:
            der = resp.read(65536)
        pem = openssl("x509", "-inform", "DER", "-outform", "PEM", data=der)
        if x509_name(pem, "subject") != x509_name(leaf, "issuer"):
            return None
        return pem
    except (urllib.error.URLError, OSError, Fail):
        return None


def create_certificate(api, tmp, cert_type, label):
    active = [c for c in api.iter_all("/v1/certificates", {
        "filter[certificateType]": cert_type, "limit": 200})]
    if active:
        log("%d %s certificate(s) exist already; their private keys are not here, so a new one "
            "is made" % (len(active), cert_type))
    key = os.path.join(tmp, label + ".key")
    csr = os.path.join(tmp, label + ".csr")
    if api.dry_run:
        csr_text = "<CSR>"
    else:
        openssl("req", "-new", "-newkey", "rsa:2048", "-nodes", "-keyout", key, "-out", csr,
                "-subj", "/CN=%s %s" % (APP_NAME, label))
        with open(csr, encoding="ascii") as f:
            csr_text = f.read()
    try:
        cert = api.post("/v1/certificates", {"data": {"type": "certificates", "attributes": {
            "certificateType": cert_type, "csrContent": csr_text}}})
    except ApiError as e:
        if e.status == 409:
            raise Fail("%s\nApple limits how many %s certificates a team has. Revoke one you no "
                       "longer use at https://developer.apple.com/account/resources/"
                       "certificates/list and run this again." % (e, cert_type))
        raise
    log("created %s certificate %s, expires %s" % (
        cert_type, attrs(cert).get("name") or cert.get("id"), attrs(cert).get("expirationDate")))
    return cert, key


def make_p12(tmp, label, cert, key, password):
    content = attrs(cert).get("certificateContent")
    if not content:
        raise Fail("App Store Connect returned no certificate content")
    pem = os.path.join(tmp, label + ".pem")
    der = base64.b64decode(content)
    with open(pem, "wb") as f:
        f.write(openssl("x509", "-inform", "DER", "-outform", "PEM", data=der))
    name = cert_common_name(pem) or label
    args = ["pkcs12", "-export", "-inkey", key, "-in", pem, "-name", name,
            "-passout", "env:SC_P12_PASSWORD", "-keypbe", "PBE-SHA1-3DES",
            "-certpbe", "PBE-SHA1-3DES", "-macalg", "sha1",
            "-out", os.path.join(tmp, label + ".p12")]
    chain = issuer_pem(pem)
    if chain:
        chain_path = os.path.join(tmp, label + "-issuer.pem")
        with open(chain_path, "wb") as f:
            f.write(chain)
        args += ["-certfile", chain_path]
    env = dict(os.environ, SC_P12_PASSWORD=password)
    openssl(*args, env=env)
    p12 = os.path.join(tmp, label + ".p12")
    openssl("pkcs12", "-in", p12, "-passin", "env:SC_P12_PASSWORD", "-noout", env=env)
    with open(p12, "rb") as f:
        data = f.read()
    log("packed %s into a .p12%s" % (name, " with its issuer" if chain else ""))
    return data


def ensure_profile(api, bundle, cert_id):
    for p in api.iter_all("/v1/bundleIds/%s/profiles" % bundle["id"], {"limit": 200}):
        a = attrs(p)
        if a.get("profileType") != "MAC_APP_STORE" or a.get("profileState") != "ACTIVE":
            continue
        certs = {c["id"] for c in api.iter_all("/v1/profiles/%s/certificates" % p["id"],
                                               {"limit": 50})}
        if cert_id in certs and a.get("profileContent"):
            log("reusing provisioning profile %s" % a.get("name"))
            return p
    name = "%s Mac App Store %s" % (APP_NAME, time.strftime("%Y-%m-%d %H%M%S"))
    log("creating provisioning profile %s" % name)
    return api.post("/v1/profiles", {"data": {
        "type": "profiles",
        "attributes": {"name": name, "profileType": "MAC_APP_STORE"},
        "relationships": {
            "bundleId": ref("bundleIds", bundle["id"]),
            "certificates": {"data": [{"type": "certificates", "id": cert_id}]},
        }}})


def make_signing(api, gh, bundle):
    """Makes every signing secret before it stores any, and stores the password last."""
    tmp = tempfile.mkdtemp(prefix="asc-setup-")
    created = []
    stored = []
    try:
        password = secrets.token_urlsafe(24)
        dist, dist_key = create_certificate(api, tmp, "DISTRIBUTION", "app")
        created.append(dist["id"])
        inst, inst_key = create_certificate(api, tmp, "MAC_INSTALLER_DISTRIBUTION", "installer")
        created.append(inst["id"])
        profile = ensure_profile(api, bundle, dist["id"])
        content = attrs(profile).get("profileContent")
        if api.dry_run:
            values = dict.fromkeys(MAS_SECRETS, "<dry run>")
        else:
            if not content:
                raise Fail("App Store Connect returned no profile content")
            base64.b64decode(content, validate=True)
            values = {
                "MAS_APP_CERT_P12": base64.b64encode(
                    make_p12(tmp, "app", dist, dist_key, password)),
                "MAS_INSTALLER_CERT_P12": base64.b64encode(
                    make_p12(tmp, "installer", inst, inst_key, password)),
                "MAS_PROVISION_PROFILE": content,
                "MAS_CERT_PASSWORD": password,
            }
        for name in MAS_SECRETS:
            gh.set(name, values[name])
            stored.append(name)
        created = []
    except BaseException as e:
        if stored:
            advice = ("%s %s replaced, but not %s, so the signing secrets no longer match each "
                      "other. Run scripts/appstore-setup.sh --renew-certs again with the same "
                      "key files." % (
                          ", ".join(stored), "was" if len(stored) == 1 else "were",
                          ", ".join(n for n in MAS_SECRETS if n not in stored)))
            if isinstance(e, Fail):
                raise Fail("%s\n%s" % (e, advice)) from e
            log(advice)
        raise
    finally:
        shutil.rmtree(tmp, ignore_errors=True)
        for cert_id in created if not api.dry_run else []:
            try:
                api.delete("/v1/certificates/%s" % cert_id)
                log("revoked certificate %s from this failed run (its private key is gone)" %
                    cert_id)
            except Fail as e:
                log("could not revoke certificate %s from this failed run: %s" % (cert_id, e))


def check_ci_key(key, bundle_id):
    try:
        Api(key).call("GET", "/v1/apps", {"filter[bundleId]": bundle_id, "limit": 1})
    except ApiError as e:
        raise Fail("the CI key %s does not work, so no secret was changed (%s). Check its Key "
                   "ID and the Issuer ID, and that the key is not revoked." % (key.key_id, e))


def cmd_setup(api, args):
    ci = None
    if args.ci_same_key:
        ci = api.key
    elif args.ci_key_file:
        ci = load_key(args.ci_key_id, args.ci_issuer_id or (api.key and api.key.issuer),
                      args.ci_key_file, env=False)
        if not api.offline:
            check_ci_key(ci, args.bundle_id)
    elif not api.dry_run:
        raise Fail("pass --ci-key-file (the App Manager key for CI) or --ci-same-key")
    gh = Gh(args.repo, api.dry_run)
    existing = set() if api.offline else gh.names()
    bundle = ensure_bundle_id(api, args.bundle_id)
    app = find_app(api, args.bundle_id)
    if args.renew_certs or not all(n in existing for n in MAS_SECRETS):
        make_signing(api, gh, bundle)
    else:
        log("the signing secrets are set already; pass --renew-certs to make new ones")
    if ci is not None:
        gh.set("ASC_KEY_ID", ci.key_id)
        gh.set("ASC_ISSUER_ID", ci.issuer)
        gh.set("ASC_KEY_P8", ci.pem)
    if app:
        gh.set("ASC_APP_ID", app["id"])
        log("app record %s found" % attrs(app).get("name"))
        return 0
    log("App Store Connect has no app with bundle ID %s yet, so ASC_APP_ID is not set." %
        args.bundle_id)
    return 3


def build_parser():
    common = argparse.ArgumentParser(add_help=False)
    common.add_argument("--dry-run", action="store_true",
                        help="print the requests that would change something; send none")
    common.add_argument("--bundle-id", default=BUNDLE_ID)
    common.add_argument("--app-id", help="the app's numeric Apple ID (default: ASC_APP_ID)")
    common.add_argument("--key-file", help="AuthKey_<id>.p8 (default: ASC_KEY_FILE/ASC_KEY_P8)")
    common.add_argument("--key-id", help="API Key ID (default: ASC_KEY_ID)")
    common.add_argument("--issuer-id", help="API Issuer ID (default: ASC_ISSUER_ID)")
    version = argparse.ArgumentParser(add_help=False)
    version.add_argument("--version", required=True, help="CFBundleShortVersionString, 0.1.0")
    build = argparse.ArgumentParser(add_help=False)
    build.add_argument("--build", required=True, help="CFBundleVersion of the upload")
    notes = argparse.ArgumentParser(add_help=False)
    notes.add_argument("--changelog", default=os.path.join(REPO_ROOT, "CHANGELOG.md"),
                       help="What's New for an update comes from its \"## <version>\" section")
    replace = argparse.ArgumentParser(add_help=False)
    replace.add_argument("--replace-waiting", action="store_true",
                         help="cancel a submission waiting for review to attach this build")

    p = argparse.ArgumentParser(description="App Store Connect automation for simple-capture.")
    sub = p.add_subparsers(dest="cmd", required=True)
    s = sub.add_parser("setup", parents=[common],
                       help="bundle ID, certificates, profile and GitHub secrets (owner, once)")
    s.add_argument("--repo", help="OWNER/NAME for gh (default: this checkout's)")
    s.add_argument("--ci-key-file", help="the App Manager key for CI")
    s.add_argument("--ci-key-id")
    s.add_argument("--ci-issuer-id")
    s.add_argument("--ci-same-key", action="store_true", help="CI uses the setup key too")
    s.add_argument("--renew-certs", action="store_true",
                   help="make new certificates and a new profile even if the secrets exist")
    s = sub.add_parser("preflight", parents=[common, version, notes, replace],
                       help='before a build: print "go" or "skip: <why>", or fail')
    s.add_argument("--skip-if-submitted", action="store_true",
                   help='print "skip: <why>" instead of failing when the version is in review '
                        "or already approved")
    s = sub.add_parser("next-build", parents=[common], help="print the next CFBundleVersion")
    s.add_argument("--at-least", type=int, default=1)
    s = sub.add_parser("upload", parents=[common, version, build], help="upload a .pkg")
    s.add_argument("--pkg", required=True)
    s = sub.add_parser("wait-build", parents=[common, version, build],
                       help="wait until the build is processed")
    s.add_argument("--timeout", type=int, default=3600)
    s.add_argument("--interval", type=int, default=30)
    s = sub.add_parser("prepare", parents=[common, version, build, notes, replace],
                       help="version, build, metadata, screenshots, price, availability")
    s.add_argument("--metadata", default=os.path.join(REPO_ROOT, "appstore"))
    s.add_argument("--skip-screenshots", action="store_true")
    sub.add_parser("submit", parents=[common, version], help="submit the version for review")
    s = sub.add_parser("check-metadata", help="validate appstore/ offline")
    s.add_argument("--metadata", default=os.path.join(REPO_ROOT, "appstore"))
    s.add_argument("--bundle-id", default=BUNDLE_ID)
    return p


def main(argv=None):
    args = build_parser().parse_args(argv)
    for name in ("version", "build"):
        value = getattr(args, name, None)
        if value is not None and not re.match(r"^\d+(\.\d+){0,2}$", value):
            raise Fail("--%s %r should be one to three dot-separated numbers" % (name, value))
    if args.cmd == "check-metadata":
        return cmd_check_metadata(args)
    key = load_key(args.key_id, args.issuer_id, args.key_file, required=not args.dry_run)
    api = Api(key, args.dry_run)
    return {
        "setup": cmd_setup,
        "preflight": cmd_preflight,
        "next-build": cmd_next_build,
        "upload": cmd_upload,
        "wait-build": cmd_wait_build,
        "prepare": cmd_prepare,
        "submit": cmd_submit,
    }[args.cmd](api, args)


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Fail as e:
        log("error: %s" % e)
        sys.exit(1)
    except KeyboardInterrupt:
        sys.exit(130)
