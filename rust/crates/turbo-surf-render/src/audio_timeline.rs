//! Native AudioParam event evaluation, following Chrome 155's licensed
//! AudioParamHandler arithmetic (including per-quantum exponential rounding).
use serde::Deserialize;
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Event {
    pub kind: String,
    pub time: f64,
    #[serde(default)]
    pub value: f32,
    #[serde(default)]
    pub constant: f64,
    #[serde(default)]
    pub duration: f64,
    #[serde(default)]
    pub curve: Vec<f32>,
}
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Automation {
    #[serde(default)]
    pub events: Vec<Event>,
    #[serde(default)]
    pub k_rate: bool,
    #[serde(default)]
    pub dirty: bool,
    #[serde(default)]
    pub inputs: Vec<usize>,
}
unsafe extern "C" {
    fn surf_audio_pow(x: f64, y: f64) -> f64;
    fn surf_audio_exp(x: f64) -> f64;
}
pub(crate) fn power(x: f64, y: f64) -> f64 {
    unsafe { surf_audio_pow(x, y) }
}
fn exp(x: f64) -> f64 {
    unsafe { surf_audio_exp(x) }
}

pub(crate) fn validate(automation: &Automation) -> Result<(), String> {
    if automation.events.len() > 256 {
        return Err("AudioParam event limit exceeded".into());
    }
    let mut previous = -1.0;
    for event in &automation.events {
        if !event.time.is_finite()
            || event.time < 0.
            || event.time < previous
            || !event.value.is_finite()
            || !event.constant.is_finite()
            || !event.duration.is_finite()
            || event.curve.len() > 65536
            || event.curve.iter().any(|value| !value.is_finite())
        {
            return Err("Invalid audio automation event".into());
        }
        if !["set", "linear", "exponential", "target", "curve"].contains(&event.kind.as_str()) {
            return Err("Unknown audio automation event".into());
        }
        if event.kind == "target" && event.constant < 0. {
            return Err("Invalid target time constant".into());
        }
        if event.kind == "curve" && (event.curve.len() < 2 || event.duration <= 0.) {
            return Err("Invalid audio value curve".into());
        }
        previous = event.time;
    }
    Ok(())
}
#[derive(Clone)]
pub(crate) struct Values {
    pub samples: Vec<f32>,
    pub sample_accurate: Vec<bool>,
}

pub(crate) fn render(
    initial: f32,
    automation: &Automation,
    length: usize,
    rate: f64,
) -> Result<Values, String> {
    validate(automation)?;
    let mut events = automation.events.clone();
    // A ramp with no previous event starts at the intrinsic value at time zero.
    if events
        .first()
        .is_some_and(|event| event.kind == "linear" || event.kind == "exponential")
    {
        events.insert(
            0,
            Event {
                kind: "set".into(),
                time: 0.,
                value: initial,
                constant: 0.,
                duration: 0.,
                curve: vec![],
            },
        );
    }
    let mut samples = vec![initial; length];
    let mut sample_accurate = vec![false; length.div_ceil(128)];
    let mut last = initial;
    let mut target_index = None;
    for quantum in (0..length).step_by(128) {
        let end = (quantum + 128).min(length);
        let mut frame = quantum;
        while frame < end {
            let time = frame as f64 / rate;
            let Some(index) = events.iter().rposition(|event| event.time <= time) else {
                frame += if automation.k_rate { 128 } else { 1 };
                continue;
            };
            let event = &events[index];
            let next = events.get(index + 1);
            let segment_end = next
                .map_or(end, |next| end.min((next.time * rate).ceil() as usize))
                .max(frame + 1);
            let count = if automation.k_rate {
                1
            } else {
                segment_end - frame
            };
            sample_accurate[quantum / 128] = !automation.k_rate
                && !(events.len() == 1
                    && event.kind != "target"
                    && event.kind != "curve"
                    && event.time < quantum as f64 / rate)
                && events.last().is_some_and(|last| {
                    let last_time = last.time
                        + if last.kind == "curve" {
                            last.duration
                        } else {
                            0.
                        };
                    last.kind == "target"
                        || quantum as f64 <= (last_time * rate / 128.).ceil() * 128.
                });
            if let Some(next) =
                next.filter(|next| next.kind == "linear" || next.kind == "exponential")
            {
                let start_value = event.value;
                let delta = next.time - event.time;
                if next.kind == "linear" {
                    let inverse = if delta <= f32::MIN_POSITIVE as f64 {
                        0.
                    } else {
                        (1. / delta) as f32
                    };
                    let difference = next.value - start_value;
                    for offset in 0..count {
                        let progress =
                            (((frame + offset) as f64 / rate - event.time) * inverse as f64) as f32;
                        last = start_value + progress * difference;
                        samples[frame + offset] = last;
                    }
                } else if start_value * next.value <= 0. || delta <= 0. {
                    last = start_value;
                    samples[frame..frame + count].fill(last);
                } else {
                    // Blink rounds the quantum's starting value to f32, then
                    // advances a double accumulator using its native fdlibm pow.
                    let multiplier = power((next.value / start_value) as f64, 1. / (delta * rate));
                    last = (start_value as f64
                        * power(
                            next.value as f64 / start_value as f64,
                            (time - event.time) / delta,
                        )) as f32;
                    let mut accumulator = last as f64;
                    for offset in 0..count {
                        last = accumulator as f32;
                        samples[frame + offset] = last;
                        accumulator *= multiplier;
                    }
                }
                target_index = None;
            } else if event.kind == "target" {
                let target = event.value;
                let constant = event.constant as f32;
                if constant == 0. {
                    last = target;
                    samples[frame..frame + count].fill(last);
                } else {
                    let control_rate = if automation.k_rate { rate / 128. } else { rate };
                    let discrete = (1. - exp(-1. / (control_rate * constant as f64))) as f32;
                    if target_index != Some(index) {
                        last = (target as f64
                            + (last - target) as f64 * exp(-(time - event.time) / constant as f64))
                            as f32;
                    } else {
                        last += (target - last) * discrete;
                    }
                    for offset in 0..count {
                        samples[frame + offset] = last;
                        last += (target - last) * discrete;
                    }
                    last = samples[frame + count - 1];
                }
                target_index = Some(index);
            } else if event.kind == "curve" {
                let points = (event.curve.len() - 1) as f64 / event.duration / rate;
                let base = points * (frame as f64 - event.time * rate);
                for offset in 0..count {
                    let position = base + offset as f64 * points;
                    let first = (position as usize).min(event.curve.len() - 1);
                    let second = (first + 1).min(event.curve.len() - 1);
                    let fraction = (position - first as f64).min(1.);
                    last = (event.curve[first] as f64
                        + (event.curve[second] - event.curve[first]) as f64 * fraction)
                        as f32;
                    samples[frame + offset] = last;
                }
                target_index = None;
            } else {
                last = event.value;
                samples[frame..frame + count].fill(last);
                target_index = None;
            }
            if automation.k_rate {
                samples[quantum..end].fill(last);
                frame = end;
            } else {
                frame += count;
            }
        }
        if automation.dirty && quantum == 0 && !automation.k_rate {
            sample_accurate[0] = true;
        }
    }
    Ok(Values {
        samples,
        sample_accurate,
    })
}

