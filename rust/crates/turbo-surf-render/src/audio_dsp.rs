//! Bounded offline graph renderer. Oscillator math follows Chrome 155's
//! BSD-licensed PeriodicWave/OscillatorHandler and ARM oscillator kernel; the
//! compressor uses its unchanged Blink/fdlibm implementation (native/audio).
use crate::{
    audio_fft,
    audio_timeline::{self, Automation, Values},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    ffi::c_void,
    time::{Duration, Instant},
};

const QUANTUM: usize = 128;
const MAX_SAMPLES: usize = 262_144;
const MAX_NODES: usize = 64;
const MAX_WORK: usize = 8_388_608;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Graph {
    pub channels: usize,
    pub length: usize,
    pub sample_rate: f32,
    pub nodes: Vec<Node>,
}
#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Node {
    pub kind: String,
    #[serde(default)]
    pub inputs: Vec<usize>,
    #[serde(default)]
    pub parameters: Vec<f32>,
    #[serde(default)]
    pub shape: String,
    #[serde(default)]
    pub started: bool,
    #[serde(default)]
    pub stop: Option<f64>,
    #[serde(default)]
    pub data: Vec<Vec<f32>>,
    #[serde(default)]
    pub start: f64,
    #[serde(default)]
    pub offset: f64,
    #[serde(default)]
    pub duration: Option<f64>,
    #[serde(default)]
    pub buffer_rate: f32,
    #[serde(default)]
    pub looped: bool,
    #[serde(default)]
    pub loop_start: f64,
    #[serde(default)]
    pub loop_end: f64,
    #[serde(default)]
    pub automation: Vec<Automation>,
    #[serde(default)]
    pub curve: Vec<f32>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Rendered {
    pub channels: Vec<Vec<f32>>,
    pub current_time: f64,
    pub reductions: HashMap<String, f32>,
    pub ends: HashMap<String, usize>,
    pub final_values: HashMap<String, Vec<f32>>,
}

unsafe extern "C" {
    fn surf_audio_compressor_new(rate: f32, parameters: *const f32) -> *mut c_void;
    fn surf_audio_compressor_process(
        handle: *mut c_void,
        left: *const f32,
        right: *const f32,
        out_left: *mut f32,
        out_right: *mut f32,
    );
    fn surf_audio_compressor_parameters(handle: *mut c_void, parameters: *const f32);
    fn surf_audio_compressor_reduction(handle: *mut c_void) -> f32;
    fn surf_audio_compressor_free(handle: *mut c_void);
}
struct Compressor(*mut c_void);
impl Compressor {
    fn new(rate: f32, params: &[f32]) -> Result<Self, String> {
        if params.len() != 5 {
            return Err("Invalid compressor parameters".into());
        }
        // Rust validates the complete five-value slice before the native adapter.
        let handle = unsafe { surf_audio_compressor_new(rate, params.as_ptr()) };
        if handle.is_null() {
            return Err("Offline audio allocation failed".into());
        }
        Ok(Self(handle))
    }
    fn parameters(&mut self, values: &[Values], offset: usize) {
        let parameters: [f32; 5] = std::array::from_fn(|index| values[index].samples[offset]);
        unsafe {
            surf_audio_compressor_parameters(self.0, parameters.as_ptr());
        }
    }
    fn reduction(&self) -> f32 {
        unsafe { surf_audio_compressor_reduction(self.0) }
    }
    fn process(&mut self, channels: &mut [Vec<f32>]) {
        let (left, right) = channels.split_at_mut(1);
        // Each channel has a complete quantum, and native processing is in-place safe.
        unsafe {
            surf_audio_compressor_process(
                self.0,
                left[0].as_ptr(),
                right[0].as_ptr(),
                left[0].as_mut_ptr(),
                right[0].as_mut_ptr(),
            );
        }
    }
}
impl Drop for Compressor {
    fn drop(&mut self) {
        unsafe { surf_audio_compressor_free(self.0) }
    }
}

