//! The little bit of MP4 the app needs.
//!
//! Yandex serves lossless as FLAC inside an MP4 container. Pulling that stream
//! out into a real `.flac` is a demux, not a re-encode: the audio frames are
//! already FLAC frames and only the container has to be taken apart. Doing it
//! here keeps the app free of an external binary that a user would have to
//! download and put on `PATH`.
//!
//! Only what is needed is parsed: the audio track's sample table, its sample
//! entry (which tells FLAC from AAC), and the FLAC metadata block that carries
//! STREAMINFO. Everything else is skipped by size.

use anyhow::{bail, Context, Result};
use std::io::Read;
use std::path::Path;

/// What a track's audio is, read from its sample entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Codec {
    Flac,
    Aac,
    Other(String),
}

impl Codec {
    pub fn name(&self) -> &str {
        match self {
            Codec::Flac => "FLAC",
            Codec::Aac => "AAC",
            Codec::Other(name) => name,
        }
    }
}

/// A box header: what it is and where its body starts.
struct Atom {
    kind: [u8; 4],
    body: u64,
    end: u64,
}

fn read_atom(data: &[u8], offset: u64) -> Result<Atom> {
    let start = offset as usize;
    if start + 8 > data.len() {
        bail!("файл обрывается на смещении {offset}");
    }
    let size32 = u32::from_be_bytes(data[start..start + 4].try_into().unwrap()) as u64;
    let mut kind = [0u8; 4];
    kind.copy_from_slice(&data[start + 4..start + 8]);
    let (body, end) = match size32 {
        // 1 means a 64-bit size follows the type; 0 means "to the end of file".
        1 => {
            if start + 16 > data.len() {
                bail!("заголовок бокса обрывается на смещении {offset}");
            }
            let large = u64::from_be_bytes(data[start + 8..start + 16].try_into().unwrap());
            (offset + 16, offset + large)
        }
        0 => (offset + 8, data.len() as u64),
        size => (offset + 8, offset + size),
    };
    // `end == body` is an empty box, which is how padding is written; only a
    // box that ends before its own header is broken.
    if end > data.len() as u64 || end < body {
        bail!("бокс {} выходит за пределы файла", String::from_utf8_lossy(&kind));
    }
    Ok(Atom { kind, body, end })
}

/// Every child box of the range `[from, to)`.
fn children(data: &[u8], from: u64, to: u64) -> Result<Vec<Atom>> {
    let mut result = Vec::new();
    let mut cursor = from;
    while cursor + 8 <= to {
        let atom = read_atom(data, cursor)?;
        if atom.end > to {
            break;
        }
        cursor = atom.end;
        result.push(atom);
    }
    Ok(result)
}

fn find<'a>(atoms: &'a [Atom], kind: &[u8; 4]) -> Option<&'a Atom> {
    atoms.iter().find(|atom| &atom.kind == kind)
}

/// Walks `moov/trak/mdia/minf/stbl` of the first audio track.
fn audio_stbl(data: &[u8], moov: &Atom) -> Result<Vec<Atom>> {
    for trak in children(data, moov.body, moov.end)? {
        if &trak.kind != b"trak" {
            continue;
        }
        let Ok(trak_children) = children(data, trak.body, trak.end) else {
            continue;
        };
        let Some(mdia) = find(&trak_children, b"mdia") else {
            continue;
        };
        let Ok(mdia_children) = children(data, mdia.body, mdia.end) else {
            continue;
        };
        // `hdlr` says whether this track is sound, video or subtitles; the first
        // track in the file is often a cover image.
        let is_sound = find(&mdia_children, b"hdlr")
            .and_then(|hdlr| {
                let at = hdlr.body as usize + 8;
                data.get(at..at + 4)
            })
            .map(|handler| handler == b"soun")
            .unwrap_or(false);
        if !is_sound {
            continue;
        }
        let Some(minf) = find(&mdia_children, b"minf") else {
            continue;
        };
        let minf_children = children(data, minf.body, minf.end)?;
        let Some(stbl) = find(&minf_children, b"stbl") else {
            continue;
        };
        return children(data, stbl.body, stbl.end);
    }
    bail!("в файле нет звуковой дорожки MP4")
}