pub(crate) fn render_at(
    initial: f32,
    automation: &Automation,
    length: usize,
    rate: f64,
    origin: usize,
) -> Result<Values, String> {
    if origin >= length {
        return Ok(Values {
            samples: vec![initial; length],
            sample_accurate: vec![false; length.div_ceil(128)],
        });
    }
    let mut shifted = automation.clone();
    let current = origin as f64 / rate;
    for event in &mut shifted.events {
        if event.kind != "curve" && event.time < current {
            event.time = current;
        }
    }
    shifted.events.sort_by(|a, b| a.time.total_cmp(&b.time));
    let mut result = render(initial, &shifted, length, rate)?;
    // HasSampleAccurateValues is queried before Blink first clamps new events.
    let accurate = if automation.events.len() == 1 {
        let event = &automation.events[0];
        match event.kind.as_str() {
            "target" => event.time <= current + 128. / rate,
            "curve" => event.time <= current && current < event.time + event.duration,
            _ => event.time >= current,
        }
    } else {
        automation.events.len() > 1
            && automation
                .events
                .first()
                .is_some_and(|event| event.time <= current + 128. / rate)
    };
    result.sample_accurate[origin / 128] = accurate && !automation.k_rate;
    result.samples[..origin].fill(initial);
    Ok(result)
}

pub(crate) fn value_at(
    initial: f32,
    automation: Automation,
    time: f64,
    rate: f64,
) -> Result<f32, String> {
    if !time.is_finite() || time < 0. || !rate.is_finite() || !(3000.0..=768000.0).contains(&rate) {
        return Err("Invalid AudioParam hold time".into());
    }
    validate(&automation)?;
    if automation
        .events
        .iter()
        .rfind(|event| event.time <= time)
        .is_some_and(|event| event.kind == "target")
    {
        let frames = (time * rate).ceil() as usize;
        if frames >= 262144 {
            return Err("AudioParam hold evaluation limit exceeded".into());
        }
        return Ok(render(initial, &automation, frames + 1, rate)?.samples[frames]);
    }
    let mut previous_time = 0.;
    let mut previous_value = initial;
    for event in &automation.events {
        if event.time > time {
            let fraction = (time - previous_time) / (event.time - previous_time);
            return Ok(match event.kind.as_str() {
                "linear" => {
                    (previous_value as f64 + (event.value - previous_value) as f64 * fraction)
                        as f32
                }
                "exponential" if previous_value * event.value > 0. => {
                    (previous_value as f64
                        * power(event.value as f64 / previous_value as f64, fraction))
                        as f32
                }
                _ => previous_value,
            });
        }
        previous_value = match event.kind.as_str() {
            "target" if event.constant > 0. => {
                (event.value as f64
                    + (previous_value - event.value) as f64
                        * exp(-(time - event.time) / event.constant)) as f32
            }
            "curve" => {
                let position = ((time - event.time) / event.duration
                    * (event.curve.len() - 1) as f64)
                    .clamp(0., (event.curve.len() - 1) as f64);
                let first = position as usize;
                let second = (first + 1).min(event.curve.len() - 1);
                (event.curve[first] as f64
                    + (event.curve[second] - event.curve[first]) as f64 * (position - first as f64))
                    as f32
            }
            _ => event.value,
        };
        previous_time = event.time;
    }
    Ok(previous_value)
}