fn wave_tables(shape: &str, rate: f32) -> Vec<Vec<f32>> {
    let size = if rate <= 24_000. {
        2048
    } else if rate <= 88_200. {
        4096
    } else {
        16384
    };
    let half = size / 2;
    let ranges = (0.5 + 3.0 * (size as f32).log2()) as usize;
    let mut fft = audio_fft::rustfft_new(size);
    let mut base = vec![0.; half];
    for (n, coefficient) in base.iter_mut().enumerate().skip(1) {
        let p = 2.0f32 / (n as f32 * std::f32::consts::PI);
        *coefficient = match shape {
            "sine" => {
                if n == 1 {
                    1.
                } else {
                    0.
                }
            }
            "triangle" => {
                if n & 1 == 1 {
                    2.0 * (p * p) * if ((n - 1) >> 1) & 1 == 1 { -1. } else { 1. }
                } else {
                    0.
                }
            }
            "square" => {
                if n & 1 == 1 {
                    2.0 * p
                } else {
                    0.
                }
            }
            _ => p * if n & 1 == 1 { 1. } else { -1. },
        };
    }
    let mut result = Vec::with_capacity(ranges);
    let mut normalization = 0.5f32;
    for range in 0..ranges {
        let partials =
            (2.0f64.powf((-(range as f32 * 400.0) / 1200.0) as f64) as f32 * half as f32) as usize;
        let real = vec![0.; half];
        let mut imaginary = base.iter().map(|x| -(size as f32) * x).collect::<Vec<_>>();
        imaginary[0] = 0.;
        imaginary[half.min(partials + 1)..].fill(0.);
        let mut data = vec![0.; size];
        fft.do_inverse_fft(&real, &imaginary, &mut data);
        if range == 0 {
            let peak = data.iter().map(|x| x.abs()).fold(0.0f32, f32::max);
            if peak != 0. {
                normalization = 1.0 / peak;
            }
        }
        for value in &mut data {
            *value *= normalization;
        }
        result.push(data);
    }
    result
}
fn oscillator(tables: &[Vec<f32>], rate: f32, frequency: f32, length: usize) -> Vec<f32> {
    let size = tables[0].len();
    let lowest = rate * 0.5 / (size / 2) as f32;
    let pitch = (1.0 + (frequency.abs() / lowest).log2() * 1200.0 / 400.0)
        .clamp(0.0, (tables.len() - 1) as f32);
    let higher = pitch as usize;
    let lower = (higher + 1).min(tables.len() - 1);
    let weight = pitch - higher as f32;
    let scale = size as f32 / rate;
    let increment = frequency * scale;
    // Frequency.value changes are a-rate in the first quantum. Blink's vector
    // reciprocal operations round differently from the later scalar division.
    let first_pitch = (1.0
        + (increment.abs() * (1.0 / scale) * (1.0 / lowest)).log2() * 1200.0 * (1.0 / 400.0f32))
        .clamp(0.0, (tables.len() - 1) as f32);
    let first_higher = first_pitch as usize;
    let first_lower = (first_higher + 1).min(tables.len() - 1);
    let first_weight = first_pitch - first_higher as f32;
    let wrap = |value: f32| value - (value / size as f32).floor() * size as f32;
    let sample = |phase: f32, hi: usize, lo: usize, factor: f32| {
        let index = phase as usize & (size - 1);
        let next = (index + 1) & (size - 1);
        let fraction = phase - index as f32;
        let high = tables[hi][index] + fraction * (tables[hi][next] - tables[hi][index]);
        let low = tables[lo][index] + fraction * (tables[lo][next] - tables[lo][index]);
        high + factor * (low - high)
    };
    let mut result = Vec::with_capacity(length);
    let mut phase = 0.0f64;
    for block in 0..length / QUANTUM {
        if block == 0 {
            for _ in 0..QUANTUM {
                result.push(sample(
                    phase as f32,
                    first_higher,
                    first_lower,
                    first_weight,
                ));
                phase += increment as f64;
                phase -= (phase / size as f64).floor() * size as f64;
            }
        } else {
            let base = phase as f32;
            let mut lanes = [0.0f32; 4];
            for (index, lane) in lanes.iter_mut().enumerate() {
                *lane = wrap(base + index as f32 * increment);
            }
            for _ in 0..QUANTUM / 4 {
                for lane in &mut lanes {
                    result.push(sample(*lane, higher, lower, weight));
                    *lane = wrap(*lane + 4.0 * increment);
                }
            }
            phase += (QUANTUM as f32 * increment) as f64;
            phase -= (phase / size as f64).floor() * size as f64;
        }
    }
    result
}