/// Which codec the audio track holds.
pub fn codec(data: &[u8]) -> Result<Codec> {
    let moov = children(data, 0, data.len() as u64)?
        .into_iter()
        .find(|atom| &atom.kind == b"moov")
        .context("в файле нет бокса moov")?;
    let stbl = audio_stbl(data, &moov)?;
    let stsd = find(&stbl, b"stsd").context("нет таблицы описаний сэмплов")?;
    // FullBox header (version + flags), then the entry count, then the entries.
    let entry = stsd.body as usize + 8;
    if entry + 8 > data.len() {
        bail!("таблица описаний сэмплов обрывается");
    }
    let fourcc = &data[entry + 4..entry + 8];
    Ok(match fourcc {
        b"fLaC" => Codec::Flac,
        b"mp4a" => Codec::Aac,
        other => Codec::Other(String::from_utf8_lossy(other).to_string()),
    })
}

/// Reads the codec of a file on disk, or `None` when it is not MP4 at all.
pub fn codec_of_file(path: &Path) -> Option<Codec> {
    let head = std::fs::read(path).ok()?;
    codec(&head).ok()
}

/// The FLAC metadata block that holds STREAMINFO, taken from the sample entry.
fn streaminfo_block(data: &[u8], stsd_entry: usize) -> Option<Vec<u8>> {
    // The sample entry is a box of its own; its children start after the audio
    // sample entry fields (8 bytes of reserved/index + 20 bytes of audio data).
    let entry_size = u32::from_be_bytes(data.get(stsd_entry..stsd_entry + 4)?.try_into().ok()?) as usize;
    let children_start = stsd_entry + 8 + 28;
    let children_end = (stsd_entry + entry_size).min(data.len());
    if children_start >= children_end {
        return None;
    }
    let atoms = children(data, children_start as u64, children_end as u64).ok()?;
    let dfla = find(&atoms, b"dfLa")?;
    // `dfLa` is a FullBox: four bytes of version and flags, then metadata blocks.
    let mut cursor = dfla.body as usize + 4;
    while cursor + 4 <= dfla.end as usize {
        let header = data[cursor];
        // A FLAC metadata header is one byte of flags and a 24-bit length, so the
        // fourth byte has to be supplied: `try_into::<[u8; 4]>` on a 3-byte slice
        // fails, which silently turned every real block into a synthetic one.
        let length = u32::from_be_bytes([0, data[cursor + 1], data[cursor + 2], data[cursor + 3]]) as usize;
        let is_last = header & 0x80 != 0;
        let block_type = header & 0x7f;
        if block_type == 0 {
            // STREAMINFO is 34 bytes of payload; its four-byte header is kept so
            // the block can be written into the new file unchanged.
            return data.get(cursor..cursor + 4 + length).map(|slice| slice.to_vec());
        }
        if is_last {
            break;
        }
        cursor += 4 + length;
    }
    None
}

/// A STREAMINFO block built from what the container already tells us.
///
/// Some muxers leave `dfLa` out. The frame data is still FLAC, and a decoder
/// only needs the sample rate, channel count and bit depth to read it, so the
/// block is filled in rather than failing the whole extraction.
fn synthetic_streaminfo(rate: u32, channels: u32, bits: u32, total_samples: u64) -> Vec<u8> {
    let mut block = Vec::with_capacity(38);
    block.extend_from_slice(&[0x00, 0x00, 0x00, 34]); // type 0, not last, length 34
    block.extend_from_slice(&[0x10, 0x00]); // min block size 4096
    block.extend_from_slice(&[0x10, 0x00]); // max block size 4096
    block.extend_from_slice(&[0x00, 0x00, 0x00]); // min frame size: unknown
    block.extend_from_slice(&[0x00, 0x00, 0x00]); // max frame size: unknown
    // 20 bits of sample rate, 3 bits of channels-1, 5 bits of bits-1, then the
    // 36-bit sample count — packed the way the FLAC format demands.
    let rate = rate & 0xF_FFFF;
    let channels = (channels.max(1) - 1) & 0x7;
    let bits = (bits.max(1) - 1) & 0x1F;
    let total = total_samples & 0xF_FFFF_FFFF;
    let packed = ((rate as u64) << 44) | ((channels as u64) << 41) | ((bits as u64) << 36) | total;
    block.extend_from_slice(&packed.to_be_bytes());
    block.extend_from_slice(&[0; 16]); // MD5 of the raw audio: unknown
    block
}

