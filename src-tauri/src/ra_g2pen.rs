//! g2p_en neural letter-to-sound, ported to plain Rust.
//!
//! Port of `G2p.predict` from g2p_en (Kyubyong Park & Jongseok Kim,
//! https://github.com/Kyubyong/g2p, Apache-2.0; see THIRD_PARTY_NOTICES.md).
//! A GRU encoder over graphemes feeds a GRU decoder that greedily emits
//! ARPAbet phonemes. Weights are the `checkpoint20.npz` arrays, converted by
//! `tools/read-aloud/convert_g2pen.py` into `data/g2pen.bin.gz` and embedded.
//! They are decompressed only in `G2pEn::load`, so nothing is resident until a
//! caller loads a `G2pEn` (and it is freed when that value is dropped).
//!
//! Never panics on malformed data or input: failures return `None`.

// Task 3 wires this into ra_names; until then only tests call it.
#![allow(dead_code)]

use std::io::Read;

static BLOB_GZ: &[u8] = include_bytes!("../data/g2pen.bin.gz");

/// Index 0..=3 are `<pad> <unk> <s> </s>`, then the ARPAbet set, as in g2p.py.
const PHONEMES: [&str; 74] = [
    "<pad>", "<unk>", "<s>", "</s>", "AA0", "AA1", "AA2", "AE0", "AE1", "AE2", "AH0", "AH1",
    "AH2", "AO0", "AO1", "AO2", "AW0", "AW1", "AW2", "AY0", "AY1", "AY2", "B", "CH", "D", "DH",
    "EH0", "EH1", "EH2", "ER0", "ER1", "ER2", "EY0", "EY1", "EY2", "F", "G", "HH", "IH0",
    "IH1", "IH2", "IY0", "IY1", "IY2", "JH", "K", "L", "M", "N", "NG", "OW0", "OW1", "OW2",
    "OY0", "OY1", "OY2", "P", "R", "S", "SH", "T", "TH", "UH0", "UH1", "UH2", "UW", "UW0",
    "UW1", "UW2", "V", "W", "Y", "Z", "ZH",
];
/// Graphemes are `<pad> <unk> </s>` then a..z, so 'a' is index 3.
const G_EOS: usize = 2;
const G_A: usize = 3;
const P_SOS: usize = 2;
const P_EOS: usize = 3;
const MAX_WORD: usize = 30;
const MAX_STEPS: usize = 30;

/// Row-major matrix (a vector when `rows == 1`).
struct Mat {
    rows: usize,
    cols: usize,
    data: Vec<f32>,
}

impl Mat {
    fn row(&self, i: usize) -> Option<&[f32]> {
        let start = i.checked_mul(self.cols)?;
        self.data.get(start..start.checked_add(self.cols)?)
    }
}

struct Gru {
    w_ih: Mat,
    w_hh: Mat,
    b_ih: Vec<f32>,
    b_hh: Vec<f32>,
    hidden: usize,
}

pub struct G2pEn {
    enc_emb: Mat,
    dec_emb: Mat,
    enc: Gru,
    dec: Gru,
    fc_w: Mat,
    fc_b: Vec<f32>,
}

type Arrays = Vec<(String, Vec<usize>, Vec<f32>)>;

/// Bounds-checked little-endian reader over the decompressed blob.
struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        let s = self.buf.get(self.pos..end)?;
        self.pos = end;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        self.take(1)?.first().copied()
    }
    fn u32(&mut self) -> Option<u32> {
        let b: [u8; 4] = self.take(4)?.try_into().ok()?;
        Some(u32::from_le_bytes(b))
    }
}

/// Parse a `G2PE` blob into (name, dims, data) entries. Trailing bytes are rejected.
fn parse_blob(raw: &[u8]) -> Option<Arrays> {
    let mut c = Cursor { buf: raw, pos: 0 };
    if c.take(4)? != b"G2PE" {
        return None;
    }
    let count = c.u32()? as usize;
    if count > 64 {
        return None;
    }
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let name_len = c.u8()? as usize;
        let name = std::str::from_utf8(c.take(name_len)?).ok()?.to_string();
        let ndim = c.u32()? as usize;
        if ndim == 0 || ndim > 2 {
            return None;
        }
        let mut dims = Vec::with_capacity(ndim);
        let mut n: usize = 1;
        for _ in 0..ndim {
            let d = c.u32()? as usize;
            n = n.checked_mul(d)?;
            dims.push(d);
        }
        let bytes = c.take(n.checked_mul(4)?)?;
        let data = bytes
            .chunks_exact(4)
            .map(|b| b.try_into().ok().map(f32::from_le_bytes))
            .collect::<Option<Vec<f32>>>()?;
        out.push((name, dims, data));
    }
    if c.pos != raw.len() {
        return None;
    }
    Some(out)
}