// Chrome's low-frequency and scalar-tail interpolator preserves double phase,
// while SIMD kernels use their separately rounded Float32 linear interpolation.
fn interpolated_sample(
    phase: f64,
    increment: f32,
    mask: usize,
    weight: f32,
    lower: &[f32],
    higher: &[f32],
) -> f32 {
    let base = phase as usize;
    let mut low = 0.0f64;
    let mut high = 0.0f64;
    if increment >= 0.3 {
        let index = base & mask;
        let next = (base + 1) & mask;
        let fraction = (phase as f32 - index as f32) as f64;
        low = (1. - fraction) * lower[index] as f64 + fraction * lower[next] as f64;
        high = (1. - fraction) * higher[index] as f64 + fraction * higher[next] as f64;
    } else {
        let t = phase - base as f64;
        let t2 = t * t;
        let (coefficients, offset, count): ([f64; 5], isize, usize) = if increment >= 0.16 {
            (
                [0.5 * t * (t - 1.), 1. - t2, 0.5 * t * (t + 1.), 0., 0.],
                -1,
                3,
            )
        } else {
            (
                [
                    t * (t2 - 1.) * (t - 2.) / 24.,
                    -t * (t - 1.) * (t2 - 4.) / 6.,
                    (t2 - 1.) * (t2 - 4.) / 4.,
                    -t * (t + 1.) * (t2 - 4.) / 6.,
                    t * (t2 - 1.) * (t + 2.) / 24.,
                ],
                -2,
                5,
            )
        };
        for (index, coefficient) in coefficients.iter().enumerate().take(count) {
            let position = ((base as isize + offset + index as isize) as usize) & mask;
            low += coefficient * lower[position] as f64;
            high += coefficient * higher[position] as f64;
        }
    }
    ((1.0f32 - weight) as f64 * high + weight as f64 * low) as f32
}
fn oscillator_events(
    tables: &[Vec<f32>],
    rate: f32,
    node: &Node,
    parameters: &[Values],
    length: usize,
) -> Vec<f32> {
    let mut output = vec![0.; length];
    if !node.started {
        return output;
    }
    let size = tables[0].len();
    let scale = size as f32 / rate;
    let lowest = rate * 0.5 / (size / 2) as f32;
    let start = crate::audio_buffer::frame(node.start, rate as f64, true);
    let stop = node
        .stop
        .map_or(length, |time| {
            crate::audio_buffer::frame(time, rate as f64, true)
        })
        .min(length);
    let mut phase = 0.0f64;
    let mut active = false;
    let lookup = |frequency: f32, vector: bool| {
        let pitch = if vector {
            (1. + (frequency.abs() * (1. / lowest)).log2() * 1200. * (1. / 400.0f32))
                .clamp(0., (tables.len() - 1) as f32)
        } else {
            (1. + (frequency.abs() / lowest).log2() * 1200. / 400.)
                .clamp(0., (tables.len() - 1) as f32)
        };
        let high = pitch as usize;
        let low = (high + 1).min(tables.len() - 1);
        (high, low, pitch - high as f32)
    };
    let sample = |position: f32, frequency: f32, vector: bool| {
        let (high, low, weight) = lookup(frequency, vector);
        let index = position as usize & (size - 1);
        let next = (index + 1) & (size - 1);
        let fraction = position - index as f32;
        let higher = tables[high][index] + fraction * (tables[high][next] - tables[high][index]);
        let lower = tables[low][index] + fraction * (tables[low][next] - tables[low][index]);
        higher + weight * (lower - higher)
    };
    for quantum in (start / 128 * 128..stop).step_by(128) {
        let begin = quantum.max(start);
        let end = (quantum + 128).min(stop);
        let automated = parameters
            .iter()
            .any(|values| values.sample_accurate[quantum / 128]);
        let frequency = |index: usize| {
            let frequency = parameters[0].samples[index];
            let detune = parameters.get(1).map_or(0., |value| value.samples[index]);
            (frequency * (detune / 1200.).exp2()).clamp(-rate * 0.5, rate * 0.5)
        };
        if !active {
            let offset = node.start * rate as f64 - start as f64;
            // Chrome's ARate branch leaves the scalar scheduling frequency at
            // zero; only Krate uses fractional start phase compensation.
            if !automated && offset < 0. {
                phase = -offset * frequency(quantum) as f64 * scale as f64;
            }
            active = true;
        }
        if automated {
            let frequency_accurate = parameters[0].sample_accurate[quantum / 128];
            let detune_accurate = parameters
                .get(1)
                .is_some_and(|value| value.sample_accurate[quantum / 128]);
            let phase_increment = |output_frame: usize| {
                // Blink reads the quantum's parameter vector from its start
                // even when the destination's scheduled source starts later.
                let frame = quantum + output_frame - begin;
                let mut final_scale = scale;
                let mut value = if frequency_accurate {
                    parameters[0].samples[frame]
                } else {
                    final_scale *= parameters[0].samples[quantum];
                    1.
                };
                if detune_accurate {
                    let detune = parameters[1].samples[frame];
                    let multiplier = (detune * (1.0 / 1200.0f32)).exp2();
                    value = if frequency_accurate {
                        value * multiplier
                    } else {
                        multiplier
                    };
                } else {
                    let detune = parameters.get(1).map_or(0., |value| value.samples[quantum]);
                    final_scale *= (detune / 1200.).exp2();
                }
                value.clamp(-rate / 2., rate / 2.) * final_scale
            };
            for (index, destination) in output.iter_mut().enumerate().take(end).skip(begin) {
                let increment = phase_increment(index);
                let normalized = (1. / scale) * increment;
                let position =
                    (phase as f32) - ((phase as f32) / size as f32).floor() * size as f32;
                let group_begin = begin + (index - begin) / 4 * 4;
                let vector_group = group_begin + 4 <= end
                    && (group_begin..group_begin + 4)
                        .all(|frame| phase_increment(frame).abs() >= 0.3);
                *destination = if vector_group {
                    sample(position, normalized, true)
                } else {
                    let (high, low, weight) = lookup(normalized, group_begin + 4 <= end);
                    interpolated_sample(
                        phase,
                        increment.abs(),
                        size - 1,
                        weight,
                        &tables[low],
                        &tables[high],
                    )
                };
                phase += increment as f64;
                phase -= (phase / size as f64).floor() * size as f64;
            }
        } else {
            let frequency = frequency(quantum);
            let increment = frequency * scale;
            if increment < 0.3 {
                let (high, low, weight) = lookup(frequency, false);
                for destination in output.iter_mut().take(end).skip(begin) {
                    *destination = interpolated_sample(
                        phase,
                        increment.abs(),
                        size - 1,
                        weight,
                        &tables[low],
                        &tables[high],
                    );
                    phase += increment as f64;
                    phase -= (phase / size as f64).floor() * size as f64;
                }
                continue;
            }
            let quantum_phase = phase;
            let count = (end - begin) / 4 * 4;
            let base = phase as f32;
            let mut lanes = [0.0f32; 4];
            for (lane, value) in lanes.iter_mut().enumerate() {
                let initial = base + lane as f32 * increment;
                *value = initial - (initial / size as f32).floor() * size as f32;
            }
            for group in (0..count).step_by(4) {
                for (lane, value) in lanes.iter_mut().enumerate() {
                    output[begin + group + lane] = sample(*value, frequency, false);
                    let next = *value + 4.0f32 * increment;
                    *value = next - (next / size as f32).floor() * size as f32;
                }
            }
            phase += (count as f32 * increment) as f64;
            phase -= (phase / size as f64).floor() * size as f64;
            let index = begin + count;
            for destination in output.iter_mut().take(end).skip(index) {
                *destination = sample(phase as f32, frequency, false);
                phase += increment as f64;
                phase -= (phase / size as f64).floor() * size as f64;
            }
            phase = quantum_phase + ((end - begin) as f32 * increment) as f64;
            phase -= (phase / size as f64).floor() * size as f64;
        }
    }
    output
}

