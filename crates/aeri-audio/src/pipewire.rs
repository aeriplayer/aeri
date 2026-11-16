//! [`OutputSink`] on Linux, through a PipeWire stream (ADR 0019).

use std::fmt;
use std::io::{self, Cursor};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use aeri_core::StreamSpec;
use pipewire as pw;
use pw::spa;
use pw::stream::{StreamFlags, StreamState};

use crate::bridge::{Renderer, RingReader, Writer, device_bridge};
use crate::{OutputSink, SinkError};

/// How long [`configure`](OutputSink::configure) waits for the stream to start.
const STARTUP: Duration = Duration::from_secs(2);

const SAMPLE_BYTES: usize = size_of::<f32>();

/// Linked by the session manager, mapped into memory, processed on the real-time thread.
/// Never `EXCLUSIVE` or `NO_CONVERT`: other apps keep their audio (ADR 0019).
const FLAGS: StreamFlags = StreamFlags::AUTOCONNECT
    .union(StreamFlags::MAP_BUFFERS)
    .union(StreamFlags::RT_PROCESS);

/// Plays through PipeWire's default output.
///
/// [`configure`](OutputSink::configure) asks the graph for the stream's sample rate; when
/// the graph runs at another rate, PipeWire converts (ADR 0019). It never takes the device
/// exclusively, so other apps keep playing.
#[derive(Debug, Default)]
pub struct PipeWireSink {
    stream: Option<Stream>,
}

impl PipeWireSink {
    /// A sink with no output open; [`configure`](OutputSink::configure) opens it.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn stream(&mut self) -> Result<&mut Stream, SinkError> {
        self.stream.as_mut().ok_or(SinkError::NotConfigured)
    }
}

/// What the loop thread tells the sink.
enum Event {
    /// The stream has negotiated its format and is running.
    Streaming,
    /// The stream reported an error; the loop has stopped.
    Failed(String),
    /// The loop could not be set up or torn down.
    Unreachable(pw::Error),
}

/// An open stream: the bridge's writer, and the thread that runs the stream.
struct Stream {
    writer: Writer<rtrb::Producer<f32>>,
    events: mpsc::Receiver<Event>,
    lost: Option<String>,
    stop: pw::channel::Sender<()>,
    thread: Option<JoinHandle<()>>,
}

impl fmt::Debug for Stream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Stream")
            .field("writer", &self.writer)
            .field("lost", &self.lost)
            .finish_non_exhaustive()
    }
}

impl Stream {
    /// The reason the stream stopped, once it has.
    fn lost(&mut self) -> Option<&str> {
        if self.lost.is_none() {
            self.lost = match self.events.try_recv() {
                Ok(Event::Streaming) | Err(mpsc::TryRecvError::Empty) => None,
                Ok(Event::Failed(message)) => Some(message),
                Ok(Event::Unreachable(error)) => Some(error.to_string()),
                Err(mpsc::TryRecvError::Disconnected) => Some("the stream's thread ended".into()),
            };
        }
        self.lost.as_deref()
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        // Fails only when the loop has already stopped.
        let _ = self.stop.send(());
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            tracing::error!("the PipeWire stream's thread panicked");
        }
    }
}

impl OutputSink for PipeWireSink {
    fn configure(&mut self, requested: StreamSpec) -> Result<StreamSpec, SinkError> {
        self.stream = None;
        let refused = |source: Box<dyn std::error::Error + Send + Sync>| SinkError::FormatRefused {
            requested,
            source,
        };
        let format = format_pod(requested).map_err(refused)?;
        let (writer, renderer) = device_bridge(requested);
        let (events, events_rx) = mpsc::channel();
        let (stop, stop_rx) = pw::channel::channel();
        let thread = thread::Builder::new()
            .name("aeri-pipewire".into())
            .spawn(move || {
                if let Err(error) = run(requested, &format, renderer, &events, stop_rx) {
                    let _ = events.send(Event::Unreachable(error));
                }
            })
            .map_err(|error| refused(Box::new(error)))?;

        let stream = Stream {
            writer,
            events: events_rx,
            lost: None,
            stop,
            thread: Some(thread),
        };
        match stream.events.recv_timeout(STARTUP) {
            Ok(Event::Streaming) => {
                self.stream = Some(stream);
                Ok(requested)
            }
            Ok(Event::Failed(message)) => Err(refused(Box::new(io::Error::other(message)))),
            Ok(Event::Unreachable(error)) => {
                tracing::warn!(%error, "could not reach the PipeWire server");
                Err(SinkError::NoDevice)
            }
            Err(error) => {
                tracing::warn!(%error, "the PipeWire stream did not start");
                Err(SinkError::NoDevice)
            }
        }
    }

