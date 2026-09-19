// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Dense vectors: quantisation, the encoder seam, and the maintainer command
//! that fills the corpus in.
//!
//! Two halves that only meet in `index.json`:
//!
//! * **Quantisation** ([`quantise`], [`dequantise`], [`cosine_i8`]) is pure
//!   arithmetic, always compiled, and is what `search` uses at query time.
//! * **The encoder** ([`Encoder`]) is behind the optional `semantic` feature,
//!   because it pulls ONNX Runtime. Without the feature this module still
//!   compiles and [`Encoder::load`] simply reports that this build has no
//!   encoder — so nothing downstream needs a `cfg`.
//!
//! # Why the encoder's name is written into the corpus
//!
//! [`ENCODER_ID`] is stamped into `index.json` by [`build`] and compared
//! against at query time. That check is the reason this constant exists as a
//! constant rather than being implicit in which model the code happens to
//! call: two *different* sentence encoders of the same dimensionality produce
//! cosines that are numerically perfect and semantically meaningless. There is
//! no runtime symptom — no error, no NaN, no obviously wrong number — just
//! quietly worse results forever. So the identity is data, it travels with the
//! vectors, and a mismatch refuses the vector path rather than degrading it.
//!
//! # The `build-vectors` pass encodes the index file it read, never
//! [`teksilo_corpus::index`]
//!
//! [`teksilo_corpus::index`] returns the corpus *embedded in this binary at
//! compile time*. That is exactly right at query time and exactly wrong here:
//! this command runs immediately after `tools/build_corpus.py` has rewritten
//! `index.json` on disk, against a `cargo-teksilo` that was compiled before
//! it — so the embedded copy is one revision stale by construction, and
//! encoding it would write vectors for text the file no longer contains.
//!
//! [`build`] therefore parses the `index.json` it was pointed at and encodes
//! *that* [`Index`]'s [`teksilo_corpus::Chunk::text`]. Before chunk text lived
//! in the index, the same hazard was answered by re-implementing the
//! generator's line-slicing against the copied files next to `index.json`; the
//! file now carries its own text, so the disk read is the whole of it.

use std::path::{Path, PathBuf};

use teksilo_corpus::{ChunkEmbedding, Index};

/// The encoder the corpus vectors are produced with, named exactly.
///
/// Served by `fastembed` from the `Xenova/bge-small-en-v1.5` ONNX conversion;
/// this is the upstream model that conversion is of, which is the name worth
/// recording because it is what determines the vector space.
pub const ENCODER_ID: &str = "BAAI/bge-small-en-v1.5";

/// Dimensionality of [`ENCODER_ID`]'s output.
pub const ENCODER_DIM: u32 = 384;

/// The instruction BGE v1.5 asks for in front of a *retrieval query*.
///
/// Asymmetric by design: passages are encoded bare, queries carry this
/// prefix. It is the model card's own wording, and it matters most for
/// exactly the short queries this tool sees.
pub const QUERY_PREFIX: &str = "Represent this sentence for searching relevant passages: ";

/// Batch size for encoding, fixed rather than defaulted.
///
/// A batch is padded to its longest member, so the shape of a batch feeds
/// into the floating-point reduction order. Pinning it keeps a re-run
/// byte-identical, which is the whole determinism requirement.
const ENCODE_BATCH: usize = 32;

// ---------------------------------------------------------------------------
// Quantisation
// ---------------------------------------------------------------------------

