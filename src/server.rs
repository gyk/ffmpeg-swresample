use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Once;
use std::time::Duration;

use anyhow::{anyhow, Result};
use ffmpeg::format::Sample;
use ffmpeg::sys::AV_TIME_BASE;
use ffmpeg::util::channel_layout::ChannelLayout;
use ffmpeg::util::frame::Audio;
use ffmpeg::util::log::level::Level;

use iceoryx2::{
    port::{listener::Listener, notifier::Notifier, subscriber::Subscriber},
    prelude::*,
    sample::Sample as IpcSample,
};

const HISTORY_SIZE: usize = 20;
const DEADLINE: Duration = Duration::from_secs(10);

mod events;
use events::IpcEvent;

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

    let mut input_ctx = ffmpeg::format::input(path)?;
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
    let mut should_copy_channel_layout = false;
    if decoder.channel_layout().bits() == 0 {
        decoder.set_channel_layout(ffmpeg::ChannelLayout::default(decoder.channels() as i32));
        should_copy_channel_layout = true;
    }

    use ffmpeg::sys::{av_channel_layout_copy, AVChannelLayout, AVChannelOrder};
    let in_ch_layout = AVChannelLayout {
        order: AVChannelOrder::AV_CHANNEL_ORDER_NATIVE,
        nb_channels: decoder.channel_layout().channels(),
        u: unsafe { std::mem::transmute_copy(&decoder.channel_layout().bits()) },
        opaque: std::ptr::null_mut(),
    };

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

            if should_copy_channel_layout {
                // Prevent "Input changed" error after FFmpeg switches to new channel layout API.
                unsafe {
                    av_channel_layout_copy(
                        &mut (*a_frame.as_mut_ptr()).ch_layout as _,
                        &in_ch_layout as _,
                    );
                }
            }

            let mut downsampled = Audio::empty();

            resampler_ctx.run(&a_frame, &mut downsampled)?;

            let pcm: &[i16] = downsampled.plane(0);
            wave_samples.extend_from_slice(pcm);
        }
    }

    Ok(wave_samples)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let node = NodeBuilder::new().create::<ipc::Service>()?;
    let service_name: ServiceName = "Audio Thumbnail Making Service".try_into()?;
    let ipc_server = IpcServer::new(&node, &service_name)?;

    let waitset = WaitSetBuilder::new().create::<ipc::Service>()?;

    let subscriber_guard = waitset.attach_deadline(&ipc_server, DEADLINE)?;

    let on_event = |attachment_id: WaitSetAttachmentId<ipc::Service>| {
        if attachment_id.has_event_from(&subscriber_guard) {
            ipc_server.handle_event().unwrap();
        } else if attachment_id.has_missed_deadline(&subscriber_guard) {
            if ipc_server.n_clients.load(Ordering::SeqCst) == 0 {
                return CallbackProgression::Stop;
            }

            println!(
                "⚠️ The subscriber did not receive a message for {:?}.",
                DEADLINE
            );
        }

        CallbackProgression::Continue
    };

    waitset.wait_and_process(on_event)?;

    println!("exit");
    Ok(())
}

#[derive(Debug)]
struct IpcServer {
    n_clients: AtomicUsize,
    subscriber: Subscriber<ipc::Service, [u8], ()>,
    notifier: Notifier<ipc::Service>,
    listener: Listener<ipc::Service>,
}

impl FileDescriptorBased for IpcServer {
    fn file_descriptor(&self) -> &FileDescriptor {
        self.listener.file_descriptor()
    }
}

impl SynchronousMultiplexing for IpcServer {}

impl IpcServer {
    fn new(
        node: &Node<ipc::Service>,
        service_name: &ServiceName,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let pubsub_service = node
            .service_builder(service_name)
            .publish_subscribe::<[u8]>()
            .history_size(HISTORY_SIZE)
            .subscriber_max_buffer_size(HISTORY_SIZE)
            .open_or_create()?;
        let event_service = node
            .service_builder(service_name)
            .event()
            .open_or_create()?;

        let listener = event_service.listener_builder().create()?;
        let notifier = event_service.notifier_builder().create()?;
        let subscriber = pubsub_service.subscriber_builder().create()?;

        notifier.notify_with_custom_event_id(IpcEvent::ServerConnected.into())?;

        Ok(Self {
            n_clients: AtomicUsize::new(0),
            subscriber,
            listener,
            notifier,
        })
    }

    fn handle_event(&self) -> Result<(), Box<dyn std::error::Error>> {
        while let Some(event) = self.listener.try_wait_one()? {
            let event: IpcEvent = event.into();
            match event {
                IpcEvent::RequestSent => {
                    while let Ok(Some(sample)) = self.receive() {
                        println!("received: len = {}", sample.payload().len());
                        let path_u8_slice: &[u8] = sample.payload();
                        let s = std::str::from_utf8(path_u8_slice)?;
                        let downsampled = downsample_audio(Path::new(s))?;
                        println!("downsampled: len = {}", downsampled.len());
                    }
                }
                IpcEvent::ClientConnected => {
                    println!("new client connected");
                    self.n_clients.fetch_add(1, Ordering::SeqCst);
                }
                IpcEvent::ClientDisconnected => {
                    println!("client disconnected");
                    self.n_clients.fetch_sub(1, Ordering::SeqCst);
                }
                _ => (),
            }
        }

        Ok(())
    }

    fn receive(
        &self,
    ) -> Result<Option<IpcSample<ipc::Service, [u8], ()>>, Box<dyn std::error::Error>> {
        match self.subscriber.receive()? {
            Some(sample) => {
                self.notifier
                    .notify_with_custom_event_id(IpcEvent::RequestReceived.into())?;
                Ok(Some(sample))
            }
            None => Ok(None),
        }
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        self.notifier
            .notify_with_custom_event_id(IpcEvent::ServerDisconnected.into())
            .unwrap();
    }
}