    fn write(&mut self, interleaved: &[f32]) -> Result<usize, SinkError> {
        let stream = self.stream()?;
        if let Some(message) = stream.lost() {
            return Err(SinkError::DeviceLost {
                source: Box::new(io::Error::other(message.to_owned())),
            });
        }
        if let Some(underruns) = stream.writer.take_underruns() {
            tracing::warn!(underruns = underruns.get(), "audio output ran dry");
        }
        Ok(stream.writer.write(interleaved))
    }

    fn set_paused(&mut self, paused: bool) {
        if let Some(stream) = &self.stream {
            stream.writer.set_paused(paused);
        }
    }

    fn flush(&mut self) {
        if let Some(stream) = &mut self.stream {
            stream.writer.flush();
        }
    }

    fn frames_played(&self) -> u64 {
        self.stream
            .as_ref()
            .map_or(0, |stream| stream.writer.frames_played())
    }

    fn is_drained(&self) -> bool {
        self.stream
            .as_ref()
            .is_none_or(|stream| stream.writer.is_drained())
    }
}

/// The stream's only format: interleaved `F32LE` at the requested rate and channel count.
fn format_pod(spec: StreamSpec) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    let channels = u32::from(spec.channels.get());
    if channels > spa::sys::SPA_AUDIO_MAX_CHANNELS {
        return Err(format!(
            "PipeWire carries at most {} channels",
            spa::sys::SPA_AUDIO_MAX_CHANNELS
        )
        .into());
    }
    let mut info = spa::param::audio::AudioInfoRaw::new();
    info.set_format(spa::param::audio::AudioFormat::F32LE);
    info.set_rate(spec.sample_rate.get());
    info.set_channels(channels);
    let mut position = [0; spa::param::audio::MAX_CHANNELS];
    match channels {
        1 => position[0] = spa::sys::SPA_AUDIO_CHANNEL_MONO,
        2 => {
            position[0] = spa::sys::SPA_AUDIO_CHANNEL_FL;
            position[1] = spa::sys::SPA_AUDIO_CHANNEL_FR;
        }
        _ => {}
    }
    info.set_position(position);
    let (bytes, _) = spa::pod::serialize::PodSerializer::serialize(
        Cursor::new(Vec::new()),
        &spa::pod::Value::Object(spa::pod::Object {
            type_: spa::sys::SPA_TYPE_OBJECT_Format,
            id: spa::sys::SPA_PARAM_EnumFormat,
            properties: info.into(),
        }),
    )?;
    Ok(bytes.into_inner())
}

/// The loop thread: opens the stream, then runs PipeWire's loop until told to stop.
fn run(
    spec: StreamSpec,
    format: &[u8],
    renderer: Renderer<rtrb::Consumer<f32>>,
    events: &mpsc::Sender<Event>,
    stop: pw::channel::Receiver<()>,
) -> Result<(), pw::Error> {
    pw::init();
    let main_loop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&main_loop, None)?;
    let core = context.connect_rc(None)?;
    let rate = spec.sample_rate.get();
    let stream = pw::stream::StreamBox::new(
        &core,
        "aeri",
        pw::properties::properties! {
            *pw::keys::MEDIA_TYPE => "Audio",
            *pw::keys::MEDIA_CATEGORY => "Playback",
            *pw::keys::MEDIA_ROLE => "Music",
            *pw::keys::NODE_RATE => format!("1/{rate}"),
        },
    )?;

    let channels = usize::from(spec.channels.get());
    let stride = frame_bytes(spec);
    let quit = main_loop.downgrade();
    let mut startup = Startup::new(events.clone(), move || {
        if let Some(main_loop) = quit.upgrade() {
            main_loop.quit();
        }
    });
    let _listener = stream
        .add_local_listener_with_user_data(renderer)
        .state_changed(move |stream, _, _, new| {
            startup.on_state(new, || report_rate(stream, rate));
        })
        .process(move |stream, renderer| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let requested = buffer.requested();
            let Some(data) = buffer.datas_mut().first_mut() else {
                return;
            };
            let size = data
                .data()
                .map_or(0, |bytes| fill(renderer, bytes, requested, channels));
            let chunk = data.chunk_mut();
            *chunk.offset_mut() = 0;
            *chunk.stride_mut() = stride;
            *chunk.size_mut() = size;
        })
        .register()?;

    let mut params = [spa::pod::Pod::from_bytes(format).ok_or(pw::Error::CreationFailed)?];
    stream.connect(spa::utils::Direction::Output, None, FLAGS, &mut params)?;

    serve(&main_loop, stop);
    stream.disconnect()
}

