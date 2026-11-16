//! [`OutputSink`] on macOS, through CoreAudio's default output unit (ADR 0016, ADR 0017).

use std::fmt;

use aeri_core::StreamSpec;
use coreaudio::audio_unit::audio_format::LinearPcmFlags;
use coreaudio::audio_unit::macos_helpers::{
    audio_unit_from_device_id, get_available_sample_rates, get_default_device_id, get_device_name,
    set_device_sample_rate,
};
use coreaudio::audio_unit::render_callback::{Args, data};
use coreaudio::audio_unit::{AudioUnit, Element, SampleFormat, Scope, StreamFormat};

use crate::bridge::{Writer, device_bridge};
use crate::{OutputSink, SinkError};

/// Plays through the system's default output device.
///
/// [`configure`](OutputSink::configure) switches the device to the requested sample rate
/// when it offers it; otherwise CoreAudio converts (ADR 0017). It never takes the device
/// exclusively, so other apps keep playing.
#[derive(Debug, Default)]
pub struct CoreAudioSink {
    stream: Option<Stream>,
}

impl CoreAudioSink {
    /// A sink with no output open; [`configure`](OutputSink::configure) opens it.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn stream(&mut self) -> Result<&mut Stream, SinkError> {
        self.stream.as_mut().ok_or(SinkError::NotConfigured)
    }
}

struct Stream {
    writer: Writer<rtrb::Producer<f32>>,
    _unit: AudioUnit,
}

impl fmt::Debug for Stream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Stream")
            .field("writer", &self.writer)
            .finish_non_exhaustive()
    }
}

impl OutputSink for CoreAudioSink {
    fn configure(&mut self, requested: StreamSpec) -> Result<StreamSpec, SinkError> {
        self.stream = None;
        let device = get_default_device_id(false).ok_or(SinkError::NoDevice)?;
        let rate = requested.sample_rate.get();
        if let Err(error) = set_device_sample_rate(device, f64::from(rate)) {
            report_kept_rate(device, rate, error);
        }

        let refused = |error: coreaudio::Error| SinkError::FormatRefused {
            requested,
            source: Box::new(error),
        };
        let mut unit = audio_unit_from_device_id(device, false).map_err(refused)?;
        unit.set_stream_format(
            StreamFormat {
                sample_rate: f64::from(rate),
                sample_format: SampleFormat::F32,
                flags: LinearPcmFlags::IS_FLOAT | LinearPcmFlags::IS_PACKED,
                channels: u32::from(requested.channels.get()),
            },
            Scope::Input,
            Element::Output,
        )
        .map_err(refused)?;

        let (writer, mut renderer) = device_bridge(requested);
        unit.set_render_callback(move |args: Args<data::Interleaved<f32>>| {
            renderer.render(args.data.buffer);
            Ok(())
        })
        .map_err(refused)?;
        unit.start().map_err(refused)?;

        self.stream = Some(Stream {
            writer,
            _unit: unit,
        });
        Ok(requested)
    }

    fn write(&mut self, interleaved: &[f32]) -> Result<usize, SinkError> {
        let writer = &mut self.stream()?.writer;
        if let Some(underruns) = writer.take_underruns() {
            tracing::warn!(underruns = underruns.get(), "audio output ran dry");
        }
        Ok(writer.write(interleaved))
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

/// Logs why the device stayed at its own rate (ADR 0017). A rate the device does not offer
/// is routine, so it is `info`; a rate it offers but would not switch to is a `warn`.
fn report_kept_rate(device: u32, rate: u32, error: coreaudio::Error) {
    let name = get_device_name(device).unwrap_or_else(|_| "the output device".to_owned());
    let ranges = get_available_sample_rates(device).unwrap_or_default();
    let offered = ranges
        .iter()
        .map(|range| {
            let (low, high) = (
                format!("{:.0}", range.mMinimum),
                format!("{:.0}", range.mMaximum),
            );
            if low == high {
                low
            } else {
                format!("{low}-{high}")
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    let wanted = f64::from(rate);
    if ranges
        .iter()
        .any(|range| (range.mMinimum..=range.mMaximum).contains(&wanted))
    {
        tracing::warn!(
            device = %name, offered, rate, %error,
            "the device offers the stream's rate but did not switch to it; CoreAudio converts"
        );
    } else {
        tracing::info!(
            device = %name, offered, rate,
            "the device does not offer the stream's rate; CoreAudio converts"
        );
    }
}