fn visit(
    index: usize,
    nodes: &[Node],
    marks: &mut [u8],
    order: &mut Vec<usize>,
) -> Result<(), String> {
    if index >= nodes.len() {
        return Err("Invalid audio connection".into());
    }
    if marks[index] == 1 {
        return Err("Feedback cycles are not supported by the offline renderer".into());
    }
    if marks[index] == 2 {
        return Ok(());
    }
    marks[index] = 1;
    for &source in &nodes[index].inputs {
        visit(source, nodes, marks, order)?;
    }
    marks[index] = 2;
    order.push(index);
    Ok(())
}
fn parameter_limits(node: &Node, index: usize, rate: f32) -> (f32, f32) {
    let full = (-f32::MAX, f32::MAX);
    match (node.kind.as_str(), index) {
        ("oscillator", 0) => (-rate / 2., rate / 2.),
        ("oscillator" | "buffer", 1) => (-153600., 153600.),
        ("compressor", 0) => (-100., 0.),
        ("compressor", 1) => (0., 40.),
        ("compressor", 2) => (1., 20.),
        ("compressor", 3 | 4) => (0., 1.),
        ("panner", 0) => (-1., 1.),
        ("delay", 0) => (0., node.parameters.get(1).copied().unwrap_or(1.)),
        (kind, 0) if kind.starts_with("biquad:") => (0., rate / 2.),
        (kind, 2) if kind.starts_with("biquad:") => (-f32::MAX, 1541.2737),
        (kind, 3) if kind.starts_with("biquad:") => (-153600., 153600.),
        _ => full,
    }
}
pub(crate) fn render(graph: Graph) -> Result<Rendered, String> {
    if !(1..=2).contains(&graph.channels) {
        return Err("Offline DSP supports mono and stereo graphs".into());
    }
    if graph.length == 0
        || graph.length.saturating_mul(graph.channels) > MAX_SAMPLES
        || graph.nodes.is_empty()
        || graph.nodes.len() > MAX_NODES
        || graph.length.saturating_mul(graph.nodes.len()) > MAX_WORK
    {
        return Err("Offline audio resource limit exceeded".into());
    }
    if !graph.sample_rate.is_finite() || !(3000.0..=768000.0).contains(&graph.sample_rate) {
        return Err("Invalid offline sample rate".into());
    }
    if graph.nodes.iter().any(|node| {
        node.parameters.iter().any(|value| !value.is_finite())
            || node.data.iter().any(|channel| {
                channel.len() > MAX_SAMPLES || channel.iter().any(|value| !value.is_finite())
            })
    }) {
        return Err("Invalid audio sample or parameter".into());
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    let padded = graph.length.div_ceil(QUANTUM) * QUANTUM;
    let mut order = Vec::new();
    visit(0, &graph.nodes, &mut vec![0; graph.nodes.len()], &mut order)?;
    let mut sources = HashMap::new();
    let mut buffers = HashMap::new();
    let mut parameters: HashMap<usize, Vec<Values>> = HashMap::new();
    let mut compressors = HashMap::new();
    let mut effects = HashMap::new();
    let mut origins: HashMap<usize, usize> = HashMap::new();
    let mut tables = HashMap::new();
    for &index in &order {
        let node = &graph.nodes[index];
        let origin = if ["oscillator", "buffer"].contains(&node.kind.as_str()) {
            if node.started
                && (node.kind != "buffer" || node.data.first().is_some_and(|data| !data.is_empty()))
            {
                let start = crate::audio_buffer::frame(node.start, graph.sample_rate as f64, true)
                    / 128
                    * 128;
                if node.stop.is_some_and(|stop| {
                    crate::audio_buffer::frame(stop, graph.sample_rate as f64, true) <= start
                }) {
                    padded
                } else {
                    start.min(padded)
                }
            } else {
                padded
            }
        } else {
            node.inputs
                .iter()
                .map(|input| origins[input])
                .min()
                .unwrap_or(padded)
        };
        origins.insert(index, origin);
        let initial = if node.kind == "buffer" && node.parameters.is_empty() {
            vec![1., 0.]
        } else {
            node.parameters.clone()
        };
        let mut values = Vec::with_capacity(initial.len());
        for (parameter, initial) in initial.iter().enumerate() {
            let mut automation = node.automation.get(parameter).cloned().unwrap_or_default();
            if !automation.inputs.is_empty() {
                return Err(
                    "AudioParam signal connections are not yet supported by the native graph"
                        .into(),
                );
            }
            if node.kind == "buffer" || node.kind == "compressor" {
                automation.k_rate = true;
            }
            let mut rendered = audio_timeline::render_at(
                *initial,
                &automation,
                padded,
                graph.sample_rate as f64,
                origin,
            )?;
            let (minimum, maximum) = parameter_limits(node, parameter, graph.sample_rate);
            for value in &mut rendered.samples {
                *value = value.clamp(minimum, maximum);
            }
            values.push(rendered);
        }
        parameters.insert(index, values);
        match node.kind.as_str() {
            "destination" | "gain" => {}
            "oscillator" => {
                if !(1..=2).contains(&node.parameters.len())
                    || !["sine", "square", "sawtooth", "triangle"].contains(&node.shape.as_str())
                {
                    return Err("Unsupported oscillator parameters".into());
                }
                let table = tables
                    .entry(node.shape.clone())
                    .or_insert_with(|| wave_tables(&node.shape, graph.sample_rate));
                if node.started {
                    let simple = node.parameters.len() == 1
                        && node.start == 0.
                        && node.parameters.get(1).copied().unwrap_or(0.) == 0.
                        && node.automation.iter().all(|param| param.events.is_empty());
                    let mut data = if simple {
                        oscillator(table, graph.sample_rate, node.parameters[0], padded)
                    } else {
                        oscillator_events(
                            table,
                            graph.sample_rate,
                            node,
                            &parameters[&index],
                            padded,
                        )
                    };
                    if let Some(stop) = node.stop {
                        let at = crate::audio_buffer::frame(stop, graph.sample_rate as f64, true)
                            .min(data.len());
                        data[at..].fill(0.);
                    }
                    sources.insert(index, data);
                }
            }
            "buffer" => {
                if node.data.len() > 2 {
                    return Err("Offline buffer sources support mono and stereo".into());
                }
                buffers.insert(
                    index,
                    crate::audio_buffer::render(
                        node,
                        graph.sample_rate,
                        padded,
                        &parameters[&index],
                    ),
                );
            }
            "compressor" => {
                compressors.insert(index, Compressor::new(graph.sample_rate, &node.parameters)?);
            }
            kind if kind.starts_with("biquad:")
                || ["waveshaper", "delay", "panner"].contains(&kind) =>
            {
                effects.insert(
                    index,
                    crate::audio_effects::Effect::new(
                        kind,
                        graph.sample_rate,
                        &node.parameters,
                        &node.curve,
                    )?,
                );
            }
            _ => return Err("Unsupported offline audio node".into()),
        }
    }
    let mut output = vec![Vec::with_capacity(graph.length); graph.channels];
    for offset in (0..padded).step_by(QUANTUM) {
        if Instant::now() > deadline {
            return Err("Offline audio execution limit exceeded".into());
        }
        let mut blocks: HashMap<usize, Vec<Vec<f32>>> = HashMap::new();
        let mut activity: HashMap<usize, bool> = HashMap::new();
        let mut channel_counts: HashMap<usize, usize> = HashMap::new();
        for &index in &order {
            let node = &graph.nodes[index];
            let mut block = vec![vec![0.; QUANTUM]; 2];
            let mut active = node.inputs.iter().any(|input| activity[input]);
            let mut input_channels = node
                .inputs
                .iter()
                .map(|input| channel_counts[input])
                .max()
                .unwrap_or(1);
            for input in &node.inputs {
                let source = &blocks[input];
                for channel in 0..2 {
                    for sample in 0..QUANTUM {
                        block[channel][sample] += source[channel][sample];
                    }
                }
            }
            match node.kind.as_str() {
                "oscillator" => {
                    active = node.started
                        && crate::audio_buffer::frame(node.start, graph.sample_rate as f64, true)
                            < offset + 128
                        && node.stop.is_none_or(|stop| {
                            crate::audio_buffer::frame(stop, graph.sample_rate as f64, true)
                                > offset
                        });
                    if let Some(source) = sources.get(&index) {
                        for channel in &mut block {
                            channel.copy_from_slice(&source[offset..offset + QUANTUM]);
                        }
                    }
                }
                "buffer" => {
                    input_channels = node.data.len().clamp(1, 2);
                    active = buffers
                        .get(&index)
                        .is_some_and(|buffer| buffer.active[offset / 128]);
                    if let Some(source) = buffers.get(&index) {
                        for (channel, destination) in block.iter_mut().enumerate() {
                            destination.copy_from_slice(
                                &source.channels[channel.min(source.channels.len() - 1)]
                                    [offset..offset + QUANTUM],
                            );
                        }
                    }
                }
                "gain" => {
                    let values = parameters[&index].first().ok_or("Invalid gain parameter")?;
                    for channel in &mut block {
                        for (sample, value) in channel.iter_mut().enumerate() {
                            *value *= values.samples[offset + sample];
                        }
                    }
                }
                "compressor" => {
                    let compressor = compressors.get_mut(&index).unwrap();
                    compressor.parameters(&parameters[&index], offset);
                    compressor.process(&mut block);
                }
                _ => {
                    if let Some(effect) = effects.get_mut(&index) {
                        effect
                            .set_input_silent(!active, offset as f64 / graph.sample_rate as f64)?;
                        let values = parameters[&index]
                            .iter()
                            .take(if node.kind == "delay" { 1 } else { 4 })
                            .map(|value| value.samples[offset..offset + 128].to_vec())
                            .collect::<Vec<_>>();
                        effect.set_sample_accurate(
                            node.kind == "delay"
                                && !node.automation.first().is_some_and(|param| param.k_rate)
                                || parameters[&index]
                                    .iter()
                                    .any(|param| param.sample_accurate[offset / 128]),
                        );
                        effect.process(&mut block, input_channels, &values)?;
                        active = !effect.output_silent();
                        input_channels = effect.output_channels(input_channels);
                    }
                }
            }
            activity.insert(index, active);
            channel_counts.insert(index, input_channels);
            blocks.insert(index, block);
        }
        let destination = &blocks[&0];
        let count = QUANTUM.min(graph.length - offset);
        for channel in 0..graph.channels {
            for (sample, _) in destination[channel].iter().enumerate().take(count) {
                let value = if graph.channels == 1 {
                    (destination[0][sample] + destination[1][sample]) * 0.5
                } else {
                    destination[channel][sample]
                };
                output[channel].push(value);
            }
        }
    }
    Ok(Rendered {
        channels: output,
        reductions: compressors
            .iter()
            .map(|(&index, compressor)| (index.to_string(), compressor.reduction()))
            .collect(),
        final_values: parameters
            .iter()
            .map(|(index, values)| {
                (
                    index.to_string(),
                    values
                        .iter()
                        .map(|value| *value.samples.last().unwrap())
                        .collect(),
                )
            })
            .collect(),
        ends: order
            .iter()
            .filter_map(|&index| {
                let node = &graph.nodes[index];
                if !node.started {
                    return None;
                }
                let end = if node.kind == "buffer" {
                    buffers.get(&index).and_then(|buffer| buffer.ended)
                } else {
                    node.stop.map(|stop| {
                        crate::audio_buffer::frame(stop, graph.sample_rate as f64, true)
                    })
                };
                end.filter(|&end| end <= padded)
                    .map(|end| (index.to_string(), end))
            })
            .collect(),
        current_time: padded as f64 / graph.sample_rate as f64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
    use sha2::{Digest, Sha256};
    fn node(kind: &str, inputs: Vec<usize>, parameters: Vec<f32>) -> Node {
        Node {
            kind: kind.into(),
            inputs,
            parameters,
            shape: "sine".into(),
            started: true,
            stop: None,
            data: vec![],
            ..Default::default()
        }
    }
    #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
    fn digest(values: &[f32]) -> String {
        let mut hash = Sha256::new();
        for value in values {
            hash.update(value.to_le_bytes());
        }
        format!("{:x}", hash.finalize())
    }
    // These bytes are measured independently by audio-probe.js in real Chrome
    // 155 on ARM64, not generated by this implementation or stored wave outputs.
    #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
    #[test]
    fn native_audio_matches_measured_chrome_155_pcm_for_distinct_real_graphs() {
        for (shape, frequency, gain, compression, expected) in [
            (
                "triangle",
                10000.,
                None,
                None,
                "84b108b728156da1fd14178b77e228c21a3adfa31677abd78497d533a1bac987",
            ),
            (
                "sine",
                440.,
                None,
                None,
                "ccb9d61b47ba88903d682b1bb635729ec5b2d29b1e4a17a183b73519d5fffbe5",
            ),
            (
                "square",
                220.,
                Some(0.25),
                None,
                "8ad515884b880db0350f33d8497f75c9d350075fe72185c2214188e04e81dbb9",
            ),
            (
                "triangle",
                10000.,
                None,
                Some(vec![-50., 40., 12., 0., 0.25]),
                "3ef8a960b6869b916572bcfb85569a918a2498f3296a3e935647e4b6f00da0b9",
            ),
            (
                "sine",
                880.,
                None,
                Some(vec![-24., 20., 4., 0.01, 0.1]),
                "777c8f5afba5ac1d3376216789971820929de7c2ddb4e1fb0476f597e7348e29",
            ),
        ] {
            let mut nodes = vec![
                node("destination", vec![], vec![]),
                node("oscillator", vec![], vec![frequency]),
            ];
            nodes[1].shape = shape.into();
            let mut tail = 1;
            if let Some(value) = gain {
                nodes.push(node("gain", vec![tail], vec![value]));
                tail = nodes.len() - 1;
            }
            if let Some(values) = compression {
                nodes.push(node("compressor", vec![tail], values));
                tail = nodes.len() - 1;
            }
            nodes[0].inputs.push(tail);
            let rendered = render(Graph {
                channels: 1,
                length: 44100,
                sample_rate: 44100.,
                nodes,
            })
            .unwrap();
            assert_eq!(
                digest(&rendered.channels[0]),
                expected,
                "{shape}/{frequency}"
            );
            assert_eq!(rendered.current_time, 44160.0 / 44100.0);
        }
    }
    #[test]
    fn native_audio_mixes_buffers_disconnects_and_applies_gain_and_stop() {
        let mut source = node("buffer", vec![], vec![]);
        source.data = vec![vec![0.75; 256], vec![-0.25; 256]];
        source.stop = Some(128.0 / 44100.0);
        let graph = Graph {
            channels: 2,
            length: 256,
            sample_rate: 44100.,
            nodes: vec![
                node("destination", vec![2], vec![]),
                source,
                node("gain", vec![1], vec![0.5]),
            ],
        };
        let rendered = render(graph).unwrap();
        assert_eq!(&rendered.channels[0][..128], &[0.375; 128]);
        assert_eq!(&rendered.channels[1][..128], &[-0.125; 128]);
        assert!(rendered
            .channels
            .iter()
            .all(|channel| channel[128..].iter().all(|value| *value == 0.)));
        let silence = render(Graph {
            channels: 1,
            length: 128,
            sample_rate: 44100.,
            nodes: vec![node("destination", vec![], vec![])],
        })
        .unwrap();
        assert!(silence.channels[0].iter().all(|value| *value == 0.));
    }
    #[test]
    fn native_audio_rejects_cycles_unsupported_nodes_and_resource_exhaustion() {
        let graph = |nodes, length| Graph {
            channels: 1,
            length,
            sample_rate: 44100.,
            nodes,
        };
        assert!(render(graph(
            vec![
                node("destination", vec![1], vec![]),
                node("gain", vec![1], vec![1.])
            ],
            128
        ))
        .unwrap_err()
        .contains("Feedback"));
        assert!(render(graph(
            vec![
                node("destination", vec![1], vec![]),
                node("convolver", vec![], vec![])
            ],
            128
        ))
        .unwrap_err()
        .contains("Unsupported"));
        assert!(render(graph(
            vec![node("destination", vec![], vec![])],
            MAX_SAMPLES + 1
        ))
        .unwrap_err()
        .contains("limit"));
    }
}

#[cfg(all(test, target_os = "macos", target_arch = "aarch64"))]
mod advanced_controls {
    use super::*;
    use sha2::{Digest, Sha256};
    #[test]
    fn scheduled_automated_resampled_and_effect_graphs_match_real_chrome_pcm() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../harness/detection/fixtures/audio-advanced-chrome155.json"
        ))
        .unwrap();
        let input: Vec<f32> = fixture["input"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_f64().unwrap() as f32)
            .collect();
        for case in fixture["cases"].as_array().unwrap() {
            let mut graph: Graph = serde_json::from_value(case["graph"].clone()).unwrap();
            for node in &mut graph.nodes {
                if node.kind == "buffer" {
                    node.data = vec![input.clone()];
                }
            }
            let output = render(graph).unwrap();
            for (channel, expected) in output
                .channels
                .iter()
                .zip(case["sha256"].as_array().unwrap())
            {
                let mut hash = Sha256::new();
                for sample in channel {
                    hash.update(sample.to_le_bytes());
                }
                assert_eq!(
                    format!("{:x}", hash.finalize()),
                    expected.as_str().unwrap(),
                    "{}",
                    case["name"]
                );
            }
        }
    }
}