/// Runs the loop until the sink sends the stop message.
fn serve(main_loop: &pw::main_loop::MainLoopRc, stop: pw::channel::Receiver<()>) {
    let stopper = main_loop.clone();
    let _stop = stop.attach(main_loop.loop_(), move |()| stopper.quit());
    main_loop.run();
}

/// How the loop thread reacts to the stream's state: it tells the sink once streaming
/// starts, and on an error tells it why and stops the loop.
struct Startup<Q> {
    events: mpsc::Sender<Event>,
    quit: Q,
    started: bool,
}

impl<Q: FnMut()> Startup<Q> {
    fn new(events: mpsc::Sender<Event>, quit: Q) -> Self {
        Self {
            events,
            quit,
            started: false,
        }
    }

    /// Handles a new state; `report` runs once, when streaming first starts.
    fn on_state(&mut self, state: StreamState, report: impl FnOnce()) {
        // A send fails only once the sink is gone, and then nobody is listening.
        match state {
            StreamState::Streaming if !self.started => {
                self.started = true;
                report();
                let _ = self.events.send(Event::Streaming);
            }
            StreamState::Error(message) => {
                let _ = self.events.send(Event::Failed(message));
                (self.quit)();
            }
            _ => {}
        }
    }
}

/// Logs whether PipeWire resamples: the graph's rate is the server's choice (ADR 0019).
fn report_rate(stream: &pw::stream::Stream, rate: u32) {
    match stream.time() {
        Ok(time) => {
            let graph = time.rate();
            if resamples(graph, rate) {
                tracing::info!(
                    rate,
                    graph_rate = graph.denom,
                    "the PipeWire graph runs at another rate; PipeWire converts"
                );
            } else {
                tracing::debug!(rate, "the PipeWire graph runs at the stream's rate");
            }
        }
        Err(error) => tracing::debug!(%error, "could not read the PipeWire graph's rate"),
    }
}

/// Whether a graph clocked at `graph` (a fraction of a second per tick) converts a
/// `rate` Hz stream.
fn resamples(graph: spa::utils::Fraction, rate: u32) -> bool {
    graph.num != 1 || graph.denom != rate
}

/// The size of one interleaved frame of `spec`, as PipeWire's chunk stride.
fn frame_bytes(spec: StreamSpec) -> i32 {
    i32::try_from(usize::from(spec.channels.get()) * SAMPLE_BYTES).unwrap_or(i32::MAX)
}

/// The callback body: renders `requested` frames (all that fit, if 0) into the front of
/// `bytes` and returns how many bytes it wrote.
///
/// Real-time safe: no allocation, locking, blocking, or panicking (brief, Step 4). One
/// render per buffer keeps ADR 0016's guarantee that no buffer mixes old and new audio.
fn fill<R: RingReader>(
    renderer: &mut Renderer<R>,
    bytes: &mut [u8],
    requested: u64,
    channels: usize,
) -> u32 {
    let samples = as_samples(bytes);
    let fits = samples.len() / channels;
    let frames = match usize::try_from(requested) {
        Ok(0) | Err(_) => fits,
        Ok(requested) => requested.min(fits),
    };
    let Some(out) = samples.get_mut(..frames * channels) else {
        return 0;
    };
    renderer.render(out);
    u32::try_from(out.len() * SAMPLE_BYTES).unwrap_or(0)
}