/// Room reserved after the metadata for later tag edits, in bytes.
const PADDING: usize = 8192;

/// The byte range of every sample in the audio track, in order.
fn sample_offsets(data: &[u8], stbl: &[Atom]) -> Result<Vec<(u64, u64)>> {
    let stsz = find(stbl, b"stsz").context("нет таблицы размеров сэмплов")?;
    let size_at = stsz.body as usize + 4;
    let fixed_size = u32::from_be_bytes(
        data.get(size_at..size_at + 4)
            .context("stsz обрывается")?
            .try_into()
            .unwrap(),
    );
    let count = u32::from_be_bytes(
        data.get(size_at + 4..size_at + 8)
            .context("stsz обрывается")?
            .try_into()
            .unwrap(),
    ) as usize;
    // A count read from a damaged header would otherwise ask for gigabytes: every
    // sample is at least one byte, so a count past the file length is corrupt.
    if count > data.len() {
        bail!("stsz объявляет {count} сэмплов — больше, чем весь файл");
    }

    let sizes: Vec<u64> = if fixed_size > 0 {
        vec![fixed_size as u64; count]
    } else {
        let start = size_at + 8;
        let end = start + count * 4;
        let bytes = data.get(start..end).context("stsz обрывается")?;
        bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|chunk| u32::from_be_bytes(*chunk) as u64)
            .collect()
    };

    // Chunk offsets: `stco` for 32-bit files, `co64` for large ones.
    let offsets: Vec<u64> = if let Some(stco) = find(stbl, b"stco") {
        let start = stco.body as usize + 4;
        let count = u32::from_be_bytes(
            data.get(start..start + 4).context("stco обрывается")?.try_into().unwrap(),
        ) as usize;
        let bytes = data
            .get(start + 4..start + 4 + count * 4)
            .context("stco обрывается")?;
        bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|chunk| u32::from_be_bytes(*chunk) as u64)
            .collect()
    } else if let Some(co64) = find(stbl, b"co64") {
        let start = co64.body as usize + 4;
        let count = u32::from_be_bytes(
            data.get(start..start + 4).context("co64 обрывается")?.try_into().unwrap(),
        ) as usize;
        let bytes = data
            .get(start + 4..start + 4 + count * 8)
            .context("co64 обрывается")?;
        bytes
            .as_chunks::<8>()
            .0
            .iter()
            .map(|chunk| u64::from_be_bytes(*chunk))
            .collect()
    } else {
        bail!("нет таблицы смещений чанков")
    };

    // Which samples belong to which chunk, as (first_sample, samples_per_chunk).
    let stsc = find(stbl, b"stsc").context("нет таблицы сэмпл-в-чанк")?;
    let start = stsc.body as usize + 4;
    let entries = u32::from_be_bytes(
        data.get(start..start + 4).context("stsc обрывается")?.try_into().unwrap(),
    ) as usize;
    let bytes = data
        .get(start + 4..start + 4 + entries * 12)
        .context("stsc обрывается")?;
    let mut runs: Vec<(usize, usize)> = bytes
        .as_chunks::<12>()
        .0
        .iter()
        .map(|chunk| {
            (
                u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) as usize,
                u32::from_be_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]) as usize,
            )
        })
        .collect();
    runs.sort_by_key(|(first_chunk, _)| *first_chunk);

    let mut result = Vec::with_capacity(sizes.len());
    let mut sample = 0usize;
    for (chunk_index, chunk_offset) in offsets.iter().enumerate() {
        // The run whose first chunk is the last one at or before this chunk.
        let per_chunk = runs
            .iter()
            .rev()
            .find(|(first, _)| *first <= chunk_index + 1)
            .map(|(_, per_chunk)| *per_chunk)
            .unwrap_or(0);
        let mut cursor = *chunk_offset;
        for _ in 0..per_chunk {
            let Some(size) = sizes.get(sample) else { break };
            result.push((cursor, *size));
            cursor += size;
            sample += 1;
        }
    }
    if result.is_empty() {
        bail!("в таблице сэмплов нет данных");
    }
    // Chunk tables that disagree with the size table produce a stream that looks
    // fine and is missing its tail. Refusing is better than handing back audio
    // that stops halfway through the song.
    if result.len() != sizes.len() {
        bail!(
            "таблицы сэмплов не совпадают: {} размеров против {} смещений",
            sizes.len(),
            result.len()
        );
    }
    Ok(result)
}