/// Quantise a vector to int8 plus the scale that decodes it.
///
/// The scale is `max|v| / 127`, so the largest-magnitude component lands on
/// ±127 and every vector uses the full range. `127` rather than `128` keeps
/// the range symmetric: with `-128` reachable, negating a vector would not
/// negate its quantisation.
///
/// A zero (or non-finite) vector gets `scale = 1.0` rather than `0.0`, so a
/// consumer decoding it never divides by the scale and never produces NaN.
pub fn quantise(values: &[f32]) -> (Vec<i8>, f32) {
    let max_abs = values
        .iter()
        .copied()
        .filter(|v| v.is_finite())
        .fold(0.0f32, |acc, v| acc.max(v.abs()));
    let scale = if max_abs > 0.0 { max_abs / 127.0 } else { 1.0 };
    let quantised = values
        .iter()
        .map(|&v| {
            let scaled = (v / scale).round();
            // NaN clamps to NaN and casts to 0, which is the right answer for
            // a dimension the encoder could not produce.
            scaled.clamp(-127.0, 127.0) as i8
        })
        .collect();
    (quantised, scale)
}

/// Decode a quantised vector back to `f32`.
pub fn dequantise(values: &[i8], scale: f32) -> Vec<f32> {
    values.iter().map(|&q| f32::from(q) * scale).collect()
}

/// Plain `f32` cosine, used to *measure* what quantisation cost.
///
/// Not on the query path — that is [`cosine_i8`] — so this exists only to
/// report a number rather than to assume one.
pub fn cosine_f32(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm =
        a.iter().map(|x| x * x).sum::<f32>().sqrt() * b.iter().map(|y| y * y).sum::<f32>().sqrt();
    if norm == 0.0 { 0.0 } else { dot / norm }
}

/// Cosine similarity between two quantised vectors.
///
/// The scales cancel out of the quotient whenever both are positive — which
/// [`quantise`] guarantees — so this is arithmetically a cosine on the raw
/// int8s. They are taken as parameters anyway: the cancellation is a property
/// of this quantiser's sign convention, not of cosine, and a caller reading
/// the signature should not have to know that to trust the result.
///
/// Returns `0.0` for a length mismatch or a zero vector, which ranks the pair
/// last rather than propagating a NaN through a sort.
pub fn cosine_i8(a: &[i8], a_scale: f32, b: &[i8], b_scale: f32) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let (mut dot, mut norm_a, mut norm_b) = (0i64, 0i64, 0i64);
    for (&x, &y) in a.iter().zip(b.iter()) {
        dot += i64::from(x) * i64::from(y);
        norm_a += i64::from(x) * i64::from(x);
        norm_b += i64::from(y) * i64::from(y);
    }
    if norm_a == 0 || norm_b == 0 {
        return 0.0;
    }
    let numerator = dot as f64 * f64::from(a_scale) * f64::from(b_scale);
    let denominator =
        (norm_a as f64).sqrt() * f64::from(a_scale) * (norm_b as f64).sqrt() * f64::from(b_scale);
    (numerator / denominator) as f32
}

// ---------------------------------------------------------------------------
// The encoder seam
// ---------------------------------------------------------------------------

/// Why an encoding could not happen, in terms a caller can act on.
///
/// Every variant is a reason to *degrade to lexical search*, never a reason to
/// fail a query — which is why `search` matches on this rather than
/// propagating it.
#[derive(Debug, thiserror::Error)]
// Each build constructs a different subset: with `semantic` there is no
// `NotCompiledIn`, without it there is nothing but. The variants all stay so
// that the type — and every `match` on it — is one shape in both builds.
#[allow(dead_code)]
pub enum EncoderError {
    #[error(
        "this build of cargo-teksilo has no encoder: it was compiled with \
         `--no-default-features`, which leaves out the `semantic` feature."
    )]
    NotCompiledIn,

    #[error(
        "could not initialise the {ENCODER_ID} encoder: {0}\n\
         (the model downloads once on first use; this needs network access, \
         or a populated FASTEMBED_CACHE_DIR / HF_HOME)"
    )]
    Init(String),

    #[error("could not encode text with {ENCODER_ID}: {0}")]
    Embed(String),

    #[error("{ENCODER_ID} returned a {got}-dimension vector, expected {ENCODER_DIM}")]
    WrongDimension { got: usize },
}

