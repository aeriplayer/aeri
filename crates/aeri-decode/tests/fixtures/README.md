# aeri-decode fixtures

One second of a 440 Hz sine, stereo, 44.1 kHz, peaking near 0.088: FFmpeg's `sine` source has amplitude 1/8, and the mono-to-stereo upmix scales it by 1/√2. Generated with FFmpeg 9.0.2; the `bitexact` flags and `-map_metadata -1` keep the output byte-identical across runs.

```bash
ffmpeg -f lavfi -i "sine=frequency=440:sample_rate=44100:duration=1" -ac 2 \
  -map_metadata -1 -fflags +bitexact -flags:a +bitexact -c:a flac sine.flac

ffmpeg -f lavfi -i "sine=frequency=440:sample_rate=44100:duration=1" -ac 2 \
  -map_metadata -1 -fflags +bitexact -flags:a +bitexact -c:a libmp3lame -b:a 128k sine.mp3

ffmpeg -f lavfi -i "sine=frequency=440:sample_rate=48000:duration=1" -ac 2 \
  -map_metadata -1 -fflags +bitexact -flags:a +bitexact -c:a flac sine-48k.flac

ffmpeg -f lavfi -i "sine=frequency=440:sample_rate=44100:duration=1" -ac 2 \
  -map_metadata -1 -fflags +bitexact -flags:a +bitexact \
  -metadata REPLAYGAIN_TRACK_GAIN="-6.54 dB" -metadata REPLAYGAIN_TRACK_PEAK=0.088000 \
  -metadata REPLAYGAIN_ALBUM_GAIN="-3.21 dB" -metadata REPLAYGAIN_ALBUM_PEAK=0.500000 \
  -c:a flac sine-rg.flac

ffmpeg -f lavfi -i "sine=frequency=440:sample_rate=44100:duration=1" -ac 2 \
  -map_metadata -1 -fflags +bitexact -flags:a +bitexact \
  -metadata REPLAYGAIN_TRACK_GAIN="-6.54 dB" -metadata REPLAYGAIN_TRACK_PEAK=0.088000 \
  -metadata REPLAYGAIN_ALBUM_GAIN="-3.21 dB" -metadata REPLAYGAIN_ALBUM_PEAK=0.500000 \
  -c:a libmp3lame -b:a 128k sine-rg.mp3

ffmpeg -f lavfi -i "sine=frequency=440:sample_rate=44100:duration=1" -ac 2 \
  -map_metadata -1 -fflags +bitexact -flags:a +bitexact \
  -metadata REPLAYGAIN_TRACK_GAIN="-6.54 dB" -metadata REPLAYGAIN_TRACK_PEAK=loud \
  -metadata REPLAYGAIN_ALBUM_GAIN="NaN dB" -metadata REPLAYGAIN_ALBUM_PEAK=-0.5 \
  -c:a flac sine-rg-bad.flac

printf 'This file is text, not FLAC.\n' > not-audio.flac
```

`sine.mp3` starts with an empty ID3v2 tag and carries a LAME header with the encoder delay and padding, as most MP3 files in the wild do. `not-audio.flac` checks that a misleading extension does not make a non-audio file decode. `sine-48k.flac` is the same tone at 48 kHz, for tests of a change of stream spec between tracks.

`sine-rg.flac` and `sine-rg.mp3` carry the same four ReplayGain values, as Vorbis comments and as ID3v2.4 `TXXX` frames. `sine-rg-bad.flac` has one good value (the track gain) and three that must be dropped: an unparsable peak, a `NaN` gain, and a negative peak.
