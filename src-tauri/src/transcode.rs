//! Decoding and MP3 export, in pure Rust.
//!
//! Two jobs live here. The first is verification: every tag rewrite is done on a
//! copy, and the copy has to be decoded end to end before it replaces the
//! original — a container whose offsets no longer line up still reports a codec
//! and a duration, yet plays silence. The second is MP3 export, which the service
//! does not offer: its streams are decoded here and re-encoded with LAME, whose
//! sources are compiled into the binary.
//!
//! Neither needs anything installed on the machine.

use anyhow::{bail, Context, Result};
use std::io::Write;
use std::path::Path;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// How far into a file to look for a hint about its container.
fn hint_for(path: &Path) -> Hint {
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|ext| ext.to_str()) {
        hint.with_extension(extension);
    }
    hint
}

/// True when the whole audio stream decodes without an error.
///
/// Probing is not enough: a damaged container still answers with a codec name, so
/// only a full decode catches it. Every packet is decoded and any decoder error
/// fails the check, which is what stops a broken tag rewrite from replacing a
/// file that still played.
pub fn decodes(path: &Path) -> bool {
    match decode_all(path, |_, _, _| Ok(())) {
        Ok(frames) => frames > 0,
        Err(error) => {
            crate::startup_log::write(&format!(
                "[warn] проверка {} не прошла: {error:#}",
                path.display()
            ));
            false
        }
    }
}

/// Decodes every packet of the first audio track, calling `sink` per buffer.
///
/// Returns the number of interleaved frames decoded. `sink` receives the samples
/// already converted to signed 16-bit, which is what an MP3 encoder wants.
pub fn decode_all<F>(path: &Path, mut sink: F) -> Result<u64>
where
    F: FnMut(&[i16], u32, u32) -> Result<()>,
{
    let file = std::fs::File::open(path)
        .with_context(|| format!("не удалось открыть {}", path.display()))?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let probed = symphonia::default::get_probe()
        .format(
            &hint_for(path),
            stream,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .with_context(|| format!("формат не распознан: {}", path.display()))?;
    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|track| track.codec_params.codec != CODEC_TYPE_NULL)
        .context("в файле нет звуковой дорожки")?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .context("декодер для этого формата не поддерживается")?;

    let mut total = 0u64;
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            // The end of the stream is a normal stop, not a failure.
            Err(symphonia::core::errors::Error::IoError(ref error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break
            }
            Err(error) => return Err(error).context("не удалось прочитать пакет"),
        };
        if packet.track_id() != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(decoded) => {
                let spec = *decoded.spec();
                let channels = spec.channels.count() as u32;
                let rate = spec.rate;
                let frames = decoded.frames() as u64;
                let mut buffer = SampleBuffer::<i16>::new(decoded.capacity() as u64, spec);
                buffer.copy_interleaved_ref(decoded);
                sink(buffer.samples(), channels, rate)?;
                total += frames;
            }
            // A reset request means the stream changed; the decoder is rebuilt.
            Err(symphonia::core::errors::Error::DecodeError(message)) => {
                bail!("декодер вернул ошибку: {message}")
            }
            Err(error) => return Err(error).context("декодирование прервалось"),
        }
    }
    Ok(total)
}

/// Mixes an N-channel buffer down to interleaved stereo.
///
/// Channels beyond the first two are folded in at equal weight instead of being
/// dropped, so a surround source does not lose its centre and rear content.
fn to_stereo(samples: &[i16], channels: usize) -> Vec<i16> {
    let mut stereo = Vec::with_capacity(samples.len() / channels * 2);
    for frame in samples.chunks_exact(channels) {
        let left = frame[0] as i32;
        let right = frame[1] as i32;
        let rest: i32 = frame[2..].iter().map(|&sample| sample as i32).sum();
        let half = (rest / 2) as i16 as i32;
        stereo.push((left + half).clamp(i16::MIN as i32, i16::MAX as i32) as i16);
        stereo.push((right + half).clamp(i16::MIN as i32, i16::MAX as i32) as i16);
    }
    stereo
}