/// Views PipeWire's byte buffer as samples, or as nothing if it is not aligned for `f32`.
#[expect(
    unsafe_code,
    reason = "PipeWire hands out audio buffers as bytes (ADR 0019)"
)]
fn as_samples(bytes: &mut [u8]) -> &mut [f32] {
    // SAFETY: every bit pattern is a valid `f32`, and `align_to_mut` yields only aligned,
    // in-bounds elements of the initialised bytes it is given.
    let (head, samples, _) = unsafe { bytes.align_to_mut::<f32>() };
    if head.is_empty() { samples } else { &mut [] }
}

#[cfg(test)]
#[cfg(not(loom))]
mod tests {
    use aeri_core::{Channels, SampleRate};

    use super::*;
    use crate::alloc_guard::assert_no_alloc;
    use crate::bridge::bridge;

    fn spec(channels: u16) -> StreamSpec {
        StreamSpec {
            sample_rate: SampleRate::new(48_000).expect("non-zero"),
            channels: Channels::new(channels).expect("non-zero"),
        }
    }

    /// A sink as `configure` leaves it once streaming, with the loop thread's side of
    /// its channels and the callback's renderer handed to the test instead.
    fn streaming_sink() -> (
        PipeWireSink,
        mpsc::Sender<Event>,
        Renderer<rtrb::Consumer<f32>>,
    ) {
        let (writer, renderer) = device_bridge(spec(2));
        let (events, events_rx) = mpsc::channel();
        let (stop, _) = pw::channel::channel();
        let stream = Stream {
            writer,
            events: events_rx,
            lost: None,
            stop,
            thread: None,
        };
        (
            PipeWireSink {
                stream: Some(stream),
            },
            events,
            renderer,
        )
    }

    fn assert_lost(result: Result<usize, SinkError>, reason: &str) {
        match result {
            Err(SinkError::DeviceLost { source }) => assert_eq!(source.to_string(), reason),
            other => panic!("expected DeviceLost({reason}), got {other:?}"),
        }
    }

    /// `frames` stereo frames of distinct samples, already in the ring.
    fn stereo_ring(frames: u16) -> Renderer<rtrb::Consumer<f32>> {
        let (mut writer, renderer) = bridge(rtrb::RingBuffer::new(64), 2);
        let samples: Vec<f32> = (1..=frames * 2).map(f32::from).collect();
        assert_eq!(writer.write(&samples), usize::from(frames));
        renderer
    }

    /// A zeroed, `f32`-aligned window of `len` bytes into `storage`.
    fn aligned(storage: &mut [u8], len: usize) -> &mut [u8] {
        let start = storage.as_ptr().align_offset(SAMPLE_BYTES);
        &mut storage[start..start + len]
    }

    fn samples_of(bytes: &[u8]) -> Vec<f32> {
        let (samples, _) = bytes.as_chunks::<SAMPLE_BYTES>();
        samples.iter().copied().map(f32::from_ne_bytes).collect()
    }