/// A loaded sentence encoder.
///
/// Deliberately the same type with and without the `semantic` feature: the
/// feature changes whether [`Encoder::load`] can ever succeed, not the shape
/// of the code that calls it.
pub struct Encoder {
    #[cfg(feature = "semantic")]
    inner: fastembed::TextEmbedding,
}

/// Where the downloaded encoder weights live.
///
/// fastembed's own default is `./.fastembed_cache`, **relative to the working
/// directory** — so a consumer running `cargo teksilo search` in their app
/// would find 129 MB of ONNX weights dropped into their repository root, and
/// again in every other directory they ran it from. That is not a cache, it is
/// litter, and it is the kind of thing that gets a tool uninstalled.
///
/// A per-user cache directory is the right home: shared across every project,
/// outside every repository, and conventional per platform. `FASTEMBED_CACHE_DIR`
/// still wins when set, because someone pinning it has a reason; `HF_HOME` takes
/// precedence over both inside fastembed itself.
#[cfg(feature = "semantic")]
fn model_cache_dir() -> std::path::PathBuf {
    if let Some(explicit) = std::env::var_os("FASTEMBED_CACHE_DIR") {
        return std::path::PathBuf::from(explicit);
    }
    base_cache_dir().join("teksilo").join("fastembed")
}

#[cfg(target_os = "macos")]
#[cfg(feature = "semantic")]
fn base_cache_dir() -> std::path::PathBuf {
    home_dir().join("Library").join("Caches")
}

#[cfg(windows)]
#[cfg(feature = "semantic")]
fn base_cache_dir() -> std::path::PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| home_dir().join("AppData").join("Local"))
}

#[cfg(all(unix, not(target_os = "macos")))]
#[cfg(feature = "semantic")]
fn base_cache_dir() -> std::path::PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".cache"))
}

/// The user's home, or the current directory if the platform will not say.
///
/// Falling back to `.` reproduces fastembed's own behaviour rather than
/// failing outright — a machine with no home is unusual enough that a working
/// tool beats a correct complaint.
#[cfg(feature = "semantic")]
fn home_dir() -> std::path::PathBuf {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("."))
}

impl Encoder {
    /// Load [`ENCODER_ID`], downloading it on first use.
    ///
    /// `show_progress` draws fastembed's download bar on stderr — wanted for
    /// the one-off `build-vectors` run, unwanted for a query, where a caller
    /// should print its own one-line note instead.
    #[cfg(feature = "semantic")]
    pub fn load(show_progress: bool) -> Result<Self, EncoderError> {
        use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};

        let options = TextInitOptions::new(EmbeddingModel::BGESmallENV15)
            .with_show_download_progress(show_progress)
            .with_cache_dir(model_cache_dir());
        let inner =
            TextEmbedding::try_new(options).map_err(|e| EncoderError::Init(e.to_string()))?;
        Ok(Self { inner })
    }

    /// The no-encoder build's `load`: always the same, honest failure.
    #[cfg(not(feature = "semantic"))]
    pub fn load(_show_progress: bool) -> Result<Self, EncoderError> {
        Err(EncoderError::NotCompiledIn)
    }

    /// Encode passages — corpus chunks — with no instruction prefix.
    #[cfg(feature = "semantic")]
    pub fn embed_documents(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>, EncoderError> {
        let vectors = self
            .inner
            .embed(texts, Some(ENCODE_BATCH))
            .map_err(|e| EncoderError::Embed(e.to_string()))?;
        for vector in &vectors {
            if vector.len() != ENCODER_DIM as usize {
                return Err(EncoderError::WrongDimension { got: vector.len() });
            }
        }
        Ok(vectors)
    }

    #[cfg(not(feature = "semantic"))]
    pub fn embed_documents(&mut self, _texts: &[String]) -> Result<Vec<Vec<f32>>, EncoderError> {
        Err(EncoderError::NotCompiledIn)
    }

    /// Encode a retrieval query, with [`QUERY_PREFIX`] in front of it.
    pub fn embed_query(&mut self, query: &str) -> Result<Vec<f32>, EncoderError> {
        let prefixed = vec![format!("{QUERY_PREFIX}{query}")];
        let mut vectors = self.embed_documents(&prefixed)?;
        vectors
            .pop()
            .ok_or(EncoderError::Embed("no vector returned".into()))
    }
}