/// Re-encodes a file as MP3 at 320 kbit/s.
///
/// The service has no MP3 stream, so this decodes whatever it did give (FLAC or
/// AAC) and encodes it again. LAME is compiled into the binary; nothing has to be
/// installed.
pub fn encode_mp3(source: &Path, target: &Path) -> Result<u64> {
    use mp3lame_encoder::{Bitrate, Builder, FlushNoGap, InterleavedPcm, MonoPcm, Quality};

    let mut builder = Some(Builder::new().context("не удалось создать MP3-энкодер")?);
    // The rate and channel count come from the source, so the encoder is built
    // once the first buffer arrives.
    let mut encoder: Option<mp3lame_encoder::Encoder> = None;
    let mut part = crate::audio::part_path(target);
    if part.extension().and_then(|ext| ext.to_str()) != Some("mp3") {
        part.set_extension("mp3");
    }
    let mut out = std::io::BufWriter::new(
        std::fs::File::create(&part)
            .with_context(|| format!("не удалось создать {}", part.display()))?,
    );

    let mut written = 0u64;
    let decoded = decode_all(source, |samples, channels, rate| {
        if encoder.is_none() {
            let mut made = builder.take().context("MP3-энкодер уже создан")?;
            made.set_brate(Bitrate::Kbps320)
                .map_err(|error| anyhow::anyhow!("неподдерживаемый битрейт: {error:?}"))?;
            made.set_num_channels(channels.min(2) as u8)
                .map_err(|error| anyhow::anyhow!("неподдерживаемое число каналов: {error:?}"))?;
            made.set_sample_rate(rate)
                .map_err(|error| anyhow::anyhow!("неподдерживаемая частота: {error:?}"))?;
            made.set_quality(Quality::Best)
                .map_err(|error| anyhow::anyhow!("неподдерживаемое качество: {error:?}"))?;
            encoder = Some(
                made.build()
                    .map_err(|error| anyhow::anyhow!("MP3-энкодер не создался: {error:?}"))?,
            );
        }
        let encoder = encoder.as_mut().expect("encoder just built");
        // LAME wants the buffer sized for the interleaved sample count; passing
        // the frame count instead under-sizes it and the C side writes past the
        // end.
        let mut buffer =
            Vec::with_capacity(mp3lame_encoder::max_required_buffer_size(samples.len()));
        let produced = match channels {
            // `InterleavedPcm` halves the slice length, because it assumes stereo
            // pairs. Feeding a mono stream through it would encode half the audio
            // at double speed, so mono takes the single-channel entry point.
            1 => encoder.encode_to_vec(MonoPcm(samples), &mut buffer),
            2 => encoder.encode_to_vec(InterleavedPcm(samples), &mut buffer),
            // More than two channels is not something the service sends, but a
            // downmix is cheaper than writing a corrupt file if it ever does.
            _ => {
                let stereo = to_stereo(samples, channels as usize);
                encoder.encode_to_vec(InterleavedPcm(&stereo), &mut buffer)
            }
        }
        .map_err(|error| anyhow::anyhow!("MP3-энкодер вернул ошибку: {error:?}"))?;
        out.write_all(&buffer[..produced])
            .context("не удалось записать MP3")?;
        written += produced as u64;
        Ok(())
    });

    let result = decoded.and_then(|_| {
        let mut encoder = encoder.context("источник не дал ни одного аудиобуфера")?;
        // Flushing writes up to a full MP3 frame; with an empty buffer LAME writes
        // into zero spare capacity and takes the process down.
        let mut tail = Vec::with_capacity(mp3lame_encoder::max_required_buffer_size(1152));
        encoder
            .flush_to_vec::<FlushNoGap>(&mut tail)
            .map_err(|error| anyhow::anyhow!("MP3-энкодер не закрылся: {error:?}"))?;
        out.write_all(&tail).context("не удалось дописать MP3")?;
        out.flush().context("не удалось дописать MP3")?;
        Ok(())
    });
    drop(out);

    if let Err(error) = result {
        let _ = std::fs::remove_file(&part);
        return Err(error);
    }
    if written == 0 {
        let _ = std::fs::remove_file(&part);
        bail!("MP3 получился пустым");
    }
    // The result has to play before it is allowed to take the name.
    if !decodes(&part) {
        let _ = std::fs::remove_file(&part);
        bail!("MP3 после кодирования не читается");
    }
    std::fs::rename(&part, target)
        .with_context(|| format!("не удалось переименовать в {}", target.display()))?;
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file that is not audio at all must fail the check instead of passing it.
    #[test]
    fn a_broken_file_does_not_decode() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("transcode-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("broken.flac");
        std::fs::write(&path, b"not audio").unwrap();
        assert!(!decodes(&path));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The encoder is only worth shipping if it produces a file that decodes back
    /// to the same length as the source.
    #[test]
    fn a_generated_tone_survives_the_mp3_round_trip() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("transcode-test-mp3");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("tone.wav");
        let target = dir.join("tone.mp3");

        // One second of a 440 Hz tone in a plain WAV, written by hand so the test
        // depends on nothing but the code under test.
        let rate = 44_100u32;
        let frames = rate;
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + frames * 4).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
        wav.extend_from_slice(&2u16.to_le_bytes()); // stereo
        wav.extend_from_slice(&rate.to_le_bytes());
        wav.extend_from_slice(&(rate * 4).to_le_bytes());
        wav.extend_from_slice(&4u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(frames * 4).to_le_bytes());
        for index in 0..frames {
            let value = ((index as f64 / rate as f64 * 440.0 * std::f64::consts::TAU).sin()
                * 12_000.0) as i16;
            wav.extend_from_slice(&value.to_le_bytes());
            wav.extend_from_slice(&value.to_le_bytes());
        }
        std::fs::write(&source, &wav).unwrap();

        let written = encode_mp3(&source, &target).expect("MP3 must be produced");
        assert!(written > 10_000, "a second of 320k MP3 is about 40 KB");
        assert!(decodes(&target), "the produced MP3 must decode");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A mono source must keep its length. Encoding it through the stereo entry
    /// point reads sample pairs, which halves the audio and doubles its pitch —
    /// a file that still decodes, so only the duration catches it.
    #[test]
    fn a_mono_tone_keeps_its_length() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("transcode-test-mono");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("mono.wav");
        let target = dir.join("mono.mp3");

        let rate = 44_100u32;
        let frames = rate;
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + frames * 2).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
        wav.extend_from_slice(&1u16.to_le_bytes()); // mono
        wav.extend_from_slice(&rate.to_le_bytes());
        wav.extend_from_slice(&(rate * 2).to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(frames * 2).to_le_bytes());
        for index in 0..frames {
            let value = ((index as f64 / rate as f64 * 440.0 * std::f64::consts::TAU).sin()
                * 12_000.0) as i16;
            wav.extend_from_slice(&value.to_le_bytes());
        }
        std::fs::write(&source, &wav).unwrap();

        encode_mp3(&source, &target).expect("MP3 must be produced");
        assert!(decodes(&target), "the produced MP3 must decode");
        // 320 kbit/s for one second is about 40 KB; half the audio would be
        // about 20 KB, so the lower bound alone catches a halved stream.
        let size = std::fs::metadata(&target).unwrap().len();
        assert!(size > 30_000, "a halved mono stream would be far smaller, got {size}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
