//! Genuine scheduled buffer playback, using Chrome 155's double interpolation,
//! fractional loop wrapping, k-rate pitch and sample-frame rounding rules.
use crate::{
    audio_dsp::Node,
    audio_timeline::{self, Values},
};
pub(crate) fn frame(time: f64, rate: f64, up: bool) -> usize {
    let precise = (time * rate * 1024.0).round() / 1024.0;
    if up {
        precise.ceil() as usize
    } else {
        precise.round() as usize
    }
}
pub(crate) struct Buffer {
    pub channels: Vec<Vec<f32>>,
    pub active: Vec<bool>,
    pub ended: Option<usize>,
}
/// The playhead state of one scheduled buffer source, in input frames.
struct Playhead {
    looped: bool,
    input_length: usize,
    input_rate: f64,
    loop_start: f64,
    loop_end: f64,
    loop_width: f64,
    position: f64,
    played: f64,
    duration: f64,
}
impl Playhead {
    /// `None` for a source that never sounds (unstarted or an empty buffer).
    fn new(node: &Node, rate: f32) -> Option<Self> {
        if !node.started {
            return None;
        }
        let input_length = node.data.first()?.len();
        if input_length == 0 {
            return None;
        }
        let input_rate = f64::from(if node.buffer_rate > 0. {
            node.buffer_rate
        } else {
            rate
        });
        let input_duration = input_length as f64 / input_rate;
        let (loop_start, loop_end) = loop_frames(node, input_length, input_rate, input_duration);
        Some(Self {
            looped: node.looped,
            input_length,
            input_rate,
            loop_start,
            loop_end,
            loop_width: loop_end - loop_start,
            position: start_position(node, input_rate, input_duration),
            played: 0.0,
            duration: node
                .duration
                .map_or(f64::INFINITY, |duration| duration * input_rate),
        })
    }
    /// Apply the loop entry rules and the sub-frame start skip once playback begins.
    fn begin(&mut self, node: &Node, start: usize, rate: f32, speed: f64) {
        if self.looped && speed >= 0. && self.position >= self.loop_end {
            self.position = self.loop_start.min((self.input_length - 1) as f64);
        }
        if self.looped && speed < 0. && self.position < self.loop_start {
            self.position = self.loop_start;
        }
        let skip = ((start as f64 - node.start * rate as f64) * speed).abs();
        self.position += if speed >= 0. { skip } else { -skip };
        self.played += skip;
    }
    fn ended(&self, speed: f64) -> bool {
        let outside = (speed >= 0. && self.position >= self.input_length as f64)
            || (speed < 0. && self.position < 0.);
        self.played >= self.duration || !self.looped && outside
    }
    /// The two interpolation taps and the fraction between them.
    fn taps(&self) -> (usize, usize, f64) {
        let last = self.input_length - 1;
        let first = (self.position.max(0.) as usize).min(last);
        let fraction = self.position - first as f64;
        let second = first + 1;
        let second =
            if self.looped && (first as f64) < self.loop_end && second as f64 >= self.loop_end {
                self.wrapped_tap().max(0.) as usize
            } else if second >= self.input_length {
                first
            } else {
                second
            };
        (first, second.min(last), fraction)
    }
    fn wrapped_tap(&self) -> f64 {
        if self.loop_width >= 1. {
            self.position + 1. - self.loop_width
        } else if self.loop_width > 0. {
            self.loop_start + (self.position + 1. - self.loop_start) % self.loop_width
        } else {
            self.loop_start
        }
    }
    fn advance(&mut self, speed: f64) {
        self.position += speed;
        self.played += speed.abs();
        if !self.looped || self.loop_width <= 0. {
            return;
        }
        if speed >= 0. && self.position >= self.loop_end {
            self.position = self.loop_start + (self.position - self.loop_end) % self.loop_width;
        } else if speed < 0. && self.position < self.loop_start {
            self.position = self.loop_end - (self.loop_start - self.position) % self.loop_width;
        }
    }
}
/// Loop points in input frames; the whole buffer unless a valid loop is set.
fn loop_frames(
    node: &Node,
    input_length: usize,
    input_rate: f64,
    input_duration: f64,
) -> (f64, f64) {
    let start_seconds = node.loop_start.clamp(0., input_duration);
    let end_seconds = if node.loop_end == 0. {
        input_duration
    } else {
        node.loop_end.clamp(0., input_duration)
    };
    if node.looped && start_seconds < end_seconds {
        (start_seconds * input_rate, end_seconds * input_rate)
    } else {
        (0.0, input_length as f64)
    }
}
/// The offset rounds to a whole frame only at unity playback rate and no detune.
fn start_position(node: &Node, input_rate: f64, input_duration: f64) -> f64 {
    let unity = node.parameters.first().copied().unwrap_or(1.) == 1.
        && node.parameters.get(1).copied().unwrap_or(0.) == 0.;
    if unity {
        frame(node.offset.min(input_duration), input_rate, false) as f64
    } else {
        node.offset.min(input_duration) * input_rate
    }
}
/// The k-rate computed playback speed in input frames per output frame.
fn quantum_speed(parameters: &[Values], quantum: usize, input_rate: f64, rate: f32) -> f64 {
    let speed = parameters
        .first()
        .map_or(1., |value| value.samples[quantum]) as f64;
    let detune = parameters.get(1).map_or(0., |value| value.samples[quantum]);
    (input_rate / rate as f64 * speed * audio_timeline::power(2., (detune / 1200.) as f64))
        .clamp(-1024., 1024.)
}
fn interpolate(source: &[f32], first: usize, second: usize, fraction: f64) -> f32 {
    let value = if first == second && first >= 1 {
        source[first] as f64 + (source[first] as f64 - source[first - 1] as f64) * fraction
    } else {
        (1. - fraction) * source[first] as f64 + fraction * source[second] as f64
    };
    value as f32
}
pub(crate) fn render(node: &Node, rate: f32, length: usize, parameters: &[Values]) -> Buffer {
    let count = node.data.len().clamp(1, 2);
    let mut result = Buffer {
        channels: vec![vec![0.; length]; count],
        active: vec![false; length.div_ceil(128)],
        ended: None,
    };
    let Some(mut playhead) = Playhead::new(node, rate) else {
        return result;
    };
    let start = frame(node.start, rate as f64, true);
    let stop = node
        .stop
        .map_or(length, |time| frame(time, rate as f64, true));
    let mut active = false;
    for output in start..stop.min(length) {
        let speed = quantum_speed(parameters, output / 128 * 128, playhead.input_rate, rate);
        if !active {
            playhead.begin(node, start, rate, speed);
            active = true;
        }
        if playhead.ended(speed) {
            result.ended = Some(output);
            break;
        }
        let (first, second, fraction) = playhead.taps();
        result.active[output / 128] = true;
        for (channel, output_channel) in result.channels.iter_mut().enumerate() {
            output_channel[output] = interpolate(&node.data[channel], first, second, fraction);
        }
        playhead.advance(speed);
    }
    if node.stop.is_some() && stop <= length {
        result.ended = Some(result.ended.map_or(stop, |end| end.min(stop)));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    // An 8 Hz context playing an 8 Hz buffer: one buffer frame per output frame, so
    // loop points in seconds are frame / 8 and outputs are exact sample values.
    const RATE: f32 = 8.;
    fn ramp(looped: bool, loop_start: f64, loop_end: f64, offset: f64) -> Node {
        Node {
            kind: "buffer".into(),
            started: true,
            data: vec![(0..8).map(|v| v as f32).collect()],
            buffer_rate: RATE,
            looped,
            loop_start: loop_start / RATE as f64,
            loop_end: loop_end / RATE as f64,
            offset: offset / RATE as f64,
            ..Default::default()
        }
    }
    fn speed(value: f32, length: usize) -> Vec<Values> {
        vec![Values {
            samples: vec![value; length],
            sample_accurate: vec![false; length],
        }]
    }
    #[test]
    fn an_empty_buffer_plays_silence_without_ending() {
        let mut node = ramp(false, 0., 0., 0.);
        node.data = vec![vec![]];
        let played = render(&node, RATE, 4, &[]);
        assert_eq!(played.channels, vec![vec![0.; 4]]);
        assert_eq!((played.ended, played.active), (None, vec![false]));
    }
    #[test]
    fn looped_playback_wraps_forward_and_backward_inside_the_loop() {
        // AudioBufferSourceNode: an offset past loopEnd starts at loopStart.
        let forward = render(&ramp(true, 2., 5., 6.), RATE, 7, &[]);
        assert_eq!(forward.channels[0], [2., 3., 4., 2., 3., 4., 2.]);
        // Played backwards, an offset before loopStart starts at loopStart and
        // wraps from loopStart to just under loopEnd.
        let backward = render(&ramp(true, 2., 5., 0.), RATE, 7, &speed(-1., 7));
        assert_eq!(backward.channels[0], [2., 4., 3., 2., 4., 3., 2.]);
        assert_eq!(backward.ended, None);
    }
    #[test]
    fn a_sub_frame_loop_holds_its_start_sample() {
        let held = render(&ramp(true, 2., 2.5, 2.), RATE, 4, &[]);
        assert_eq!(held.channels[0], [2.; 4]);
    }
}