// ---------------------------------------------------------------------------
// Warming the cache
// ---------------------------------------------------------------------------

/// Roughly what the weights cost to fetch, for a plan that offers to do it.
///
/// A number in a plan is a promise, so it is stated once here rather than
/// retyped wherever a prompt needs it.
pub const ENCODER_DOWNLOAD_MB: u32 = 129;

/// Fetch the encoder now, so the first `search` does not stall on it.
///
/// Loading **is** the download: fastembed pulls the ONNX weights into
/// [`model_cache_dir`] while constructing the model, so building one and
/// dropping it leaves exactly the cache a later query wants. A separate
/// "download" entry point would be a second route to the same bytes, free to
/// disagree with the one `search` actually takes.
///
/// Progress is shown: this is an interactive one-off that moves ~129 MB, and a
/// silent two-minute pause reads as a hang.
#[cfg(feature = "semantic")]
pub fn prefetch_encoder() -> Result<std::path::PathBuf, EncoderError> {
    drop(Encoder::load(true)?);
    Ok(model_cache_dir())
}

/// The no-encoder build's pre-fetch: nothing to fetch, and it says so.
#[cfg(not(feature = "semantic"))]
pub fn prefetch_encoder() -> Result<std::path::PathBuf, EncoderError> {
    Err(EncoderError::NotCompiledIn)
}

/// Where the weights would land — `None` when this build has no encoder.
#[cfg(feature = "semantic")]
pub fn encoder_cache_dir() -> Option<std::path::PathBuf> {
    Some(model_cache_dir())
}

#[cfg(not(feature = "semantic"))]
pub fn encoder_cache_dir() -> Option<std::path::PathBuf> {
    None
}

/// Whether the weights are already on disk.
///
/// A plan must not offer to download 129 MB that is already there. The probe
/// is "any `.onnx` under the cache directory" rather than a guess at
/// fastembed's internal directory layout, which is its own to change: a false
/// negative costs one no-op re-check, a false positive would be a lie in a
/// plan.
pub fn encoder_is_cached() -> bool {
    fn has_onnx(dir: &std::path::Path) -> bool {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return false;
        };
        entries.flatten().any(|entry| {
            let path = entry.path();
            if path.is_dir() {
                has_onnx(&path)
            } else {
                path.extension().is_some_and(|ext| ext == "onnx")
            }
        })
    }

    encoder_cache_dir().is_some_and(|dir| has_onnx(&dir))
}

// ---------------------------------------------------------------------------
// `cargo teksilo build-vectors`
// ---------------------------------------------------------------------------

/// Why `build-vectors` could not complete.
#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error("{0}")]
    Usage(String),

    #[error(
        "no corpus index at {0}.\n\
         `build-vectors` is a maintainer command: run it from a teksilo \
         checkout, or pass --corpus <path to index.json>."
    )]
    NoIndex(PathBuf),

    #[error("could not read {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("could not write {path}: {source}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("{0} is not a valid corpus index: {1}")]
    Parse(PathBuf, String),

    #[error(
        "{0} does not survive a serde round trip unchanged, so writing vectors \
         into it would also reformat it and break `build_corpus.py --check`.\n\
         First difference at byte {1}. This means the Rust schema and \
         tools/build_corpus.py have diverged; reconcile them before rebuilding \
         vectors."
    )]
    NotRoundTrippable(PathBuf, usize),

    #[error(transparent)]
    Encoder(#[from] EncoderError),
}

/// Parsed `build-vectors` arguments.
#[derive(Debug, Default, PartialEq, Eq)]
struct BuildArgs {
    corpus: Option<PathBuf>,
}

