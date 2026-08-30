#!/usr/bin/env python3
"""canonical-json.v1 (gate-tax 8 §7a) — the recipe encoder.

Normative form: UTF-8; object members sorted by key code-point order;
array order preserved; no insignificant whitespace; strings minimally
escaped (\" \\ \b \f \n \r \t, other U+0000-001F as lowercase \\u00xx,
no \\/, non-ASCII as raw UTF-8); numbers integer-only (optional '-',
no leading zeros, |n| <= 2^53-1).

Normative REJECTIONS (typed abstain reasons, never encoded):
  non-integer-number   fractions/exponents/out-of-range anywhere
  negative-zero        a -0 source lexeme
  duplicate-member     repeated object member names (parsed-name
                       equality — the escaped-equivalent case rejects)
  lone-surrogate       a string that cannot round-trip to UTF-8

Rejection can only shrink modeled coverage; it can never fabricate
agreement. The mjs mirror lands with the first oracle-side consumer
(phasing note in the packet's implementation record) and must pass
this module's golden set byte-for-byte.

API: encode_source(text) -> (bytes, None) | (None, reason)
CLI: canonical_json.py <file>  (prints hex sha256 or REJECTED:<reason>)
"""
import hashlib
import json
import sys

ESCAPES = {
    '"': '\\"',
    "\\": "\\\\",
    "\b": "\\b",
    "\f": "\\f",
    "\n": "\\n",
    "\r": "\\r",
    "\t": "\\t",
}


class Rejection(Exception):
    def __init__(self, reason):
        super().__init__(reason)
        self.reason = reason


def _pairs_hook(pairs):
    names = [key for key, _ in pairs]
    if len(set(names)) != len(names):
        raise Rejection("duplicate-member")
    return dict(pairs)


def _parse_int(lexeme):
    if lexeme == "-0":
        raise Rejection("negative-zero")
    value = int(lexeme)
    if abs(value) > 2**53 - 1:
        raise Rejection("non-integer-number")
    return value


def _parse_float(_lexeme):
    raise Rejection("non-integer-number")


def _encode_string(value):
    out = ['"']
    for char in value:
        if char in ESCAPES:
            out.append(ESCAPES[char])
        elif ord(char) < 0x20:
            out.append(f"\\u{ord(char):04x}")
        else:
            out.append(char)
    out.append('"')
    return "".join(out)


def _encode(value):
    if value is None:
        return "null"
    if value is True:
        return "true"
    if value is False:
        return "false"
    if isinstance(value, int):
        return str(value)
    if isinstance(value, str):
        return _encode_string(value)
    if isinstance(value, list):
        return "[" + ",".join(_encode(item) for item in value) + "]"
    if isinstance(value, dict):
        return "{" + ",".join(
            f"{_encode_string(key)}:{_encode(value[key])}"
            for key in sorted(value)
        ) + "}"
    raise Rejection("non-integer-number")


def encode_source(text):
    """Canonical bytes for a JSON source text, or a typed rejection."""
    try:
        parsed = json.loads(
            text,
            object_pairs_hook=_pairs_hook,
            parse_int=_parse_int,
            parse_float=_parse_float,
            parse_constant=lambda _lexeme: (_ for _ in ()).throw(
                Rejection("non-integer-number")
            ),
        )
    except Rejection as rejection:
        return None, rejection.reason
    except ValueError:
        return None, "invalid-json"
    try:
        encoded = _encode(parsed).encode("utf-8")
    except Rejection as rejection:
        return None, rejection.reason
    except UnicodeEncodeError:
        return None, "lone-surrogate"
    return encoded, None


def exclude_members(text, pointers):
    """Parse, remove the members the JSON pointers name (model-level
    member removal — the self-sha256-after exclusion), re-encode."""
    encoded, reason = encode_source(text)
    if reason:
        return None, reason
    parsed = json.loads(text, object_pairs_hook=_pairs_hook,
                        parse_int=_parse_int, parse_float=_parse_float)
    for pointer in pointers:
        parts = [part.replace("~1", "/").replace("~0", "~")
                 for part in pointer.lstrip("/").split("/")]
        node = parsed
        for part in parts[:-1]:
            node = node[int(part)] if isinstance(node, list) else node[part]
        if isinstance(node, dict) and parts[-1] in node:
            del node[parts[-1]]
        else:
            return None, "self-recipe-scope"
    try:
        return _encode(parsed).encode("utf-8"), None
    except (Rejection, UnicodeEncodeError):
        return None, "lone-surrogate"


if __name__ == "__main__":
    body = open(sys.argv[1], encoding="utf-8", errors="surrogatepass").read()
    canonical, why = encode_source(body)
    if why:
        print(f"REJECTED:{why}")
        sys.exit(1)
    print(hashlib.sha256(canonical).hexdigest())