/// Puts the chunk offsets of a re-written MP4 back onto its audio.
///
/// A tag write can grow `moov`, which pushes `mdat` down the file. The tagger
/// patches the `stco`/`co64` tables in most cases, but not always — a real
/// 37 MB file came back with every offset still pointing at the old position,
/// which decodes to noise and fails the playback check, so the tags were simply
/// dropped.
///
/// The repair compares the two tables index by index: an entry that still holds
/// its old value is unpatched and gets moved by the shift; an entry the tagger
/// already moved is left alone. That makes it exact, and safe to run always —
/// when the tagger did its job this function changes nothing.
pub fn repair_chunk_offsets(before: &Path, after: &Path) -> Result<()> {
    let old = std::fs::read(before)
        .with_context(|| format!("не удалось прочитать {}", before.display()))?;
    let mut new = std::fs::read(after)
        .with_context(|| format!("не удалось прочитать {}", after.display()))?;

    // Only MP4s have chunk offset tables. Anything else — a `.flac`, an MP3 —
    // reaches this function too, and parsing it as boxes would fail the tag
    // write for no reason.
    if !looks_like_mp4(&old) || !looks_like_mp4(&new) {
        return Ok(());
    }
    let Some((old_start, _)) = mdat_range(&old)? else {
        return Ok(());
    };
    let Some((new_start, _)) = mdat_range(&new)? else {
        return Ok(());
    };
    let shift = new_start as i64 - old_start as i64;
    if shift == 0 {
        return Ok(());
    }

    let before_tables = chunk_offset_tables(&old)?;
    let after_tables = chunk_offset_tables(&new)?;
    // Tagging never adds or removes chunks, so a mismatch means this is not a
    // plain re-write and guessing would do more harm than the playback check.
    if before_tables.len() != after_tables.len() {
        return Ok(());
    }

    let mut changed = false;
    for (old_at, new_at) in before_tables.iter().zip(&after_tables) {
        let (old_values, old_wide) = table_values(&old, *old_at)?;
        let (new_values, new_wide) = table_values(&new, *new_at)?;
        if old_wide != new_wide || old_values.len() != new_values.len() {
            continue;
        }
        for (index, (was, now)) in old_values.iter().zip(&new_values).enumerate() {
            // Already patched by the tagger, or pointing outside the audio.
            if was != now || *was < old_start {
                continue;
            }
            let moved = *was as i64 + shift;
            let moved = u64::try_from(moved).context("смещение чанка ушло в минус")?;
            let at = new_at + 16 + index * if new_wide { 8 } else { 4 };
            if new_wide {
                new[at..at + 8].copy_from_slice(&moved.to_be_bytes());
            } else {
                let narrow = u32::try_from(moved)
                    .context("смещение чанка не влезает в 32 бита")?;
                new[at..at + 4].copy_from_slice(&narrow.to_be_bytes());
            }
            changed = true;
        }
    }
    if changed {
        std::fs::write(after, &new)
            .with_context(|| format!("не удалось записать {}", after.display()))?;
    }
    Ok(())
}

/// True when the bytes open with a top-level MP4 box.
fn looks_like_mp4(data: &[u8]) -> bool {
    matches!(
        data.get(4..8),
        Some(b"ftyp") | Some(b"moov") | Some(b"free") | Some(b"skip") | Some(b"mdat") | Some(b"wide")
    )
}