fn parse_build_args(args: &[String]) -> Result<BuildArgs, BuildError> {
    let mut parsed = BuildArgs::default();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--corpus" => {
                let value = it
                    .next()
                    .ok_or_else(|| BuildError::Usage("--corpus needs a path".into()))?;
                parsed.corpus = Some(PathBuf::from(value));
            }
            other if other.starts_with("--corpus=") => {
                parsed.corpus = Some(PathBuf::from(&other["--corpus=".len()..]));
            }
            other => {
                return Err(BuildError::Usage(format!(
                    "unknown argument `{other}`\n\nUSAGE: cargo teksilo build-vectors \
                     [--corpus <path to index.json>]"
                )));
            }
        }
    }
    Ok(parsed)
}

/// The committed corpus index, relative to a repository root.
const CORPUS_INDEX_REL: &str = "crates/teksilo-corpus/corpus/index.json";

/// Find the committed `index.json` by walking up from `dir`.
///
/// A maintainer runs this from anywhere in the checkout, so an exact-cwd
/// requirement would be a papercut with no upside.
fn locate_index(dir: &Path) -> Option<PathBuf> {
    let mut here = Some(dir);
    while let Some(candidate) = here {
        let path = candidate.join(CORPUS_INDEX_REL);
        if path.is_file() {
            return Some(path);
        }
        here = candidate.parent();
    }
    None
}

/// What to encode: every chunk's own stored text, in index order.
///
/// A one-liner with a name, because *which* `Index` it is handed is the whole
/// correctness argument — the one parsed from the file on disk, never
/// [`teksilo_corpus::index`]'s compile-time copy. See the module docs.
fn chunk_texts(index: &Index) -> Vec<String> {
    index
        .chunks
        .iter()
        .map(|chunk| chunk.text().to_string())
        .collect()
}

/// Serialize an index exactly as `tools/build_corpus.py` writes it: compact,
/// non-ASCII unescaped, one trailing newline.
fn serialize_index(index: &Index) -> Result<String, BuildError> {
    let mut text = serde_json::to_string(index)
        .map_err(|e| BuildError::Parse(PathBuf::from("<in memory>"), e.to_string()))?;
    text.push('\n');
    Ok(text)
}

/// Byte offset of the first difference between two strings, if any.
fn first_difference(a: &str, b: &str) -> Option<usize> {
    a.as_bytes()
        .iter()
        .zip(b.as_bytes())
        .position(|(x, y)| x != y)
        .or_else(|| (a.len() != b.len()).then(|| a.len().min(b.len())))
}

