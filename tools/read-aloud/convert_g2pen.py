"""Convert g2p_en's checkpoint20.npz into the compact blob Read Aloud embeds,
and generate the Rust parity fixture from the reference Python model.

Usage: python tools/read-aloud/convert_g2pen.py <path/to/g2p_en/checkpoint20.npz>

g2p_en (https://github.com/Kyubyong/g2p) is Apache-2.0; see THIRD_PARTY_NOTICES.md.

Blob layout (all little-endian), gzip level 9:
  b"G2PE", u32 count,
  per array: u8 name_len, name, u32 ndim, u32 dims[ndim], f32 data[prod(dims)]
"""
import gzip
import os
import struct
import sys
import types

import numpy as np

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
OUT_BLOB = os.path.join(ROOT, "src-tauri", "data", "g2pen.bin.gz")
OUT_FIXTURE = os.path.join(ROOT, "src-tauri", "src", "ra_g2pen_fixture.tsv")

# Fixed order; the Rust loader looks arrays up by name, but keep it stable.
NAMES = [
    "enc_emb", "enc_w_ih", "enc_w_hh", "enc_b_ih", "enc_b_hh",
    "dec_emb", "dec_w_ih", "dec_w_hh", "dec_b_ih", "dec_b_hh",
    "fc_w", "fc_b",
]

WORDS = (
    "pelosi zelensky buttigieg nguyen schumer kamala putin netanyahu anthropic "
    "kubernetes nvidia ozempic erdogan bezos tlaib ramaswamy manchin sinema "
    "youngkin mayorkas navalny lukashenko sunak kishida milei sheinbaum houthi "
    "kharkiv bakhmut moderna"
).split()


def write_blob(npz_path):
    v = np.load(npz_path)
    out = bytearray(b"G2PE")
    out += struct.pack("<I", len(NAMES))
    for name in NAMES:
        a = np.ascontiguousarray(v[name], dtype="<f4")
        nb = name.encode("ascii")
        out += struct.pack("<B", len(nb)) + nb
        out += struct.pack("<I", a.ndim)
        for d in a.shape:
            out += struct.pack("<I", d)
        out += a.tobytes()
    # mtime=0 so the blob is byte-identical across runs.
    with open(OUT_BLOB, "wb") as f:
        with gzip.GzipFile(fileobj=f, mode="wb", compresslevel=9, mtime=0) as gz:
            gz.write(bytes(out))
    print(f"raw {len(out)} bytes -> {OUT_BLOB} {os.path.getsize(OUT_BLOB)} bytes")


def load_reference_model(npz_path):
    """Import g2p_en's g2p.py with nltk/inflect stubbed out (we only need predict)."""
    for m in ["nltk", "nltk.corpus", "nltk.tokenize", "inflect", "distance"]:
        sys.modules[m] = types.ModuleType(m)
    sys.modules["nltk"].pos_tag = None
    sys.modules["nltk"].data = types.SimpleNamespace(find=lambda *a: None)
    sys.modules["nltk"].download = lambda *a, **k: None
    sys.modules["nltk.corpus"].cmudict = types.SimpleNamespace(dict=lambda: {})
    sys.modules["nltk.tokenize"].TweetTokenizer = lambda *a, **k: types.SimpleNamespace(tokenize=None)
    sys.modules["nltk.tokenize"].word_tokenize = None
    sys.modules["inflect"].engine = lambda: None
    g2p_py = os.path.join(os.path.dirname(os.path.abspath(npz_path)), "g2p.py")
    src = open(g2p_py, encoding="utf8").read().replace(
        "from .expand import normalize_numbers", "normalize_numbers=None")
    mod = types.ModuleType("g2pmod")
    mod.__file__ = g2p_py
    exec(compile(src, g2p_py, "exec"), mod.__dict__)
    return mod.G2p()


def write_fixture(npz_path):
    g = load_reference_model(npz_path)
    lines = []
    for w in WORDS:
        # Words are already lowercase a-z, so predict() sees exactly what
        # G2p.__call__ would pass it after its lower() + [^a-z] filter.
        assert w.isascii() and w.isalpha() and w.islower(), w
        pron = g.predict(w)
        # Python caps decode at 20 steps, Rust at 30: a 20-phoneme result would
        # mean Python truncated and the two could legitimately differ.
        assert len(pron) < 20, (w, pron)
        lines.append(f"{w}\t{' '.join(pron)}")
    with open(OUT_FIXTURE, "w", encoding="utf8", newline="\n") as f:
        f.write("\n".join(lines) + "\n")
    print(f"{len(lines)} lines -> {OUT_FIXTURE}")


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    write_blob(sys.argv[1])
    write_fixture(sys.argv[1])