    #[test]
    fn renders_only_the_frames_pipewire_asked_for() {
        let mut renderer = stereo_ring(4);
        let mut storage = [0_u8; 64 + SAMPLE_BYTES];
        let bytes = aligned(&mut storage, 64);

        let written = assert_no_alloc(|| fill(&mut renderer, bytes, 3, 2));

        assert_eq!(written, 24);
        assert_eq!(samples_of(&bytes[..24]), [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        assert!(bytes[24..].iter().all(|&byte| byte == 0));
    }

    #[test]
    fn with_no_request_fills_every_whole_frame_that_fits() {
        let mut renderer = stereo_ring(4);
        let mut storage = [0_u8; 36 + SAMPLE_BYTES];
        let bytes = aligned(&mut storage, 36);

        let written = assert_no_alloc(|| fill(&mut renderer, bytes, 0, 2));

        assert_eq!(written, 32, "the ninth sample would split a frame");
        assert_eq!(
            samples_of(&bytes[..32]),
            [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]
        );
    }

    #[test]
    fn a_request_larger_than_the_buffer_is_clamped_to_it() {
        let mut renderer = stereo_ring(4);
        let mut storage = [0_u8; 16 + SAMPLE_BYTES];
        let bytes = aligned(&mut storage, 16);

        let written = assert_no_alloc(|| fill(&mut renderer, bytes, 1_000, 2));

        assert_eq!(written, 16);
        assert_eq!(samples_of(bytes), [1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn a_misaligned_buffer_gets_nothing_and_consumes_nothing() {
        let mut renderer = stereo_ring(2);
        let mut storage = [0_u8; 32 + SAMPLE_BYTES];
        let start = storage.as_ptr().align_offset(SAMPLE_BYTES) + 1;
        let bytes = &mut storage[start..start + 16];

        assert_eq!(assert_no_alloc(|| fill(&mut renderer, bytes, 2, 2)), 0);

        let mut out = [0.0; 4];
        renderer.render(&mut out);
        assert_eq!(out, [1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn an_unconfigured_sink_refuses_writes_and_has_nothing_to_play() {
        let mut sink = PipeWireSink::new();

        assert!(matches!(
            sink.write(&[0.0; 2]),
            Err(SinkError::NotConfigured)
        ));
        assert_eq!(sink.frames_played(), 0);
        assert!(sink.is_drained());
    }

    #[test]
    #[cfg_attr(miri, ignore = "pipewire's channel opens a pipe")]
    fn a_streaming_sink_plays_pauses_and_flushes_through_its_bridge() {
        let (mut sink, _events, mut renderer) = streaming_sink();
        let mut out = [0.0; 4];

        assert_eq!(
            sink.write(&[1.0, 2.0, 3.0, 4.0, 5.0]).expect("streaming"),
            2
        );
        assert!(!sink.is_drained());
        sink.set_paused(true);
        renderer.render(&mut out);
        assert_eq!((out, sink.frames_played()), ([0.0; 4], 0));

        sink.set_paused(false);
        renderer.render(&mut out);
        assert_eq!((out, sink.frames_played()), ([1.0, 2.0, 3.0, 4.0], 2));
        assert!(sink.is_drained());

        assert_eq!(sink.write(&[6.0, 7.0]).expect("streaming"), 1);
        sink.flush();
        renderer.render(&mut out);
        assert_eq!((out, sink.frames_played()), ([0.0; 4], 0));
    }

    #[test]
    #[cfg_attr(miri, ignore = "pipewire's channel opens a pipe")]
    fn a_stream_error_after_starting_loses_the_device_for_good() {
        let (mut sink, events, _renderer) = streaming_sink();
        events.send(Event::Streaming).expect("the sink listens");
        assert_eq!(sink.write(&[0.0; 2]).expect("still streaming"), 1);

        events
            .send(Event::Failed("node was destroyed".into()))
            .expect("the sink listens");

        assert_lost(sink.write(&[0.0; 2]), "node was destroyed");
        assert_lost(sink.write(&[0.0; 2]), "node was destroyed");
    }

    #[test]
    #[cfg_attr(miri, ignore = "pipewire's channel opens a pipe")]
    fn a_loop_thread_that_ends_loses_the_device() {
        let (mut sink, events, _renderer) = streaming_sink();
        drop(events);

        assert_lost(sink.write(&[0.0; 2]), "the stream's thread ended");
    }

    #[test]
    fn the_ring_holds_a_fifth_of_a_second() {
        let (mut writer, _renderer) = device_bridge(spec(2));

        assert_eq!(writer.write(&vec![0.0; 2 * 10_000]), 9_600);
    }

    #[test]
    fn startup_tells_the_sink_once_when_streaming_first_starts() {
        let (events, events_rx) = mpsc::channel();
        let mut quits = 0;
        let mut reports = 0;
        let mut startup = Startup::new(events, || quits += 1);

        startup.on_state(StreamState::Paused, || reports += 1);
        assert!(events_rx.try_recv().is_err(), "paused is not started");
        startup.on_state(StreamState::Streaming, || reports += 1);
        startup.on_state(StreamState::Paused, || reports += 1);
        startup.on_state(StreamState::Streaming, || reports += 1);
        drop(startup);

        assert!(matches!(events_rx.try_recv(), Ok(Event::Streaming)));
        assert!(
            events_rx.try_recv().is_err(),
            "a restart is not a second start"
        );
        assert_eq!((reports, quits), (1, 0));
    }

    #[test]
    fn a_stream_error_is_passed_on_and_stops_the_loop() {
        let (events, events_rx) = mpsc::channel();
        let mut quits = 0;
        let mut startup = Startup::new(events, || quits += 1);

        startup.on_state(StreamState::Error("no target node".into()), || {
            panic!("an error is not a start");
        });
        drop(startup);

        assert!(
            matches!(events_rx.try_recv(), Ok(Event::Failed(message)) if message == "no target node")
        );
        assert_eq!(quits, 1);
    }

    #[test]
    fn only_a_graph_ticking_at_the_stream_rate_does_not_resample() {
        let per_tick = |num, denom| spa::utils::Fraction { num, denom };

        assert!(!resamples(per_tick(1, 44_100), 44_100));
        assert!(resamples(per_tick(1, 48_000), 44_100));
        assert!(resamples(per_tick(2, 44_100), 44_100));
    }

    #[test]
    #[cfg_attr(miri, ignore = "runs PipeWire's main loop, which is C")]
    fn dropping_a_stream_stops_its_loop_and_joins_the_thread() {
        let (stop, stop_rx) = pw::channel::channel();
        let (ready, ready_rx) = mpsc::channel();
        let (done, done_rx) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("test-pipewire-loop".into())
            .spawn(move || {
                pw::init();
                let main_loop =
                    pw::main_loop::MainLoopRc::new(None).expect("a loop needs no server");
                ready.send(()).expect("the test waits");
                serve(&main_loop, stop_rx);
                done.send(()).expect("the test waits");
            })
            .expect("the thread starts");
        ready_rx.recv().expect("the loop is built");
        assert!(
            done_rx.recv_timeout(Duration::from_millis(50)).is_err(),
            "the loop runs until it is stopped"
        );

        let (writer, _renderer) = device_bridge(spec(2));
        let (_events, events_rx) = mpsc::channel();
        drop(Stream {
            writer,
            events: events_rx,
            lost: None,
            stop,
            thread: Some(thread),
        });

        assert!(
            done_rx.try_recv().is_ok(),
            "drop returned before the loop ended"
        );
    }

    #[test]
    #[cfg_attr(miri, ignore = "pipewire's channel opens a pipe")]
    fn a_sink_shows_its_stream_state_when_debugged() {
        let (sink, _events, _renderer) = streaming_sink();

        let shown = format!("{sink:?}");

        assert!(shown.contains("writer: Writer"), "{shown}");
        assert!(shown.contains("lost: None"), "{shown}");
    }

    #[test]
    fn a_chunk_stride_is_one_frame_of_samples() {
        assert_eq!(frame_bytes(spec(1)), 4);
        assert_eq!(frame_bytes(spec(6)), 24);
    }

    #[test]
    #[cfg_attr(miri, ignore = "parsing the format calls into libspa's C code")]
    fn the_format_names_mono_and_stereo_positions_and_refuses_too_many_channels() {
        let parse = |bytes: &[u8]| {
            let mut info = spa::param::audio::AudioInfoRaw::new();
            info.parse(spa::pod::Pod::from_bytes(bytes).expect("a pod"))
                .expect("an audio format");
            info
        };

        let mono = parse(&format_pod(spec(1)).expect("mono serialises"));
        assert_eq!(mono.format(), spa::param::audio::AudioFormat::F32LE);
        assert_eq!((mono.rate(), mono.channels()), (48_000, 1));
        assert_eq!(mono.position()[0], spa::sys::SPA_AUDIO_CHANNEL_MONO);

        let stereo = parse(&format_pod(spec(2)).expect("stereo serialises"));
        assert_eq!(
            stereo.position()[..2],
            [
                spa::sys::SPA_AUDIO_CHANNEL_FL,
                spa::sys::SPA_AUDIO_CHANNEL_FR
            ]
        );

        assert_eq!(
            parse(&format_pod(spec(64)).expect("64 channels fit")).channels(),
            64
        );
        assert!(format_pod(spec(65)).is_err());
    }
}