fn take_mat(arrays: &mut Arrays, name: &str) -> Option<Mat> {
    let i = arrays.iter().position(|(n, _, _)| n == name)?;
    let (_, dims, data) = arrays.swap_remove(i);
    let (rows, cols) = match dims.as_slice() {
        [r, c] => (*r, *c),
        [c] => (1, *c),
        _ => return None,
    };
    Some(Mat { rows, cols, data })
}

fn take_vec(arrays: &mut Arrays, name: &str, len: usize) -> Option<Vec<f32>> {
    let m = take_mat(arrays, name)?;
    (m.rows == 1 && m.cols == len).then_some(m.data)
}

fn take_gru(arrays: &mut Arrays, prefix: &str, input: usize) -> Option<Gru> {
    let w_ih = take_mat(arrays, &format!("{prefix}_w_ih"))?;
    let w_hh = take_mat(arrays, &format!("{prefix}_w_hh"))?;
    let hidden = w_hh.cols;
    let h3 = hidden.checked_mul(3)?;
    if hidden == 0 || w_ih.rows != h3 || w_ih.cols != input || w_hh.rows != h3 {
        return None;
    }
    let b_ih = take_vec(arrays, &format!("{prefix}_b_ih"), h3)?;
    let b_hh = take_vec(arrays, &format!("{prefix}_b_hh"), h3)?;
    Some(Gru { w_ih, w_hh, b_ih, b_hh, hidden })
}

/// `w @ x + b` for a row-major `rows x cols` matrix. Eight independent f32
/// lanes let the compiler vectorize (a single running sum cannot be reordered).
fn matvec(w: &Mat, x: &[f32], b: &[f32]) -> Option<Vec<f32>> {
    const L: usize = 8;
    if w.cols == 0 || x.len() != w.cols || b.len() != w.rows {
        return None;
    }
    let mut out = Vec::with_capacity(w.rows);
    for (row, bias) in w.data.chunks_exact(w.cols).zip(b) {
        let mut lanes = [0.0f32; L];
        let rc = row.chunks_exact(L);
        let xc = x.chunks_exact(L);
        let mut tail = 0.0f32;
        for (a, v) in rc.remainder().iter().zip(xc.remainder()) {
            tail += a * v;
        }
        for (ra, xa) in rc.zip(xc) {
            for ((acc, a), v) in lanes.iter_mut().zip(ra).zip(xa) {
                *acc += a * v;
            }
        }
        out.push(lanes.iter().sum::<f32>() + tail + bias);
    }
    (out.len() == w.rows).then_some(out)
}

fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

/// One GRU step, exactly g2p.py's `grucell`:
/// r,z = sigmoid(ih[:2H] + hh[:2H]); n = tanh(ih[2H:] + r*hh[2H:]); h' = (1-z)n + zh.
fn gru_step(g: &Gru, x: &[f32], h: &[f32]) -> Option<Vec<f32>> {
    let hn = g.hidden;
    if h.len() != hn {
        return None;
    }
    let ih = matvec(&g.w_ih, x, &g.b_ih)?;
    let hh = matvec(&g.w_hh, h, &g.b_hh)?;
    let mut out = Vec::with_capacity(hn);
    for i in 0..hn {
        let r = sigmoid(ih.get(i)? + hh.get(i)?);
        let z = sigmoid(ih.get(hn + i)? + hh.get(hn + i)?);
        let n = (ih.get(2 * hn + i)? + r * hh.get(2 * hn + i)?).tanh();
        out.push((1.0 - z) * n + z * h.get(i)?);
    }
    Some(out)
}

impl G2pEn {
    /// Decompress and parse the embedded weights. `None` (logged) on any failure.
    pub fn load() -> Option<G2pEn> {
        let r = Self::load_from_gz(BLOB_GZ);
        if r.is_none() {
            log::warn!("ra_g2pen: embedded g2p_en weights failed to load");
        }
        r
    }

    fn load_from_gz(gz: &[u8]) -> Option<G2pEn> {
        let mut raw = Vec::new();
        flate2::read::GzDecoder::new(gz).read_to_end(&mut raw).ok()?;
        Self::from_raw(&raw)
    }

    fn from_raw(raw: &[u8]) -> Option<G2pEn> {
        let mut a = parse_blob(raw)?;
        let enc_emb = take_mat(&mut a, "enc_emb")?;
        let dec_emb = take_mat(&mut a, "dec_emb")?;
        if enc_emb.rows != G_A + 26 || dec_emb.rows != PHONEMES.len() {
            return None;
        }
        let enc = take_gru(&mut a, "enc", enc_emb.cols)?;
        let dec = take_gru(&mut a, "dec", dec_emb.cols)?;
        if enc.hidden != dec.hidden {
            return None;
        }
        let fc_w = take_mat(&mut a, "fc_w")?;
        if fc_w.rows != PHONEMES.len() || fc_w.cols != dec.hidden {
            return None;
        }
        let fc_b = take_vec(&mut a, "fc_b", fc_w.rows)?;
        Some(G2pEn { enc_emb, dec_emb, enc, dec, fc_w, fc_b })
    }