/// Run `cargo teksilo build-vectors`.
///
/// Returns the process exit code. Everything it prints is a summary; the
/// only artifact is the rewritten `index.json`.
pub fn build(dir: &Path, args: &[String]) -> Result<i32, BuildError> {
    let parsed = parse_build_args(args)?;
    let index_path = match parsed.corpus {
        Some(path) => {
            if !path.is_file() {
                return Err(BuildError::NoIndex(path));
            }
            path
        }
        None => {
            locate_index(dir).ok_or_else(|| BuildError::NoIndex(PathBuf::from(CORPUS_INDEX_REL)))?
        }
    };
    let original = std::fs::read_to_string(&index_path).map_err(|source| BuildError::Read {
        path: index_path.clone(),
        source,
    })?;
    let mut index: Index = serde_json::from_str(&original)
        .map_err(|e| BuildError::Parse(index_path.clone(), e.to_string()))?;

    // Round-trip guard, *before* any encoding work. If re-serializing the file
    // unchanged does not reproduce it byte for byte, then writing vectors into
    // it would silently reformat everything else too — and the reformatting,
    // not the vectors, is what would then show up as corpus drift in CI.
    let reserialized = serialize_index(&index)?;
    if let Some(at) = first_difference(&original, &reserialized) {
        return Err(BuildError::NotRoundTrippable(index_path, at));
    }

    println!(
        "corpus: {} ({} chunks, teksilo {})",
        index_path.display(),
        index.chunk_count,
        index.teksilo_version
    );
    println!("encoder: {ENCODER_ID} ({ENCODER_DIM} dimensions, int8 + per-vector scale)");

    let texts = chunk_texts(&index);

    let mut encoder = Encoder::load(true)?;
    let mut vectors = Vec::with_capacity(texts.len());
    for (batch_no, batch) in texts.chunks(ENCODE_BATCH).enumerate() {
        vectors.extend(encoder.embed_documents(batch)?);
        if batch_no % 8 == 0 {
            eprint!("\r  encoded {} / {} chunks", vectors.len(), texts.len());
        }
    }
    eprintln!("\r  encoded {} / {} chunks", vectors.len(), texts.len());

    let mut worst_fidelity = 1.0f32;
    for (chunk, vector) in index.chunks.iter_mut().zip(vectors.iter()) {
        let (values, scale) = quantise(vector);
        // What int8 storage actually costs, measured rather than asserted:
        // the cosine between the encoder's own output and what a consumer
        // will decode back out of the corpus.
        worst_fidelity = worst_fidelity.min(cosine_f32(vector, &dequantise(&values, scale)));
        chunk.embedding = Some(ChunkEmbedding { scale, values });
    }
    index.encoder = Some(ENCODER_ID.to_string());
    index.encoder_dim = Some(ENCODER_DIM);

    let written = serialize_index(&index)?;
    std::fs::write(&index_path, &written).map_err(|source| BuildError::Write {
        path: index_path.clone(),
        source,
    })?;

    println!(
        "wrote {} vectors: {} -> {} bytes (+{:.1} %), worst quantisation fidelity {:.5}",
        index.chunks.len(),
        original.len(),
        written.len(),
        (written.len() as f64 / original.len() as f64 - 1.0) * 100.0,
        worst_fidelity
    );
    // `teksilo-corpus` embeds `corpus/index.json` with `include_str!`, which
    // cargo cannot see through on its own — the crate would look fresh with a
    // stale corpus baked in. Its `build.rs` declares that file, so an ordinary
    // build re-embeds; this note says so rather than leaving the reader to
    // wonder why a freshly written vector did not show up in the next query.
    println!(
        "note: `cargo teksilo search` reads the corpus embedded at compile time. \
         Rebuild to query these vectors:\n\
         \x20   cargo build -p cargo-teksilo"
    );
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_corpus::Chunk;

    /// A deterministic pseudo-random unit vector, so the fidelity assertions
    /// below are about the quantiser rather than about one lucky sample.
    fn pseudo_unit_vector(seed: u64, dim: usize) -> Vec<f32> {
        let mut state = seed | 1;
        let mut values: Vec<f32> = (0..dim)
            .map(|_| {
                state = state
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                ((state >> 33) as f32 / (1u64 << 31) as f32) - 1.0
            })
            .collect();
        let norm = values.iter().map(|v| v * v).sum::<f32>().sqrt();
        for v in &mut values {
            *v /= norm;
        }
        values
    }

    #[test]
    fn a_vector_is_still_itself_after_quantisation() {
        // The claim int8 storage rests on: the corpus is 4 MB smaller and the
        // similarity it is there to compute is unchanged to three decimals.
        for seed in 1..=32u64 {
            let original = pseudo_unit_vector(seed, ENCODER_DIM as usize);
            let (quantised, scale) = quantise(&original);
            let (reference, reference_scale) = quantise(&original);
            let self_cosine = cosine_i8(&quantised, scale, &reference, reference_scale);
            assert!(
                self_cosine > 0.99,
                "self cosine {self_cosine} at seed {seed}"
            );

            // …and against the float original, which is the fidelity that
            // actually matters at query time.
            let fidelity = cosine_f32(&original, &dequantise(&quantised, scale));
            assert!(fidelity > 0.999, "fidelity {fidelity} at seed {seed}");
        }
    }

    #[test]
    fn quantisation_uses_the_whole_range_and_round_trips_its_peak() {
        let (values, scale) = quantise(&[0.5, -0.25, 0.0, 0.125]);
        assert_eq!(
            values[0], 127,
            "the largest magnitude must land on the rail"
        );
        assert!((dequantise(&values, scale)[0] - 0.5).abs() < 1e-6);
        assert_eq!(values[2], 0);
        assert!(
            values.iter().all(|&v| v >= -127),
            "the range must stay symmetric"
        );
    }

    #[test]
    fn a_degenerate_vector_does_not_produce_nan() {
        let (values, scale) = quantise(&[0.0, 0.0, 0.0]);
        assert_eq!(
            scale, 1.0,
            "a zero scale would make decoding divide by zero"
        );
        assert_eq!(values, vec![0, 0, 0]);
        assert!(dequantise(&values, scale).iter().all(|v| v.is_finite()));
        // …and it ranks last rather than poisoning a sort with NaN.
        assert_eq!(cosine_i8(&values, scale, &[1, 2, 3], 0.1), 0.0);

        let (nan_values, _) = quantise(&[f32::NAN, 1.0]);
        assert_eq!(nan_values[0], 0);
    }

    #[test]
    fn cosine_is_orientation_not_magnitude() {
        let a = pseudo_unit_vector(7, 64);
        let (qa, sa) = quantise(&a);
        // The same direction at a tenth the magnitude quantises to the same
        // int8s with a tenth the scale — and must score the same.
        let scaled: Vec<f32> = a.iter().map(|v| v * 0.1).collect();
        let (qb, sb) = quantise(&scaled);
        assert!((cosine_i8(&qa, sa, &qb, sb) - 1.0).abs() < 1e-5);

        let opposite: Vec<f32> = a.iter().map(|v| -v).collect();
        let (qc, sc) = quantise(&opposite);
        assert!((cosine_i8(&qa, sa, &qc, sc) + 1.0).abs() < 1e-5);
    }

    #[test]
    fn a_length_mismatch_scores_zero_rather_than_panicking() {
        assert_eq!(cosine_i8(&[1, 2, 3], 1.0, &[1, 2], 1.0), 0.0);
        assert_eq!(cosine_i8(&[], 1.0, &[], 1.0), 0.0);
    }

    #[test]
    fn build_args_parse_both_spellings_and_reject_the_rest() {
        assert_eq!(parse_build_args(&[]).unwrap(), BuildArgs { corpus: None });
        let spaced = ["--corpus".to_string(), "a/b.json".to_string()];
        let equals = ["--corpus=a/b.json".to_string()];
        assert_eq!(
            parse_build_args(&spaced).unwrap().corpus,
            Some(PathBuf::from("a/b.json"))
        );
        assert_eq!(
            parse_build_args(&equals).unwrap().corpus,
            Some(PathBuf::from("a/b.json"))
        );
        assert!(parse_build_args(&["--corpus".to_string()]).is_err());
        assert!(parse_build_args(&["--wat".to_string()]).is_err());
    }

    #[test]
    fn a_missing_corpus_says_where_it_looked() {
        let empty = tempfile::tempdir().unwrap();
        let err = build(empty.path(), &[]).unwrap_err();
        let text = err.to_string();
        assert!(
            text.contains("crates/teksilo-corpus/corpus/index.json"),
            "{text}"
        );
        assert!(text.contains("--corpus"), "{text}");

        let err = build(empty.path(), &["--corpus=/nope/index.json".to_string()]).unwrap_err();
        assert!(err.to_string().contains("/nope/index.json"));
    }

    #[test]
    fn locate_index_walks_up_to_the_repository_root() {
        let root = tempfile::tempdir().unwrap();
        let corpus = root.path().join(CORPUS_INDEX_REL);
        std::fs::create_dir_all(corpus.parent().unwrap()).unwrap();
        std::fs::write(&corpus, "{}").unwrap();
        let deep = root.path().join("crates/teksilo-widgets/src");
        std::fs::create_dir_all(&deep).unwrap();
        assert_eq!(locate_index(&deep), Some(corpus));
        assert_eq!(locate_index(tempfile::tempdir().unwrap().path()), None);
    }

    #[test]
    fn the_text_encoded_is_the_index_files_own_text_not_this_binarys_corpus() {
        // The staleness hazard this pass exists to dodge: `cargo-teksilo` was
        // compiled against a corpus one revision older than the `index.json`
        // that `build_corpus.py` has just written, so encoding the embedded
        // copy would attach vectors to text the file no longer contains.
        // `chunk_texts` must read the parsed-from-disk index, verbatim.
        let marker = "# A\nnot-in-the-embedded-corpus";
        let on_disk = serialize_index(&Index {
            schema: 1,
            teksilo_version: "0.0.0".into(),
            encoder: None,
            encoder_dim: None,
            chunk_count: 1,
            avgdl: 1.0,
            terms: vec!["marker".into()],
            df: Default::default(),
            chunks: vec![Chunk {
                id: 0,
                kind: "guide".into(),
                path: "docs/nowhere.md".into(),
                heading_path: vec!["A".into()],
                text: marker.into(),
                line_start: 0,
                line_end: 1,
                crate_name: None,
                tokens: Default::default(),
                len: 1,
                embedding: None,
            }],
        })
        .unwrap();
        let index: Index = serde_json::from_str(&on_disk).unwrap();
        assert_eq!(chunk_texts(&index), vec![marker.to_string()]);

        // …and that is genuinely a different source from the compiled-in one.
        let embedded = teksilo_corpus::index().unwrap();
        assert!(
            embedded
                .chunks
                .iter()
                .all(|c| c.text() != index.chunks[0].text()),
            "the marker text must not exist in the embedded corpus, or this \
             test proves nothing about which of the two was read"
        );
    }

    #[test]
    fn the_serializer_matches_build_corpus_pys_output_shape() {
        // Compact separators, no ASCII escaping, exactly one trailing newline.
        // Any of those drifting would reformat the whole file on the first
        // `build-vectors` run.
        let index = Index {
            schema: 1,
            teksilo_version: "0.12.1".into(),
            encoder: None,
            encoder_dim: None,
            chunk_count: 0,
            avgdl: 142.7,
            terms: vec!["café".into()],
            df: Default::default(),
            chunks: vec![],
        };
        let text = serialize_index(&index).unwrap();
        assert!(text.ends_with("}\n") && !text.ends_with("}\n\n"));
        assert!(text.contains("\"schema\":1,"), "compact separators: {text}");
        assert!(
            text.contains("café"),
            "non-ASCII must not be escaped: {text}"
        );
        assert!(text.contains("\"avgdl\":142.7,"), "{text}");
    }

    #[test]
    fn first_difference_finds_the_offset_or_the_truncation() {
        assert_eq!(first_difference("abc", "abc"), None);
        assert_eq!(first_difference("abc", "abd"), Some(2));
        assert_eq!(first_difference("abc", "ab"), Some(2));
        assert_eq!(first_difference("ab", "abc"), Some(2));
    }

    #[test]
    fn the_committed_corpus_index_survives_a_serde_round_trip() {
        // The precondition `build-vectors` refuses to run without: the Rust
        // schema reproduces `tools/build_corpus.py`'s bytes exactly. Skipped
        // outside a checkout, where the sibling crate's corpus is not on disk.
        let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let Some(path) = locate_index(repo) else {
            return;
        };
        let original = std::fs::read_to_string(&path).unwrap();
        let index: Index = serde_json::from_str(&original).expect("committed index parses");
        let reserialized = serialize_index(&index).unwrap();
        assert_eq!(
            first_difference(&original, &reserialized),
            None,
            "the committed index.json does not round-trip through the Rust schema"
        );
    }
}