/// First byte and end of the `mdat` payload, if the file has one.
fn mdat_range(data: &[u8]) -> Result<Option<(u64, u64)>> {
    let top = children(data, 0, data.len() as u64)?;
    Ok(find(&top, b"mdat").map(|atom| (atom.body, atom.end)))
}

/// Absolute start offsets of every `stco`/`co64` box in the file.
fn chunk_offset_tables(data: &[u8]) -> Result<Vec<usize>> {
    let mut found = Vec::new();
    let mut stack = vec![(0u64, data.len() as u64)];
    while let Some((from, to)) = stack.pop() {
        for atom in children(data, from, to)? {
            if &atom.kind == b"stco" || &atom.kind == b"co64" {
                found.push(atom.body as usize - 8);
            } else if matches!(&atom.kind, b"moov" | b"trak" | b"mdia" | b"minf" | b"stbl") {
                stack.push((atom.body, atom.end));
            }
        }
    }
    // The walk pops a stack, so restore document order: tables are matched to
    // their counterpart in the other file by position.
    found.sort_unstable();
    Ok(found)
}

/// Every entry of one `stco`/`co64` box, and whether it is the 64-bit flavour.
fn table_values(data: &[u8], at: usize) -> Result<(Vec<u64>, bool)> {
    let count = u32::from_be_bytes(
        data.get(at + 12..at + 16)
            .context("таблица смещений обрывается")?
            .try_into()
            .unwrap(),
    ) as usize;
    let wide = data.get(at + 4..at + 8) == Some(b"co64");
    let width = if wide { 8 } else { 4 };
    let mut values = Vec::with_capacity(count.min(data.len() / width));
    for index in 0..count {
        let from = at + 16 + index * width;
        let Some(bytes) = data.get(from..from + width) else { break };
        values.push(if wide {
            u64::from_be_bytes(bytes.try_into().unwrap())
        } else {
            u32::from_be_bytes(bytes.try_into().unwrap()) as u64
        });
    }
    Ok((values, wide))
}

