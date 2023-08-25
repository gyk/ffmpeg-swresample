use std::path::{Path, PathBuf};
use std::sync::Once;

use anyhow::{anyhow, Result};
use ffmpeg::format::Sample;
use ffmpeg::sys::AV_TIME_BASE;
use ffmpeg::util::channel_layout::ChannelLayout;
use ffmpeg::util::frame::Audio;
use ffmpeg::util::log::level::Level;

static INIT_FFMPEG: Once = Once::new();

fn init_ffmpeg() {
    ffmpeg::init().expect("Unable to initialize FFmpeg");
    ffmpeg::util::log::set_level(Level::Fatal);
}

const DOWNSAMPLE_RATE: u32 = 22050;

pub fn downsample_audio<P: AsRef<Path>>(path: P) -> Result<Vec<i16>> {
    let path = path.as_ref();
    std::panic::catch_unwind(move || downsample_audio_impl(path))
        .map_err(|_| anyhow!("FFmpeg panics when processing audio stream"))?
}

fn downsample_audio_impl(path: &Path) -> Result<Vec<i16>> {
    INIT_FFMPEG.call_once(init_ffmpeg);

    let mut input_ctx = ffmpeg::format::input(&path)?;
    let a_stream = input_ctx
        .streams()
        .best(ffmpeg::media::Type::Audio)
        .ok_or(ffmpeg::Error::StreamNotFound)?;

    let a_index = a_stream.index();

    let decoder_ctx = ffmpeg::codec::Context::from_parameters(a_stream.parameters())?;
    let mut decoder = decoder_ctx.decoder().audio()?;

    let _duration = input_ctx.duration() as f64 / AV_TIME_BASE as f64;

    // When channel layout is 0 (e.g., some WAV files), set it to the default value. See
    // https://stackoverflow.com/q/20001363.
    if decoder.channel_layout().bits() == 0 {
        decoder.set_channel_layout(ffmpeg::ChannelLayout::default(decoder.channels() as i32));
    }

    let mut resampler_ctx = ffmpeg::software::resampling::Context::get(
        decoder.format(),
        decoder.channel_layout(),
        decoder.rate(),
        Sample::I16(ffmpeg::format::sample::Type::Planar),
        ChannelLayout::MONO,
        DOWNSAMPLE_RATE,
    )?;

    let mut wave_samples: Vec<i16> = Vec::new();

    for packet in input_ctx.packets().filter_map(|(stream, packet)| {
        if stream.index() == a_index {
            Some(packet)
        } else {
            None
        }
    }) {
        let _ = decoder.send_packet(&packet);

        let mut a_frame = Audio::empty();

        if decoder.receive_frame(&mut a_frame).is_ok() {
            debug_assert!(a_frame.is_key());

            let mut downsampled = Audio::empty();

            resampler_ctx.run(&a_frame, &mut downsampled)?;

            let pcm: &[i16] = downsampled.plane(0);
            wave_samples.extend_from_slice(pcm);
        }
    }

    Ok(wave_samples)
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let p = args[1].parse::<PathBuf>()?;
    println!("{}", downsample_audio(p)?.len());
    Ok(())
}
