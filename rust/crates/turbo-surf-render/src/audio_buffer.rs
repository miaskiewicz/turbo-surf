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
pub(crate) fn render(node: &Node, rate: f32, length: usize, parameters: &[Values]) -> Buffer {
    let count = node.data.len().clamp(1, 2);
    let mut result = Buffer {
        channels: vec![vec![0.; length]; count],
        active: vec![false; length.div_ceil(128)],
        ended: None,
    };
    if !node.started || node.data.is_empty() {
        return result;
    }
    let input_length = node.data[0].len();
    if input_length == 0 {
        return result;
    }
    let input_rate = if node.buffer_rate > 0. {
        node.buffer_rate
    } else {
        rate
    };
    let input_duration = input_length as f64 / input_rate as f64;
    let start = frame(node.start, rate as f64, true);
    let stop = node
        .stop
        .map_or(length, |time| frame(time, rate as f64, true));
    let mut position = if node.parameters.first().copied().unwrap_or(1.) == 1.
        && node.parameters.get(1).copied().unwrap_or(0.) == 0.
    {
        frame(node.offset.min(input_duration), input_rate as f64, false) as f64
    } else {
        node.offset.min(input_duration) * input_rate as f64
    };
    let mut loop_start = 0.0;
    let mut loop_end = input_length as f64;
    let start_seconds = node.loop_start.clamp(0., input_duration);
    let end_seconds = if node.loop_end == 0. {
        input_duration
    } else {
        node.loop_end.clamp(0., input_duration)
    };
    if node.looped && start_seconds < end_seconds {
        loop_start = start_seconds * input_rate as f64;
        loop_end = end_seconds * input_rate as f64;
    }
    let loop_width = loop_end - loop_start;
    let mut played = 0.0;
    let duration = node
        .duration
        .map_or(f64::INFINITY, |duration| duration * input_rate as f64);
    let mut active = false;
    for output in start..stop.min(length) {
        let quantum = output / 128 * 128;
        let speed = parameters
            .first()
            .map_or(1., |value| value.samples[quantum]) as f64;
        let detune = parameters.get(1).map_or(0., |value| value.samples[quantum]);
        let speed = (input_rate as f64 / rate as f64
            * speed
            * audio_timeline::power(2., (detune / 1200.) as f64))
        .clamp(-1024., 1024.);
        if !active {
            if node.looped && speed >= 0. && position >= loop_end {
                position = loop_start.min((input_length - 1) as f64);
            }
            if node.looped && speed < 0. && position < loop_start {
                position = loop_start;
            }
            let skip = ((start as f64 - node.start * rate as f64) * speed).abs();
            position += if speed >= 0. { skip } else { -skip };
            played += skip;
            active = true;
        }
        if played >= duration {
            result.ended = Some(output);
            break;
        }
        if !node.looped
            && ((speed >= 0. && position >= input_length as f64) || (speed < 0. && position < 0.))
        {
            result.ended = Some(output);
            break;
        }
        let first = (position.max(0.) as usize).min(input_length - 1);
        let fraction = position - first as f64;
        let mut second = first + 1;
        if node.looped && (first as f64) < loop_end && second as f64 >= loop_end {
            let wrapped = if loop_width >= 1. {
                position + 1. - loop_width
            } else if loop_width > 0. {
                loop_start + (position + 1. - loop_start) % loop_width
            } else {
                loop_start
            };
            second = wrapped.max(0.) as usize;
        } else if second >= input_length {
            second = first;
        }
        second = second.min(input_length - 1);
        result.active[output / 128] = true;
        for (channel, output_channel) in result.channels.iter_mut().enumerate() {
            let source = &node.data[channel];
            let value = if first == second && first >= 1 {
                source[first] as f64 + (source[first] as f64 - source[first - 1] as f64) * fraction
            } else {
                (1. - fraction) * source[first] as f64 + fraction * source[second] as f64
            };
            output_channel[output] = value as f32;
        }
        position += speed;
        played += speed.abs();
        if node.looped && loop_width > 0. {
            if speed >= 0. && position >= loop_end {
                position = loop_start + (position - loop_end) % loop_width;
            } else if speed < 0. && position < loop_start {
                position = loop_end - (loop_start - position) % loop_width;
            }
        }
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
