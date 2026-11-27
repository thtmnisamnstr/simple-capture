"""Unit tests for asc.py's offline parts: python3 -I scripts/asc/test_asc.py"""

import base64
import contextlib
import hashlib
import importlib.util
import io
import json
import os
import re
import shutil
import struct
import subprocess
import sys
import tempfile
import unittest
import zlib
from unittest import mock

sys.dont_write_bytecode = True
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import asc  # noqa: E402

ISSUER = "69a6de70-03db-47e3-e053-5b8c7c11a4d1"


def der_int(value):
    body = value.lstrip(b"\0") or b"\0"
    if body[0] & 0x80:
        body = b"\0" + body
    return b"\x02" + bytes([len(body)]) + body


def raw_to_der(raw):
    body = der_int(raw[:32]) + der_int(raw[32:])
    return b"\x30" + bytes([len(body)]) + body


def png(width, height):
    def chunk(tag, data):
        return struct.pack(">I", len(data)) + tag + data + struct.pack(
            ">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
    ihdr = struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IEND", b"")


def b64url_decode(text):
    return base64.urlsafe_b64decode(text + "=" * (-len(text) % 4))


class Tmp(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.mkdtemp(prefix="asc-test-")
        self.addCleanup(shutil.rmtree, self.tmp, True)

    def write(self, rel, data):
        path = os.path.join(self.tmp, rel)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "wb" if isinstance(data, bytes) else "w") as f:
            f.write(data)
        return path


class DerToRaw(unittest.TestCase):
    def test_pads_and_strips(self):
        r = b"\x80" + b"\x11" * 31
        s = b"\x22" * 30
        raw = asc.der_to_raw(raw_to_der(r + b"\0\0" + s))
        self.assertEqual(raw, r + b"\0\0" + s)
        self.assertEqual(len(raw), 64)

    def test_leading_zero_byte_for_high_bit(self):
        der = raw_to_der(b"\xff" * 64)
        self.assertEqual(der[3:5], b"\x21\x00")
        self.assertEqual(asc.der_to_raw(der), b"\xff" * 64)

    def test_rejects_malformed(self):
        good = raw_to_der(b"\x01" * 64)
        for bad in (b"", b"\x31" + good[1:], good + b"\0", good[:-1],
                    b"\x30\x06\x02\x01\x81\x02\x01\x01",
                    b"\x30\x26\x02\x21\x01" + b"\x01" * 32 + b"\x02\x01\x01"):
            with self.assertRaises(ValueError, msg=bad.hex()):
                asc.der_to_raw(bad)


class Jwt(Tmp):
    def setUp(self):
        super().setUp()
        self.key = os.path.join(self.tmp, "AuthKey_ABCDE12345.p8")
        self.pub = os.path.join(self.tmp, "pub.pem")
        subprocess.run(["openssl", "genpkey", "-algorithm", "EC", "-pkeyopt",
                        "ec_paramgen_curve:P-256", "-out", self.key], check=True,
                       stderr=subprocess.DEVNULL)
        subprocess.run(["openssl", "pkey", "-in", self.key, "-pubout", "-out", self.pub],
                       check=True)
        with open(self.key, "rb") as f:
            self.pem = f.read()

    def verify(self, data, raw):
        sig = os.path.join(self.tmp, "sig.der")
        with open(sig, "wb") as f:
            f.write(raw_to_der(raw))
        proc = subprocess.run(["openssl", "dgst", "-sha256", "-verify", self.pub,
                               "-signature", sig], input=data, stdout=subprocess.PIPE,
                              stderr=subprocess.PIPE)
        return proc.returncode == 0

    def test_signature_round_trip(self):
        raw = asc.der_to_raw(asc.openssl_sign(self.pem, b"payload"))
        self.assertEqual(len(raw), 64)
        self.assertTrue(self.verify(b"payload", raw))
        self.assertFalse(self.verify(b"other", raw))

    def test_token(self):
        token = asc.make_jwt("ABCDE12345", ISSUER, self.pem, now=1_800_000_000, ttl=3600)
        head, body, sig = token.split(".")
        self.assertEqual(json.loads(b64url_decode(head)),
                         {"alg": "ES256", "kid": "ABCDE12345", "typ": "JWT"})
        self.assertEqual(json.loads(b64url_decode(body)), {
            "iss": ISSUER, "iat": 1_799_999_985, "exp": 1_800_001_125,
            "aud": "appstoreconnect-v1"})
        self.assertNotIn("=", token)
        self.assertTrue(self.verify((head + "." + body).encode(), b64url_decode(sig)))

    def test_bad_key_fails_without_echoing_it(self):
        with self.assertRaises(asc.Fail) as ctx:
            asc.openssl_sign(b"-----BEGIN PRIVATE KEY-----\nnot a key\n", b"x")
        self.assertNotIn("not a key", str(ctx.exception))

    def test_load_key_from_file_takes_id_from_name(self):
        with mock.patch.dict(os.environ, {}, clear=True):
            key = asc.load_key(issuer=ISSUER, key_file=self.key)
        self.assertEqual(key.key_id, "ABCDE12345")
        self.assertEqual(key.pem, self.pem)
        self.assertNotIn("PRIVATE", repr(key))

    def test_load_key_from_env(self):
        env = {"ASC_KEY_ID": "ABCDE12345", "ASC_ISSUER_ID": ISSUER,
               "ASC_KEY_P8": self.pem.decode()}
        with mock.patch.dict(os.environ, env, clear=True):
            self.assertEqual(asc.load_key().key_id, "ABCDE12345")
            self.assertIsNone(asc.load_key(env=False, required=False))

    def test_load_key_rejects_bad_ids(self):
        with mock.patch.dict(os.environ, {}, clear=True):
            with self.assertRaises(asc.Fail):
                asc.load_key("lower-case", ISSUER, self.key)
            with self.assertRaises(asc.Fail):
                asc.load_key("ABCDE12345", "not-a-uuid", self.key)
            with self.assertRaises(asc.Fail):
                asc.load_key()


class P12(Tmp):
    def test_packs_key_and_certificate(self):
        key = os.path.join(self.tmp, "app.key")
        pem = os.path.join(self.tmp, "self.pem")
        subprocess.run(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1",
                        "-keyout", key, "-out", pem, "-subj",
                        "/CN=Apple Distribution: Test Person (TEAM123456)"],
                       check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        der = subprocess.run(["openssl", "x509", "-in", pem, "-outform", "DER"], check=True,
                             stdout=subprocess.PIPE).stdout
        cert = {"attributes": {"certificateContent": base64.b64encode(der).decode()}}
        with contextlib.redirect_stderr(io.StringIO()) as err:
            data = asc.make_p12(self.tmp, "app", cert, key, "pw-123")
        self.assertIn("Apple Distribution: Test Person (TEAM123456)", err.getvalue())
        info = subprocess.run(["openssl", "pkcs12", "-passin", "env:PW", "-nodes"], input=data,
                              env=dict(os.environ, PW="pw-123"), stdout=subprocess.PIPE,
                              stderr=subprocess.PIPE)
        self.assertEqual(info.returncode, 0, info.stderr)
        self.assertIn(b"friendlyName: Apple Distribution: Test Person (TEAM123456)", info.stdout)
        self.assertIn(b"PRIVATE KEY", info.stdout)


class FakeResponse:
    def __init__(self, doc):
        self.body = json.dumps(doc).encode() if doc is not None else b""

    def read(self):
        return self.body

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        return False


def http_error(code, doc):
    return asc.urllib.error.HTTPError(asc.API + "/x", code, "error", {},
                                      io.BytesIO(json.dumps(doc).encode()))


class Http(unittest.TestCase):
    def setUp(self):
        self.requests = []
        patches = [mock.patch.object(asc, "make_jwt", return_value="tok"),
                   mock.patch.object(asc.time, "sleep")]
        for p in patches:
            p.start()
            self.addCleanup(p.stop)
        self.api = asc.Api(asc.Key("ABCDE12345", ISSUER, b"unused"))

    def serve(self, *responses):
        queue = list(responses)

        def urlopen(req, timeout=None):
            self.requests.append(req)
            item = queue.pop(0)
            if isinstance(item, Exception):
                raise item
            return FakeResponse(item)

        p = mock.patch.object(asc.urllib.request, "urlopen", side_effect=urlopen)
        p.start()
        self.addCleanup(p.stop)

    def test_pages_and_retries_rate_limits(self):
        self.serve(http_error(429, {"errors": []}),
                   {"data": [{"id": "1"}], "links": {"next": asc.API + "/v1/builds?cursor=2"}},
                   {"data": [{"id": "2"}], "links": {}})
        self.assertEqual([b["id"] for b in self.api.get_all("/v1/builds", {"limit": 1})],
                         ["1", "2"])
        self.assertEqual(len(self.requests), 3)
        self.assertEqual(self.requests[0].get_header("Authorization"), "Bearer tok")
        self.assertTrue(self.requests[2].full_url.endswith("cursor=2"))

    def test_missing_and_errors(self):
        self.serve(http_error(404, {"errors": []}),
                   http_error(409, {"errors": [{"code": "ENTITY_ERROR", "title": "Bad",
                                                "detail": "limit reached"}]}))
        self.assertIsNone(self.api.get("/v1/apps/1/appAvailabilityV2"))
        with self.assertRaises(asc.ApiError) as ctx:
            self.api.post("/v1/certificates", {"data": {}})
        self.assertEqual(ctx.exception.status, 409)
        self.assertIn("ENTITY_ERROR Bad: limit reached", str(ctx.exception))
        self.assertEqual(self.requests[1].get_method(), "POST")

    def test_does_not_follow_links_off_the_api(self):
        self.serve({"data": [{"id": "1"}], "links": {"next": "https://example.com/steal"}})
        with self.assertRaises(asc.Fail):
            self.api.get_all("/v1/builds")
        self.assertEqual(len(self.requests), 1)

    def test_post_is_not_retried_on_server_errors(self):
        self.serve(http_error(500, {"errors": []}), {"data": {"id": "x"}})
        with self.assertRaises(asc.ApiError):
            self.api.post("/v1/reviewSubmissions", {"data": {}})
        self.assertEqual(len(self.requests), 1)


class Helpers(unittest.TestCase):
    def test_redact(self):
        body = {"data": {"attributes": {"notes": "n", "contactEmail": "a@b.c",
                                        "demoAccountPassword": "pw", "contactPhone": ""}}}
        out = asc.redact(body)["data"]["attributes"]
        self.assertEqual(out, {"notes": "n", "contactEmail": "<redacted>",
                               "demoAccountPassword": "<redacted>", "contactPhone": ""})

    def test_url_and_host_guard(self):
        api = asc.Api()
        self.assertEqual(api.url("/v1/builds", {"filter[app]": "1", "filter[state]": "A,B"}),
                         asc.API + "/v1/builds?filter[app]=1&filter[state]=A,B")
        with self.assertRaises(asc.Fail):
            api.call("GET", "https://example.com/v1/apps")

    def test_altool_errors(self):
        ok = '{"success-message":"No errors uploading","os-version":"26.1"}'
        self.assertEqual(asc.altool_errors(0, ok, ""), [])
        self.assertEqual(asc.altool_errors(0, "Running altool\n" + ok + "\n", "log {not json}"),
                         [])
        self.assertEqual(asc.altool_errors(
            0, 'note\n{"product-errors":[{"message":"ITMS-90296: not sandboxed"}]}', ""),
            ["ITMS-90296: not sandboxed"])
        self.assertEqual(asc.altool_errors(0, "", ""), ["altool printed no JSON result"])
        self.assertEqual(asc.altool_errors(0, "", "No errors uploading 'x.pkg'"), [])
        self.assertEqual(asc.altool_errors(0, ok, "ERROR ITMS-90000: broken"),
                         ["ERROR ITMS-90000: broken"])
        self.assertEqual(asc.altool_errors(0, "*** Error: Unable to upload", ""),
                         ["*** Error: Unable to upload"])
        self.assertEqual(asc.altool_errors(1, ok, ""), ["altool exit 1"])

    def test_small_helpers(self):
        self.assertEqual(asc.lead_int("12.3"), 12)
        self.assertIsNone(asc.lead_int("x"))
        self.assertEqual(asc.changed({"a": 1, "b": 2}, {"a": 1, "b": 3, "c": None}), {"b": 3})
        self.assertEqual(asc.rel_id({"relationships": {"x": {"data": {"id": "7"}}}}, "x"), "7")
        self.assertIsNone(asc.rel_id({"relationships": {"x": {"links": {}}}}, "x"))


class Listing(Tmp):
    def listing(self):
        files = {
            "name": "simple-capture", "subtitle": "Screen Recorder and Editor",
            "description": "Records.\n\nEdits.", "keywords": "screen,record",
            "promotional_text": "Promo",
            "support_url": "https://example.com/support",
            "marketing_url": "https://example.com", "privacy_url": "https://example.com/p",
        }
        for stem, text in files.items():
            self.write("metadata/en-US/%s.txt" % stem, text + "\n")
        self.write("app_info.json", json.dumps({
            "platform": "MAC_OS", "bundleId": asc.BUNDLE_ID, "primaryLocale": "en-US",
            "primaryCategory": "PHOTO_AND_VIDEO", "secondaryCategory": "PRODUCTIVITY",
            "copyright": "2026 Someone",
            "contentRightsDeclaration": "DOES_NOT_USE_THIRD_PARTY_CONTENT"}))
        self.write("age_rating.json", json.dumps({
            "gambling": False, "violenceRealistic": "NONE", "kidsAgeBand": None}))
        self.write("review.json", json.dumps({
            "_comment": "ignored", "notes": "No account needed.",
            "demo_account_required": False}))
        self.write("screenshots/en-US/01.png", png(2880, 1800))
        self.write("screenshots/en-US/02.png", png(1280, 800))
        self.write("screenshots/en-US/.DS_Store", b"x")
        self.write("screenshots/en-US/README.md", "how these were made")
        self.write("screenshots/tools/make.sh", "#!/bin/sh\n")


class MetadataTests(Listing):
    def test_loads_and_maps(self):
        self.listing()
        meta = asc.load_metadata(self.tmp)
        self.assertEqual(meta.locales["en-US"]["info"], {
            "name": "simple-capture", "subtitle": "Screen Recorder and Editor",
            "privacyPolicyUrl": "https://example.com/p"})
        self.assertEqual(meta.locales["en-US"]["version"]["description"], "Records.\n\nEdits.")
        self.assertNotIn("whatsNew", meta.locales["en-US"]["version"])
        self.assertEqual(meta.app_info, {
            "primaryCategory": "PHOTO_AND_VIDEO", "secondaryCategory": "PRODUCTIVITY",
            "copyright": "2026 Someone",
            "contentRightsDeclaration": "DOES_NOT_USE_THIRD_PARTY_CONTENT"})
        self.assertEqual(meta.age_rating, {"gambling": False, "violenceRealistic": "NONE",
                                           "kidsAgeBand": None})
        self.assertEqual(meta.review, {"notes": "No account needed.",
                                       "demoAccountRequired": False})
        shots = meta.screenshots["en-US"]
        self.assertEqual([s.name for s in shots], ["01.png", "02.png"])
        self.assertEqual(shots[0].size, (2880, 1800))
        self.assertEqual(len(shots[0].md5), 32)

    def test_reports_every_problem(self):
        self.listing()
        self.write("metadata/en-US/name.txt", "x" * 31)
        self.write("metadata/en-US/support_url.txt", "not a url")
        self.write("metadata/en-US/titel.txt", "typo")
        self.write("review.json", json.dumps({"contactEmail": "someone@example.com"}))
        self.write("app_info.json", json.dumps({"bundleId": "com.other", "category": "X"}))
        self.write("screenshots/en-US/03.png", png(1920, 1080))
        self.write("screenshots/en-US/04.jpg", b"jpeg")
        self.write("screenshots/fr-FR/01.png", png(1280, 800))
        with self.assertRaises(asc.Fail) as ctx:
            asc.load_metadata(self.tmp)
        text = str(ctx.exception)
        for needle in ("31 characters", "not a single http(s) URL", "titel.txt",
                       "personal data", "bundleId is not", "unknown key 'category'",
                       "1920x1080", "must be .png", "no metadata/fr-FR"):
            self.assertIn(needle, text)
        self.assertNotIn("someone@example.com", text)

    def test_release_notes_file_is_retired(self):
        self.listing()
        self.write("metadata/en-US/release_notes.txt", "First release.")
        with self.assertRaises(asc.Fail) as ctx:
            asc.load_metadata(self.tmp)
        self.assertIn("release_notes.txt: not used; What's New comes from the version's "
                      "section of CHANGELOG.md", str(ctx.exception))

    def test_too_many_screenshots(self):
        self.listing()
        for i in range(3, 12):
            self.write("screenshots/en-US/%02d.png" % i, png(1440, 900))
        with self.assertRaises(asc.Fail) as ctx:
            asc.load_metadata(self.tmp)
        self.assertIn("at most 10", str(ctx.exception))

    def test_repository_listing(self):
        root = os.path.join(asc.REPO_ROOT, "appstore")
        if not os.path.isdir(root):
            self.skipTest("no appstore/ folder yet")
        meta = asc.load_metadata(root)
        self.assertIn("en-US", meta.locales)


class DryRun(Listing):
    def run_main(self, argv, env=None):
        err = io.StringIO()
        out = io.StringIO()
        clean = {k: v for k, v in os.environ.items() if not k.startswith("ASC_")}
        clean.update(env or {})
        with mock.patch.dict(os.environ, clean, clear=True), \
                mock.patch.object(asc.subprocess, "run",
                                  side_effect=AssertionError("no subprocess")), \
                mock.patch.object(asc.urllib.request, "urlopen",
                                  side_effect=AssertionError("no network")), \
                contextlib.redirect_stderr(err), contextlib.redirect_stdout(out):
            code = asc.main(argv)
        return code, out.getvalue(), err.getvalue()

    def test_prepare_plans_a_fresh_account(self):
        self.listing()
        code, _, err = self.run_main(
            ["prepare", "--dry-run", "--version", "0.1.0", "--build", "7",
             "--metadata", self.tmp],
            {"ASC_REVIEW_CONTACT_EMAIL": "someone@example.com"})
        self.assertEqual(code, 0)
        for needle in ("POST /v1/appStoreVersions",
                       '"releaseType": "AFTER_APPROVAL"',
                       "PATCH /v1/appStoreVersions/dry-run-1/relationships/build",
                       "PATCH /v1/appInfos/APP_INFO_ID",
                       "POST /v1/appInfoLocalizations",
                       "PATCH /v1/ageRatingDeclarations/AGE_RATING_ID",
                       "POST /v1/appPriceSchedules", "POST /v2/appAvailabilities",
                       "POST /v1/appStoreVersionLocalizations",
                       "POST /v1/appStoreReviewDetails", "POST /v1/appScreenshotSets",
                       "POST /v1/appScreenshots", "sourceFileChecksum",
                       "the app's first, so it has no What's New"):
            self.assertIn(needle, err)
        self.assertNotIn("whatsNew", err)
        self.assertNotIn("someone@example.com", err)
        self.assertIn('"contactEmail": "<redacted>"', err)

    def test_preflight_plans_a_fresh_account(self):
        code, out, _ = self.run_main(["preflight", "--dry-run", "--version", "0.1.0"])
        self.assertEqual((code, out.strip()), (0, "go"))

    def test_submit_and_next_build(self):
        code, _, err = self.run_main(["submit", "--dry-run", "--version", "0.1.0"])
        self.assertEqual(code, 0)
        self.assertIn("POST /v1/reviewSubmissions", err)
        self.assertIn("POST /v1/reviewSubmissionItems", err)
        self.assertIn('"submitted": true', err)
        code, out, _ = self.run_main(["next-build", "--dry-run", "--at-least", "41"])
        self.assertEqual((code, out.strip()), (0, "41"))

    def test_setup_plans_without_credentials(self):
        code, _, err = self.run_main(["setup", "--dry-run", "--ci-same-key"])
        self.assertEqual(code, 3)
        for needle in ("POST /v1/bundleIds", '"certificateType": "DISTRIBUTION"',
                       '"certificateType": "MAC_INSTALLER_DISTRIBUTION"',
                       '"csrContent": "<redacted>"', '"profileType": "MAC_APP_STORE"',
                       "gh secret set MAS_APP_CERT_P12"):
            self.assertIn(needle, err)

    def test_rejects_bad_version(self):
        with self.assertRaises(asc.Fail):
            self.run_main(["submit", "--dry-run", "--version", "v0.1.0"])


class Changelog(Tmp):
    def test_section_lookup(self):
        text = ("# Changelog\n\n## 0.10.0\n\nTen.\n\n## 0.1.0 (unreleased)\n\nOne.\n\n"
                "### Fixed\n- x\n\n## 0.0.9\n\nOld.\n")
        self.assertEqual(asc.changelog_section(text, "0.1.0"), "One.\n\n### Fixed\n- x")
        self.assertEqual(asc.changelog_section(text, "0.10.0"), "Ten.")
        self.assertIsNone(asc.changelog_section(text, "0.1"))
        self.assertIsNone(asc.changelog_section(text, "0.2.0"))
        self.assertEqual(asc.changelog_section("## [1.2.0] - 2027-01-01\nA\n", "1.2.0"), "A")
        self.assertEqual(asc.changelog_section("## v1.2.0\nA\n# Older\nB\n", "1.2.0"), "A")

    def test_markdown_to_text(self):
        md = ("Intro with `code` and a [link](https://example.com) and **bold**\n"
              "wrapped here.\n\n"
              "### Fixed ###\n"
              "- One *thing*,\n  wrapped.\n"
              "  - Nested <https://example.com/x>.\n"
              "* Two \\*literal\\*\n\n"
              "1. First\n2. Second\n\n"
              "<!-- hidden -->\n---\n"
              "[ref]: https://example.com\n"
              "Last ![img](a.png) line.\n")
        self.assertEqual(asc.markdown_to_text(md),
                         "Intro with code and a link and bold wrapped here.\n\n"
                         "Fixed\n"
                         "• One thing, wrapped.\n"
                         "  • Nested https://example.com/x.\n"
                         "• Two *literal*\n\n"
                         "1. First\n2. Second\n\n"
                         "Last img line.")

    def test_long_sections_are_cut_after_a_line(self):
        path = self.write("CHANGELOG.md", "## 1.0.0\n\n" + "".join(
            "- Item %03d %s\n" % (i, "x" * 60) for i in range(100)))
        with contextlib.redirect_stderr(io.StringIO()):
            text = asc.whats_new(path, "1.0.0", limit=500)
        lines = text.splitlines()
        self.assertLessEqual(len(text), 500)
        self.assertEqual(lines[-1], "…")
        self.assertEqual(lines[-2], "• Item %03d %s" % (len(lines) - 2, "x" * 60))

    def test_missing_section_or_file(self):
        path = self.write("CHANGELOG.md", "## 0.1.0\n\nFirst.\n")
        with self.assertRaises(asc.Fail) as ctx:
            asc.whats_new(path, "0.2.0")
        self.assertIn('has no "## 0.2.0" section', str(ctx.exception))
        with self.assertRaises(asc.Fail):
            asc.whats_new(os.path.join(self.tmp, "none.md"), "0.2.0")

    def test_repository_changelog(self):
        path = os.path.join(asc.REPO_ROOT, "CHANGELOG.md")
        with open(path, encoding="utf-8") as f:
            section = asc.changelog_section(f.read(), "0.1.0")
        self.assertTrue(section)
        text = asc.markdown_to_text(section)
        lines = text.splitlines()
        with contextlib.redirect_stderr(io.StringIO()):
            self.assertLessEqual(len(asc.whats_new(path, "0.1.0")), asc.WHATS_NEW_LIMIT)
        self.assertTrue(text.startswith(" ".join(section.split("\n", 1)[0].split()[:4])))
        for title in re.findall(r"(?m)^#{3,6}\s+(.+?)\s*$", section):
            self.assertIn(title, lines)
        for code in re.findall(r"`([^`\n]+)`", section):
            self.assertIn(code, text)
        for label in re.findall(r"\[([^\]\n]+)\]\(", section):
            self.assertIn(label, text)
        for mark in ("`", "**", "](", "\n- ", "\n#"):
            self.assertNotIn(mark, text)
        self.assertEqual(len([line for line in lines if line.startswith("• ")]),
                         len(re.findall(r"(?m)^- ", section)))

    def test_versions(self):
        self.assertEqual(asc.changelog_versions(
            "# Changelog\n## 0.10.0\n## [0.2.0] - 2027-01-01\n## v0.1.0 (unreleased)\n"
            "## Unreleased\n### 9.9.9\n"), ["0.10.0", "0.2.0", "0.1.0"])


EDITIONS_MD = """Intro for macOS.

<!-- Linux: a hidden note -->
### Record
- Windows from another Space now record correctly.
- Highlights on windows you record.
- On Windows the app starts faster.
- Windows: one NSIS setup.
  A wrapped line.
- Faster GIFs with Gifski.
  - Nested under it.
- Linux: .deb fixes.
- Exports keep their colours.

### Windows
- Faster startup.

#### Details
- More.

### Known limits
- The Windows installer is not code-signed.

- The Windows and Linux builds are experimental.

### Fixed
A paragraph about macOS, Windows and Linux,
wrapped.

Crash-free recording on macOS.
"""


class EditionFilter(Tmp):
    def test_drops_blocks_about_other_editions(self):
        md, dropped = asc.drop_other_editions(EDITIONS_MD)
        self.assertEqual(dropped, 12)
        self.assertEqual(asc.markdown_to_text(md),
                         "Intro for macOS.\n\n"
                         "Record\n"
                         "• Windows from another Space now record correctly.\n"
                         "• Highlights on windows you record.\n"
                         "• Exports keep their colours.\n\n"
                         "Fixed\n"
                         "Crash-free recording on macOS.")

    def test_keeps_sections_about_the_mac(self):
        md = "### Fixed\n- Exports work.\n\n### Known limits\n- Copy-protected apps send silence.\n"
        self.assertEqual(asc.drop_other_editions(md), (md.strip(), 0))

    def test_whats_new_leaves_them_out_and_says_how_many(self):
        path = self.write("CHANGELOG.md", "## 0.2.0\n\n" + EDITIONS_MD + "\n## 0.1.0\n\nFirst.\n")
        err = io.StringIO()
        with contextlib.redirect_stderr(err):
            text = asc.whats_new(path, "0.2.0")
        self.assertTrue(text.startswith("Intro for macOS.\n\nRecord\n• Windows from another"))
        self.assertIsNone(asc.OTHER_EDITION.search(text))
        self.assertIn("left out 12 blocks of the 0.2.0 section about other editions",
                      err.getvalue())

    def test_a_section_only_about_other_editions_fails(self):
        path = self.write("CHANGELOG.md", "## 0.2.0\n\n### Windows\n- Faster.\n\n- Linux: fixes.\n")
        with self.assertRaises(asc.Fail) as ctx, contextlib.redirect_stderr(io.StringIO()):
            asc.whats_new(path, "0.2.0")
        self.assertIn("only about other editions", str(ctx.exception))


class ListingRules(Tmp):
    def load_check(self):
        spec = importlib.util.spec_from_file_location(
            "appstore_check", os.path.join(asc.REPO_ROOT, "appstore", "check.py"))
        check = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(check)
        return check

    def test_whats_new_rules(self):
        problems = asc.whats_new_problems
        self.assertEqual(problems("Crash-free recording, free-form crops and a warning when free "
                                  "disk space is low. Windows from another Space record."), [])
        for text, word in (("Now free for everyone.", "'free'"), ("Free of charge.", "'Free'"),
                           ("Fixes from the beta.", "'beta'"), ("Prices drop.", "'Prices'"),
                           ("Only $5.", "'$5'"), ("It needs no network access.", "'no network'")):
            self.assertEqual(len(problems(text)), 1, text)
            self.assertIn(word, problems(text)[0])
        self.assertIn("4001 characters", problems("x" * 4001)[0])

    def test_whats_new_is_checked_after_the_oldest_version(self):
        check = self.load_check()
        self.write("CHANGELOG.md", "# Changelog\n\n## 0.2.0\n\n- Now free.\n- Windows: setup.\n\n"
                                   "## 0.1.0\n\n- First, for macOS, Windows and Linux.\n")
        with mock.patch.object(check, "REPO", check.Path(self.tmp)), \
                mock.patch.object(check, "load_asc", return_value=asc), \
                contextlib.redirect_stderr(io.StringIO()):
            found = {}
            for version in ("0.1.0", "0.2.0", "0.3.0"):
                problems, counts = [], {}
                note = check.check_whats_new(version, problems, counts)
                found[version] = (note, problems, counts)
        self.assertIn("0.1.0 is CHANGELOG.md's oldest version", found["0.1.0"][0])
        self.assertEqual(found["0.1.0"][1:], ([], {}))
        self.assertEqual(found["0.2.0"][0], "")
        self.assertEqual(found["0.2.0"][2], {"What's New": len("• Now free.")})
        self.assertEqual(len(found["0.2.0"][1]), 1)
        self.assertIn("contains 'free'", found["0.2.0"][1][0])
        self.assertIn('no "## 0.3.0" section', found["0.3.0"][1][0])


FAKE_PEM ="-----BEGIN PRIVATE KEY-----\nnot used\n-----END PRIVATE KEY-----\n"
API_ENV = {"ASC_KEY_ID": "ABCDE12345", "ASC_ISSUER_ID": ISSUER, "ASC_KEY_P8": FAKE_PEM,
           "ASC_APP_ID": "123"}


def store_version(id_, string, state):
    return {"id": id_, "type": "appStoreVersions",
            "attributes": {"versionString": string, "appVersionState": state,
                           "releaseType": "AFTER_APPROVAL"}}


def submission(id_, state, version_id=None):
    sub = {"id": id_, "type": "reviewSubmissions", "attributes": {"state": state}}
    if version_id:
        sub["relationships"] = {"appStoreVersionForReview": {
            "data": {"type": "appStoreVersions", "id": version_id}}}
    return sub


class FakeAsc:
    def __init__(self, routes=None):
        self.routes = dict(routes or {})
        self.calls = []
        self.created = 0

    def __call__(self, api, method, path, params=None, body=None, missing_ok=False):
        self.calls.append((method, path, dict(params or {}), body))
        route = self.routes.get((method, path))
        if callable(route):
            route = route(params or {}, body, api.key)
        if route is not None:
            return route
        if method == "GET":
            if missing_ok:
                return None
            raise asc.ApiError(method, path, 404, b"{}")
        if method == "POST":
            self.created += 1
            return {"data": dict(body["data"], id="new-%d" % self.created)}
        if method == "PATCH":
            return {"data": (body or {}).get("data")}
        return {}

    def sent(self, method, path=None):
        return [b for m, p, _, b in self.calls if m == method and path in (None, p)]

    def writes(self):
        return [(m, p) for m, p, _, _ in self.calls if m != "GET"]


class Stateful(Listing):
    def setUp(self):
        super().setUp()
        self.logs = []
        for p in (mock.patch.object(asc.time, "sleep"),
                  mock.patch.object(asc, "log", side_effect=self.logs.append),
                  mock.patch.object(asc.subprocess, "run",
                                    side_effect=AssertionError("no subprocess")),
                  mock.patch.object(asc.urllib.request, "urlopen",
                                    side_effect=AssertionError("no network"))):
            p.start()
            self.addCleanup(p.stop)

    def api(self, fake):
        p = mock.patch.object(asc.Api, "call", new=lambda api, *a, **k: fake(api, *a, **k))
        p.start()
        self.addCleanup(p.stop)
        return asc.Api(asc.Key("ABCDE12345", ISSUER, b"unused"))

    def run_main(self, fake, argv):
        env = {k: v for k, v in os.environ.items() if not k.startswith("ASC_")}
        env.update(API_ENV)
        out = io.StringIO()
        self.api(fake)
        with mock.patch.dict(os.environ, env, clear=True), contextlib.redirect_stdout(out):
            code = asc.main(argv)
        return code, out.getvalue()


class Prepare(Stateful):
    def fake(self, versions):
        return FakeAsc({
            ("GET", "/v1/builds"): {"data": [{"id": "B7", "attributes": {
                "processingState": "VALID", "usesNonExemptEncryption": False}}]},
            ("GET", "/v1/apps/123/appStoreVersions"): {"data": versions},
            ("GET", "/v1/apps/123/appInfos"): {"data": [
                {"id": "I1", "attributes": {"state": "PREPARE_FOR_SUBMISSION"}}]},
            ("GET", "/v1/appInfos/I1/ageRatingDeclaration"): {"data": {"id": "AR1",
                                                                       "attributes": {}}},
            ("GET", "/v1/apps/123/appPricePoints"): {"data": [
                {"id": "PP0", "attributes": {"customerPrice": "0.0"}}]},
            ("GET", "/v1/territories"): {"data": [{"id": "USA"}, {"id": "GBR"}]},
        })

    def prepare(self, fake, version, changelog):
        return self.run_main(fake, ["prepare", "--version", version, "--build", "7",
                                    "--metadata", self.tmp, "--skip-screenshots",
                                    "--changelog", changelog])

    def test_renames_the_first_version_without_whats_new(self):
        self.listing()
        fake = self.fake([store_version("V1", "1.0", "PREPARE_FOR_SUBMISSION")])
        fake.routes[("GET", "/v1/appStoreVersions/V1/appStoreVersionLocalizations")] = {
            "data": [{"id": "L1", "attributes": {"locale": "en-US", "description": "old"}}]}
        code, _ = self.prepare(fake, "0.1.0", os.path.join(self.tmp, "none.md"))
        self.assertEqual(code, 0)
        self.assertIn({"data": {"type": "appStoreVersions", "id": "V1",
                                "attributes": {"versionString": "0.1.0"}}},
                      fake.sent("PATCH", "/v1/appStoreVersions/V1"))
        self.assertEqual(fake.sent("POST", "/v1/appStoreVersions"), [])
        self.assertEqual(fake.sent("PATCH", "/v1/appStoreVersions/V1/relationships/build"),
                         [asc.ref("builds", "B7")])
        self.assertEqual(fake.sent("PATCH", "/v1/builds/B7"), [])
        loc = fake.sent("PATCH", "/v1/appStoreVersionLocalizations/L1")[0]["data"]
        self.assertEqual(loc["attributes"]["description"], "Records.\n\nEdits.")
        self.assertNotIn("whatsNew", json.dumps([c[3] for c in fake.calls]))

    def test_an_update_takes_whats_new_from_the_changelog(self):
        self.listing()
        changelog = self.write("CHANGELOG.md", (
            "# Changelog\n\n## 0.2.0\n\n### Fixed\n"
            "- **Exports** to a [chosen folder](https://example.com)\n  work again.\n\n"
            "## 0.1.0\n\nFirst.\n"))
        fake = self.fake([store_version("V0", "0.1.0", "READY_FOR_DISTRIBUTION")])
        fake.routes[("GET", "/v1/appStoreVersions/new-1/appStoreVersionLocalizations")] = {
            "data": [{"id": "L2", "attributes": {"locale": "en-US"}}]}
        code, _ = self.prepare(fake, "0.2.0", changelog)
        self.assertEqual(code, 0)
        created = fake.sent("POST", "/v1/appStoreVersions")
        self.assertEqual(created[0]["data"]["attributes"]["versionString"], "0.2.0")
        loc = fake.sent("PATCH", "/v1/appStoreVersionLocalizations/L2")[0]["data"]
        self.assertEqual(loc["attributes"]["whatsNew"],
                         "Fixed\n• Exports to a chosen folder work again.")

    def test_whats_new_leaves_out_other_editions(self):
        self.listing()
        changelog = self.write("CHANGELOG.md", (
            "## 0.2.0\n\n### Fixed\n- Linux: the .deb installs again.\n"
            "- Windows from another Space record again.\n\n## 0.1.0\n\nFirst.\n"))
        fake = self.fake([store_version("V0", "0.1.0", "READY_FOR_DISTRIBUTION")])
        fake.routes[("GET", "/v1/appStoreVersions/new-1/appStoreVersionLocalizations")] = {
            "data": [{"id": "L2", "attributes": {"locale": "en-US"}}]}
        self.assertEqual(self.prepare(fake, "0.2.0", changelog)[0], 0)
        loc = fake.sent("PATCH", "/v1/appStoreVersionLocalizations/L2")[0]["data"]
        self.assertEqual(loc["attributes"]["whatsNew"],
                         "Fixed\n• Windows from another Space record again.")

    def test_an_update_without_its_section_fails_before_any_change(self):
        self.listing()
        changelog = self.write("CHANGELOG.md", "## 0.1.0\n\nFirst.\n")
        fake = self.fake([store_version("V0", "0.1.0", "READY_FOR_DISTRIBUTION")])
        with self.assertRaises(asc.Fail) as ctx:
            self.prepare(fake, "0.2.0", changelog)
        self.assertIn('no "## 0.2.0" section', str(ctx.exception))
        self.assertEqual(fake.writes(), [])


class Versions(Stateful):
    def test_first_version_detection(self):
        first = asc.is_first_version
        self.assertTrue(first([], "0.1.0"))
        self.assertTrue(first([store_version("V1", "1.0", "PREPARE_FOR_SUBMISSION")], "0.1.0"))
        self.assertTrue(first([store_version("V1", "0.1.0", "WAITING_FOR_REVIEW")], "0.1.0"))
        self.assertTrue(first([store_version("V0", "0.1.0", "REJECTED")], "0.1.1"))
        self.assertFalse(first([store_version("V0", "0.1.0", "READY_FOR_DISTRIBUTION")],
                               "0.2.0"))

    def test_waiting_with_another_build_needs_replace_waiting(self):
        fake = FakeAsc({("GET", "/v1/appStoreVersions/V1/build"): {"data": {"id": "OLD"}}})
        with self.assertRaises(asc.Fail) as ctx:
            asc.ensure_version(self.api(fake), "123",
                               [store_version("V1", "0.1.0", "WAITING_FOR_REVIEW")],
                               "0.1.0", "B7", False, None)
        self.assertIn("--replace-waiting", str(ctx.exception))
        self.assertEqual(fake.writes(), [])

    def test_replace_waiting_cancels_the_submission(self):
        fake = FakeAsc({
            ("GET", "/v1/appStoreVersions/V1/build"): {"data": {"id": "OLD"}},
            ("GET", "/v1/reviewSubmissions"): {"data": [
                submission("S0", "WAITING_FOR_REVIEW", "V0"),
                submission("S1", "WAITING_FOR_REVIEW", "V1")]},
            ("GET", "/v1/appStoreVersions/V1"): {
                "data": store_version("V1", "0.1.0", "DEVELOPER_REJECTED")},
        })
        v = asc.ensure_version(self.api(fake), "123",
                               [store_version("V1", "0.1.0", "WAITING_FOR_REVIEW")],
                               "0.1.0", "B7", True, None)
        self.assertEqual(asc.version_state(v), "DEVELOPER_REJECTED")
        self.assertEqual(fake.writes(), [("PATCH", "/v1/reviewSubmissions/S1")])
        self.assertEqual(fake.sent("PATCH")[0]["data"]["attributes"], {"canceled": True})

    def test_in_review_with_this_build_is_left_alone(self):
        fake = FakeAsc({("GET", "/v1/appStoreVersions/V1/build"): {"data": {"id": "B7"}}})
        self.assertIsNone(asc.ensure_version(
            self.api(fake), "123", [store_version("V1", "0.1.0", "IN_REVIEW")], "0.1.0", "B7",
            False, None))
        self.assertEqual(fake.writes(), [])


class Screenshots(Stateful):
    def shot(self, name, data):
        return asc.Shot(self.write("shots/" + name, data), (2880, 1800))

    def fake(self, *existing):
        return FakeAsc({
            ("GET", "/v1/appStoreVersionLocalizations/L1/appScreenshotSets"): {
                "data": [{"id": "S1"}]},
            ("GET", "/v1/appScreenshotSets/S1/appScreenshots"): {"data": [
                {"id": id_, "attributes": {"sourceFileChecksum": hashlib.md5(data).hexdigest(),
                                           "assetDeliveryState": {"state": "COMPLETE"}}}
                for id_, data in existing]},
            ("GET", "/v1/appScreenshots/new-1"): {"data": {"id": "new-1", "attributes": {
                "assetDeliveryState": {"state": "COMPLETE"}}}},
        })

    def test_reuses_deletes_uploads_and_reorders(self):
        x, y, z = b"x" * 10, b"y" * 10, b"z" * 10
        fake = self.fake(("A", x), ("B", y))
        asc.sync_screenshots(self.api(fake), "L1", "en-US",
                             [self.shot("01.png", y), self.shot("02.png", z)])
        self.assertEqual(fake.writes(), [
            ("DELETE", "/v1/appScreenshots/A"),
            ("POST", "/v1/appScreenshots"),
            ("PATCH", "/v1/appScreenshots/new-1"),
            ("PATCH", "/v1/appScreenshotSets/S1/relationships/appScreenshots")])
        self.assertEqual(fake.sent("POST")[0]["data"]["attributes"],
                         {"fileName": "02.png", "fileSize": 10})
        self.assertEqual(fake.sent("PATCH", "/v1/appScreenshots/new-1")[0]["data"]["attributes"],
                         {"uploaded": True, "sourceFileChecksum": hashlib.md5(z).hexdigest()})
        self.assertEqual(
            fake.sent("PATCH", "/v1/appScreenshotSets/S1/relationships/appScreenshots"),
            [{"data": [{"type": "appScreenshots", "id": "B"},
                       {"type": "appScreenshots", "id": "new-1"}]}])

    def test_unchanged_screenshots_send_nothing_and_reorder_only_patches(self):
        x, y = b"x" * 10, b"y" * 10
        fake = self.fake(("A", x), ("B", y))
        asc.sync_screenshots(self.api(fake), "L1", "en-US",
                             [self.shot("01.png", x), self.shot("02.png", y)])
        self.assertEqual(fake.writes(), [])
        asc.sync_screenshots(self.api(fake), "L1", "en-US",
                             [self.shot("01.png", y), self.shot("02.png", x)])
        self.assertEqual(fake.writes(), [
            ("PATCH", "/v1/appScreenshotSets/S1/relationships/appScreenshots")])


class Submit(Stateful):
    def fake(self, submissions, items=()):
        def by_state(params, body, key):
            states = params.get("filter[state]", "").split(",")
            return {"data": [s for s in submissions if s["attributes"]["state"] in states]}

        return FakeAsc({
            ("GET", "/v1/apps/123/appStoreVersions"): {
                "data": [store_version("V1", "0.1.0", "REJECTED")]},
            ("GET", "/v1/appStoreVersions/V1/build"): {"data": {"id": "B7"}},
            ("GET", "/v1/reviewSubmissions"): by_state,
            ("GET", "/v1/reviewSubmissions/S1/items"): {"data": list(items)},
        })

    def test_resubmits_the_unresolved_submission(self):
        item = {"id": "IT1", "attributes": {"state": "REJECTED"}, "relationships": {
            "appStoreVersion": {"data": {"type": "appStoreVersions", "id": "V1"}}}}
        fake = self.fake([submission("S1", "UNRESOLVED_ISSUES", "V1")], [item])
        self.assertEqual(self.run_main(fake, ["submit", "--version", "0.1.0"])[0], 0)
        self.assertEqual(fake.writes(), [("PATCH", "/v1/reviewSubmissions/S1")])
        self.assertEqual(fake.sent("PATCH")[0]["data"]["attributes"], {"submitted": True})

    def test_fills_a_draft_submission(self):
        fake = self.fake([submission("S1", "READY_FOR_REVIEW")])
        self.assertEqual(self.run_main(fake, ["submit", "--version", "0.1.0"])[0], 0)
        self.assertEqual(fake.writes(), [("POST", "/v1/reviewSubmissionItems"),
                                         ("PATCH", "/v1/reviewSubmissions/S1")])

    def test_refuses_while_another_version_is_in_review(self):
        fake = self.fake([submission("S9", "WAITING_FOR_REVIEW", "V0")])
        with self.assertRaises(asc.Fail):
            self.run_main(fake, ["submit", "--version", "0.1.0"])
        self.assertEqual(fake.writes(), [])


class Preflight(Stateful):
    def preflight(self, versions, version="0.1.0", *flags, changelog=None):
        fake = FakeAsc({("GET", "/v1/apps/123/appStoreVersions"): {"data": versions}})
        try:
            code, out = self.run_main(fake, [
                "preflight", "--version", version,
                "--changelog", changelog or os.path.join(self.tmp, "none.md")] + list(flags))
        finally:
            self.assertEqual(fake.writes(), [])
        self.assertEqual(code, 0)
        return out.strip()

    def test_a_new_app_needs_no_changelog(self):
        self.assertEqual(self.preflight([store_version("V1", "1.0", "PREPARE_FOR_SUBMISSION")]),
                         "go")

    def test_a_version_in_review(self):
        waiting = [store_version("V1", "0.1.0", "WAITING_FOR_REVIEW")]
        with self.assertRaises(asc.Fail) as ctx:
            self.preflight(waiting)
        self.assertIn("--replace-waiting", str(ctx.exception))
        self.assertEqual(self.preflight(waiting, "0.1.0", "--replace-waiting"), "go")
        self.assertTrue(self.preflight(waiting, "0.1.0", "--skip-if-submitted").startswith(
            "skip: version 0.1.0 is WAITING_FOR_REVIEW already"))

    def test_an_approved_version(self):
        done = [store_version("V1", "0.1.0", "READY_FOR_DISTRIBUTION")]
        with self.assertRaises(asc.Fail) as ctx:
            self.preflight(done, "0.1.0", "--replace-waiting")
        self.assertIn("release a new version number", str(ctx.exception))
        self.assertTrue(self.preflight(done, "0.1.0", "--skip-if-submitted").startswith(
            "skip: version 0.1.0 is READY_FOR_DISTRIBUTION already"))

    def test_an_update_needs_its_changelog_section(self):
        released = [store_version("V0", "0.1.0", "READY_FOR_DISTRIBUTION")]
        with self.assertRaises(asc.Fail):
            self.preflight(released, "0.2.0")
        with self.assertRaises(asc.Fail) as ctx:
            self.preflight(released, "0.2.0",
                           changelog=self.write("old.md", "## 0.1.0\n\n- First.\n"))
        self.assertIn('no "## 0.2.0" section', str(ctx.exception))
        self.assertEqual(self.preflight(
            released, "0.2.0", changelog=self.write("new.md", "## 0.2.0\n\n- Fixes.\n")), "go")

    def test_an_update_checks_whats_new_before_the_build(self):
        released = [store_version("V0", "0.1.0", "READY_FOR_DISTRIBUTION")]
        with self.assertRaises(asc.Fail) as ctx:
            self.preflight(released, "0.2.0", changelog=self.write(
                "bad.md", "## 0.2.0\n\n- Exports are free of charge now.\n- Fixes.\n"))
        self.assertIn("contains 'free'", str(ctx.exception))
        self.assertEqual(self.preflight(released, "0.2.0", changelog=self.write(
            "mixed.md", "## 0.2.0\n\n- Windows: the installer is signed.\n- Fixes.\n")), "go")
        self.assertIn("What's New: left out 1 block of the 0.2.0 section about other editions",
                      self.logs)
        self.assertIn("What's New: 8 characters from %s" % os.path.join(self.tmp, "mixed.md"),
                      self.logs)

    def test_no_new_version_while_another_is_in_review(self):
        with self.assertRaises(asc.Fail) as ctx:
            self.preflight([store_version("V0", "0.1.0", "WAITING_FOR_REVIEW")], "0.2.0",
                           "--skip-if-submitted")
        self.assertIn("version 0.1.0 is WAITING_FOR_REVIEW", str(ctx.exception))


class Setup(Stateful):
    def test_a_bad_ci_key_fails_before_any_secret(self):
        admin = self.write("AuthKey_ADMIN12345.p8", FAKE_PEM)
        ci = self.write("AuthKey_CI12345678.p8", FAKE_PEM)

        def apps(params, body, key):
            if key.key_id == "CI12345678":
                raise asc.ApiError("GET", "/v1/apps", 401, json.dumps({"errors": [
                    {"code": "NOT_AUTHORIZED", "title": "Authentication credentials are "
                                                        "missing or invalid."}]}).encode())
            return {"data": []}

        fake = FakeAsc({("GET", "/v1/apps"): apps})
        with mock.patch.object(asc, "Gh", side_effect=AssertionError("no gh")), \
                self.assertRaises(asc.Fail) as ctx:
            self.run_main(fake, ["setup", "--key-file", admin, "--key-id", "ADMIN12345",
                                 "--issuer-id", ISSUER, "--ci-key-file", ci,
                                 "--ci-key-id", "CI12345678", "--repo", "owner/repo"])
        self.assertIn("the CI key CI12345678 does not work, so no secret was changed",
                      str(ctx.exception))
        self.assertIn("NOT_AUTHORIZED", str(ctx.exception))
        self.assertEqual(fake.writes(), [])


class Signing(Stateful):
    def make_signing(self, fake, sets, fail_on=None):
        certs = iter([({"id": "C1"}, "k1"), ({"id": "C2"}, "k2")])

        class Gh:
            def set(self, name, value):
                if name == fail_on:
                    raise asc.Fail("gh secret set %s failed: HTTP 502" % name)
                sets.append(name)

        profile = {"attributes": {"profileContent": base64.b64encode(b"profile").decode()}}
        with mock.patch.object(asc, "create_certificate", side_effect=lambda *a: next(certs)), \
                mock.patch.object(asc, "ensure_profile", return_value=profile), \
                mock.patch.object(asc, "make_p12", return_value=b"p12"):
            asc.make_signing(self.api(fake), Gh(), {"id": "BUNDLE"})

    def test_stores_the_password_last(self):
        fake, sets = FakeAsc(), []
        self.make_signing(fake, sets)
        self.assertEqual(sorted(sets), sorted(asc.MAS_SECRETS))
        self.assertEqual(sets[-1], "MAS_CERT_PASSWORD")
        self.assertEqual(fake.writes(), [])

    def test_a_partial_failure_says_what_to_run(self):
        fake, sets = FakeAsc(), []
        with self.assertRaises(asc.Fail) as ctx:
            self.make_signing(fake, sets, fail_on="MAS_PROVISION_PROFILE")
        text = str(ctx.exception)
        self.assertIn("MAS_APP_CERT_P12, MAS_INSTALLER_CERT_P12 were replaced, but not "
                      "MAS_PROVISION_PROFILE, MAS_CERT_PASSWORD", text)
        self.assertIn("scripts/appstore-setup.sh --renew-certs", text)
        self.assertEqual(fake.writes(), [("DELETE", "/v1/certificates/C1"),
                                         ("DELETE", "/v1/certificates/C2")])

    def test_a_failure_before_any_secret_only_revokes(self):
        fake, sets = FakeAsc(), []
        with self.assertRaises(asc.Fail) as ctx:
            self.make_signing(fake, sets, fail_on="MAS_APP_CERT_P12")
        self.assertNotIn("--renew-certs", str(ctx.exception))
        self.assertEqual(sets, [])
        self.assertEqual(len(fake.writes()), 2)


class Upload(Stateful):
    def upload(self, *results, helptext="--upload-package --api-key --api-issuer"):
        pkg = self.write("simple-capture.pkg", b"pkg")
        runs = []
        queue = list(results)

        def run(cmd, **kw):
            if cmd[:3] == ["xcrun", "altool", "--help"]:
                return subprocess.CompletedProcess(cmd, 0, helptext, None)
            env = kw["env"]
            with open(os.path.join(env["API_PRIVATE_KEYS_DIR"], "AuthKey_ABCDE12345.p8")) as f:
                runs.append((cmd, f.read(), env))
            code, out, err = queue.pop(0)
            return subprocess.CompletedProcess(cmd, code, out, err)

        with mock.patch.object(asc.shutil, "which", return_value="/usr/bin/xcrun"), \
                mock.patch.object(asc.subprocess, "run", side_effect=run):
            try:
                self.run_main(FakeAsc(), ["upload", "--pkg", pkg, "--version", "0.1.0",
                                          "--build", "7"])
            finally:
                for _, _, env in runs:
                    self.assertFalse(os.path.exists(env["API_PRIVATE_KEYS_DIR"]))
                    self.assertFalse(any(k.startswith("ASC_KEY") for k in env))
        return [cmd for cmd, _, _ in runs], runs

    def test_uses_the_documented_flags(self):
        cmds, runs = self.upload((0, '{"success-message":"No errors uploading"}', "{ log"))
        self.assertEqual(len(cmds), 1)
        self.assertEqual(cmds[0][4:6], ["-t", "macos"])
        self.assertIn("--api-key", cmds[0])
        self.assertIn("--api-issuer", cmds[0])
        self.assertEqual(runs[0][1], FAKE_PEM)

    def test_falls_back_to_the_old_flags(self):
        cmds, _ = self.upload((64, "", "Error: Unknown option '--api-key'\nUsage: altool"),
                              (0, '{"success-message":"No errors uploading"}', ""))
        self.assertEqual(len(cmds), 2)
        self.assertIn("--apiKey", cmds[1])
        cmds, _ = self.upload((0, '{"success-message":"No errors uploading"}', ""),
                              helptext="--upload-package --apiKey --apiIssuer")
        self.assertIn("--apiKey", cmds[0])

    def test_terse_help_still_uploads_and_missing_altool_fails(self):
        cmds, _ = self.upload((0, '{"success-message":"No errors uploading"}', ""),
                              helptext="OVERVIEW: altool\nUSAGE: altool <subcommand>")
        self.assertIn("--api-key", cmds[0])
        with self.assertRaises(asc.Fail) as ctx:
            self.upload(helptext='xcrun: error: unable to find utility "altool", not a '
                                 'developer tool or in PATH')
        self.assertIn("comes with Xcode", str(ctx.exception))

    def test_fails_without_a_result(self):
        with self.assertRaises(asc.Fail) as ctx:
            self.upload((0, "", ""))
        self.assertIn("no JSON result", str(ctx.exception))
        with self.assertRaises(asc.Fail) as ctx:
            self.upload((0, '{"product-errors":[{"message":"ERROR ITMS-90238: bad"}]}', ""))
        self.assertIn("ITMS-90238", str(ctx.exception))


if __name__ == "__main__":
    unittest.main()