    /// ARPAbet with stress digits, space-separated (e.g. `"P EH0 L OW1 S IY0"`).
    /// `None` for empty, longer than 30 chars, or anything outside a-z.
    pub fn predict(&self, word_lower: &str) -> Option<String> {
        if word_lower.is_empty()
            || word_lower.len() > MAX_WORD
            || !word_lower.bytes().all(|b| b.is_ascii_lowercase())
        {
            return None;
        }
        let ids = self.decode(&self.encode(word_lower)?)?;
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            out.push(*PHONEMES.get(id)?);
        }
        Some(out.join(" "))
    }

    /// Encoder: graphemes + `</s>` through the GRU from a zero state; last hidden.
    fn encode(&self, word: &str) -> Option<Vec<f32>> {
        let mut h = vec![0.0f32; self.enc.hidden];
        let ids = word
            .bytes()
            .map(|b| G_A + usize::from(b.wrapping_sub(b'a')))
            .chain([G_EOS]);
        for id in ids {
            h = gru_step(&self.enc, self.enc_emb.row(id)?, &h)?;
        }
        Some(h)
    }

    /// Greedy decoder from `<s>`, stopping at `</s>` or after `MAX_STEPS`.
    /// (Python stops at 20; every fixture word ends well before that.)
    fn decode(&self, enc_h: &[f32]) -> Option<Vec<usize>> {
        let mut h = enc_h.to_vec();
        let mut prev = P_SOS;
        let mut preds = Vec::new();
        for _ in 0..MAX_STEPS {
            h = gru_step(&self.dec, self.dec_emb.row(prev)?, &h)?;
            let logits = matvec(&self.fc_w, &h, &self.fc_b)?;
            // First maximum wins, like numpy argmax.
            let mut best = 0usize;
            let mut best_v = f32::NEG_INFINITY;
            for (i, v) in logits.iter().enumerate() {
                if *v > best_v {
                    best = i;
                    best_v = *v;
                }
            }
            if best == P_EOS {
                break;
            }
            preds.push(best);
            prev = best;
        }
        Some(preds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_python_g2p_en_on_fixture() {
        let g = G2pEn::load().expect("weights load");
        let mut bad = Vec::new();
        for line in include_str!("ra_g2pen_fixture.tsv").lines().filter(|l| !l.is_empty()) {
            let (w, want) = line.split_once('\t').unwrap();
            let got = g.predict(w).unwrap_or_default();
            if got != want { bad.push(format!("{w}: got {got} want {want}")); }
        }
        assert!(bad.is_empty(), "{bad:#?}");
    }

    #[test]
    fn rejects_bad_input_without_panicking() {
        let g = G2pEn::load().unwrap();
        for w in ["", "Pelosi", "o'neil", "東京", "🙂", &"a".repeat(31), "abc1"] {
            assert!(g.predict(w).is_none(), "{w}");
        }
    }

    #[test]
    #[ignore] // perf: run in release: cargo test --release ra_g2pen -- --ignored
    fn under_5ms_per_word_release() {
        let g = G2pEn::load().unwrap();
        let t = std::time::Instant::now();
        for _ in 0..10 { for w in ["zelensky", "lukashenko", "kubernetes"] { g.predict(w); } }
        eprintln!("ra_g2pen: {:.3} ms/word", t.elapsed().as_secs_f64() * 1000.0 / 30.0);
        assert!(t.elapsed().as_secs_f64() * 1000.0 / 30.0 < 5.0, "{:?}", t.elapsed());
    }

    #[test]
    fn malformed_blobs_give_none_without_panicking() {
        let mut raw = Vec::new();
        flate2::read::GzDecoder::new(BLOB_GZ).read_to_end(&mut raw).unwrap();
        assert!(G2pEn::from_raw(&raw).is_some());
        // Truncations, bad magic, trailing junk.
        for cut in [0, 3, 4, 7, 8, 9, 20, 100, raw.len() / 2, raw.len() - 1] {
            assert!(G2pEn::from_raw(&raw[..cut]).is_none(), "cut {cut}");
        }
        let mut bad = raw.clone();
        bad[0] = b'X';
        assert!(G2pEn::from_raw(&bad).is_none());
        let mut long = raw.clone();
        long.push(0);
        assert!(G2pEn::from_raw(&long).is_none());
        // Huge declared dims must not allocate or overflow.
        let mut huge = b"G2PE".to_vec();
        huge.extend(1u32.to_le_bytes());
        huge.push(1);
        huge.push(b'x');
        huge.extend(2u32.to_le_bytes());
        huge.extend(u32::MAX.to_le_bytes());
        huge.extend(u32::MAX.to_le_bytes());
        assert!(G2pEn::from_raw(&huge).is_none());
        // Non-gzip and truncated gzip.
        assert!(G2pEn::load_from_gz(b"not gzip").is_none());
        assert!(G2pEn::load_from_gz(&BLOB_GZ[..BLOB_GZ.len() / 2]).is_none());
    }
}