/// Writes the FLAC stream of an MP4 file out as a native `.flac` file.
///
/// Returns the number of bytes written. The audio is copied frame by frame, so
/// the result is bit-identical to what the service sent.
pub fn extract_flac(source: &Path, target: &Path) -> Result<u64> {
    let mut data = Vec::new();
    std::fs::File::open(source)
        .with_context(|| format!("не удалось открыть {}", source.display()))?
        .read_to_end(&mut data)
        .with_context(|| format!("не удалось прочитать {}", source.display()))?;

    let moov = children(&data, 0, data.len() as u64)?
        .into_iter()
        .find(|atom| &atom.kind == b"moov")
        .context("в файле нет бокса moov")?;
    let stbl = audio_stbl(&data, &moov)?;

    let stsd = find(&stbl, b"stsd").context("нет таблицы описаний сэмплов")?;
    let entry = stsd.body as usize + 8;
    let streaminfo = streaminfo_block(&data, entry);

    let samples = sample_offsets(&data, &stbl)?;

    // A FLAC file is the magic, one STREAMINFO block, then the frames.
    // Capped at the file length: a corrupt size field must not turn into a
    // multi-gigabyte reservation.
    let audio_len = samples
        .iter()
        .map(|(_, size)| *size as usize)
        .sum::<usize>()
        .min(data.len());
    let mut out: Vec<u8> = Vec::with_capacity(audio_len + 64);
    out.extend_from_slice(b"fLaC");
    match streaminfo {
        // The block came out of `dfLa` flagged "last" because it was alone there.
        // It is not last here: a padding block follows, which is what gives a tag
        // writer room to add a comment without shifting every audio frame.
        Some(mut block) => {
            if let Some(first) = block.first_mut() {
                *first &= 0x7f;
            }
            out.extend_from_slice(&block);
        }
        None => {
            // No `dfLa`: the rate and depth come from the tags instead.
            let meta = crate::audio::read_file_metadata(source).unwrap_or_default();
            let rate = if meta.sample_rate > 0 { meta.sample_rate } else { 44_100 };
            let bits = if meta.bits_per_sample > 0 { meta.bits_per_sample } else { 16 };
            let channels = if meta.channels > 0 { meta.channels } else { 2 };
            let total = meta.duration_ms * rate as u64 / 1000;
            out.extend_from_slice(&synthetic_streaminfo(rate, channels, bits, total));
        }
    }

    // The same padding the reference file carries, so the result lines up with it
    // and a later tag write is an in-place edit.
    out.extend_from_slice(&[0x81, 0x00, 0x20, 0x00]); // PADDING, last, 8192 bytes
    out.extend_from_slice(&[0u8; PADDING]);

    for (offset, size) in samples {
        let start = offset as usize;
        let end = start + size as usize;
        let bytes = data
            .get(start..end)
            .with_context(|| format!("сэмпл на смещении {offset} выходит за файл"))?;
        out.extend_from_slice(bytes);
    }

    // Written beside the target first: a half-written `.flac` must never replace
    // a good file.
    let part = crate::audio::part_path(target);
    std::fs::write(&part, &out)?;
    std::fs::rename(&part, target)?;
    Ok(out.len() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a box with a four-byte type and a body.
    fn atom(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = ((body.len() + 8) as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(body);
        out
    }

    /// A minimal MP4 with one sound track holding a FLAC sample entry.
    fn tiny_mp4(fourcc: &[u8; 4], sample: &[u8]) -> Vec<u8> {
        let mut stsd_entry = atom(fourcc, &{
            let mut body = vec![0u8; 28];
            // `dfLa` with a STREAMINFO block: type 0, last, 34 bytes of payload.
            let mut streaminfo = vec![0x80, 0x00, 0x00, 34];
            streaminfo.extend_from_slice(&[0x10, 0x00, 0x10, 0x00]);
            streaminfo.extend_from_slice(&[0; 6]);
            streaminfo.extend_from_slice(&0u64.to_be_bytes()[2..]);
            streaminfo.extend_from_slice(&[0; 16]);
            let dfla = atom(b"dfLa", &{
                let mut full = vec![0u8; 4];
                full.extend_from_slice(&streaminfo);
                full
            });
            body.extend_from_slice(&dfla);
            body
        });
        stsd_entry.splice(0..4, ((stsd_entry.len()) as u32).to_be_bytes());

        let mut stsd_body = vec![0u8; 4]; // version + flags
        stsd_body.extend_from_slice(&1u32.to_be_bytes());
        stsd_body.extend_from_slice(&stsd_entry);

        let stsz = atom(b"stsz", &{
            let mut body = Vec::new();
            body.extend_from_slice(&0u32.to_be_bytes()); // per-sample sizes
            body.extend_from_slice(&1u32.to_be_bytes()); // one sample
            body.extend_from_slice(&(sample.len() as u32).to_be_bytes());
            body
        });
        let stsc = atom(b"stsc", &{
            let mut body = Vec::new();
            body.extend_from_slice(&1u32.to_be_bytes());
            body.extend_from_slice(&1u32.to_be_bytes()); // first chunk
            body.extend_from_slice(&1u32.to_be_bytes()); // samples per chunk
            body.extend_from_slice(&1u32.to_be_bytes()); // sample description index
            body
        });
        // The sample sits right after `ftyp` and `moov` in this fixture, so the
        // chunk offset is patched once the sizes are known.
        let mut stco = atom(b"stco", &{
            let mut body = Vec::new();
            body.extend_from_slice(&0u32.to_be_bytes()); // version + flags
            body.extend_from_slice(&1u32.to_be_bytes()); // one chunk
            body.extend_from_slice(&0u32.to_be_bytes()); // offset, patched below
            body
        });

        let build = |stco: &Vec<u8>| {
            let mut stbl_body = Vec::new();
            stbl_body.extend_from_slice(&atom(b"stsd", &stsd_body));
            stbl_body.extend_from_slice(&stsz);
            stbl_body.extend_from_slice(&stsc);
            stbl_body.extend_from_slice(stco);
            let stbl = atom(b"stbl", &stbl_body);
            let minf = atom(b"minf", &stbl);
            let mut hdlr_body = vec![0u8; 8];
            hdlr_body.extend_from_slice(b"soun");
            let hdlr = atom(b"hdlr", &hdlr_body);
            let mut mdia_body = Vec::new();
            mdia_body.extend_from_slice(&hdlr);
            mdia_body.extend_from_slice(&minf);
            let mdia = atom(b"mdia", &mdia_body);
            let trak = atom(b"trak", &mdia);
            let moov = atom(b"moov", &trak);
            let ftyp = atom(b"ftyp", b"isom\0\0\0\0");
            let mut file = ftyp;
            file.extend_from_slice(&moov);
            file
        };

        let mut file = build(&stco);
        let data_offset = file.len() as u32 + 8;
        stco = atom(b"stco", &{
            let mut body = Vec::new();
            body.extend_from_slice(&0u32.to_be_bytes()); // version + flags
            body.extend_from_slice(&1u32.to_be_bytes()); // one chunk
            body.extend_from_slice(&data_offset.to_be_bytes());
            body
        });
        file = build(&stco);
        file.extend_from_slice(&atom(b"mdat", sample));
        file
    }

    #[test]
    fn the_codec_comes_from_the_sample_entry() {
        assert_eq!(codec(&tiny_mp4(b"fLaC", b"xx")).unwrap(), Codec::Flac);
        assert_eq!(codec(&tiny_mp4(b"mp4a", b"xx")).unwrap(), Codec::Aac);
    }

    /// The whole point of a demuxer: what comes out has to be what went in.
    ///
    /// The reference is a stream extracted from the same container by lossless
    /// copy, committed beside the fixture so the comparison runs in CI. Everything
    /// the two share must match byte for byte: the STREAMINFO block and every
    /// audio frame. The reference also carries a vendor comment naming its
    /// producer and an 8 KB padding block, which are container furniture rather
    /// than audio, so they are skipped rather than copied — reproducing them would
    /// add 8 KB to every downloaded track.
    #[test]
    fn the_stream_matches_the_reference_byte_for_byte() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("mp4-reference-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("flac-in-mp4.m4a");
        let reference = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("reference-flac-stream.flac");
        let target = dir.join("ours.flac");

        extract_flac(&source, &target).expect("FLAC должен извлечься");
        let ours = std::fs::read(&target).unwrap();
        let theirs = std::fs::read(&reference).unwrap();

        let (our_info, our_audio) = split_flac(&ours);
        let (their_info, their_audio) = split_flac(&theirs);

        // The whole STREAMINFO block, header included: the rate, the channel
        // count, the bit depth, the sample total and the MD5 of the raw audio all
        // have to be what the container said they were.
        assert_eq!(
            our_info, their_info,
            "STREAMINFO отличается: извлечённый поток описывает себя иначе, чем эталон"
        );
        assert_eq!(
            our_audio.len(),
            their_audio.len(),
            "длина аудиоданных не совпадает: {} против {}",
            our_audio.len(),
            their_audio.len()
        );
        assert!(
            our_audio == their_audio,
            "аудиокадры отличаются от эталонного потока"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Splits a FLAC file into its STREAMINFO payload and the audio that follows
    /// every metadata block.
    fn split_flac(bytes: &[u8]) -> (Vec<u8>, Vec<u8>) {
        assert_eq!(&bytes[0..4], b"fLaC", "не FLAC");
        let mut cursor = 4;
        let mut info = Vec::new();
        loop {
            let header = bytes[cursor];
            let length =
                u32::from_be_bytes([0, bytes[cursor + 1], bytes[cursor + 2], bytes[cursor + 3]])
                    as usize;
            if header & 0x7f == 0 {
                info = bytes[cursor..cursor + 4 + length].to_vec();
            }
            cursor += 4 + length;
            if header & 0x80 != 0 {
                break;
            }
        }
        (info, bytes[cursor..].to_vec())
    }

    /// A box that claims more bytes than the file holds must be rejected rather
    /// than read out of bounds.
    #[test]
    fn a_truncated_file_is_refused() {
        let mut file = tiny_mp4(b"fLaC", b"x");
        file.truncate(20);
        assert!(codec(&file).is_err());
    }
}

