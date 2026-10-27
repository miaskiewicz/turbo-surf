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

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(kind: &str, time: f64, value: f32) -> Event {
        Event {
            kind: kind.into(),
            time,
            value,
            constant: 0.,
            duration: 0.,
            curve: vec![],
        }
    }

    fn auto(events: Vec<Event>) -> Automation {
        Automation {
            events,
            ..Default::default()
        }
    }

    #[test]
    fn validation_rejects_each_malformed_timeline() {
        let err = |events: Vec<Event>| validate(&auto(events)).unwrap_err();
        assert_eq!(
            err((0..257).map(|i| ev("set", i as f64, 0.)).collect()),
            "AudioParam event limit exceeded"
        );
        assert!(validate(&auto((0..256).map(|i| ev("set", i as f64, 0.)).collect())).is_ok());
        let mut nan_constant = ev("target", 0., 0.);
        nan_constant.constant = f64::NAN;
        let mut infinite_duration = ev("set", 0., 0.);
        infinite_duration.duration = f64::INFINITY;
        let mut huge_curve = ev("curve", 0., 0.);
        huge_curve.curve = vec![0.; 65537];
        huge_curve.duration = 1.;
        for bad in [
            ev("set", f64::NAN, 0.),
            ev("set", -1., 0.),
            ev("set", 0., f32::INFINITY),
            nan_constant,
            infinite_duration,
            huge_curve,
        ] {
            assert_eq!(err(vec![bad]), "Invalid audio automation event");
        }
        assert_eq!(
            err(vec![ev("set", 2., 0.), ev("set", 1., 0.)]),
            "Invalid audio automation event"
        );
        assert_eq!(
            err(vec![ev("hold", 0., 0.)]),
            "Unknown audio automation event"
        );
        let mut target = ev("target", 0., 1.);
        target.constant = -0.5;
        assert_eq!(err(vec![target]), "Invalid target time constant");
        let mut short = ev("curve", 0., 0.);
        short.curve = vec![1.];
        short.duration = 1.;
        assert_eq!(err(vec![short]), "Invalid audio value curve");
        let mut flat = ev("curve", 0., 0.);
        flat.curve = vec![1., 2.];
        assert_eq!(err(vec![flat]), "Invalid audio value curve");
        let mut nan_curve = ev("curve", 0., 0.);
        nan_curve.curve = vec![1., f32::NAN];
        nan_curve.duration = 1.;
        assert_eq!(err(vec![nan_curve]), "Invalid audio automation event");
        let mut max_curve = ev("curve", 0., 0.);
        max_curve.curve = vec![0.; 65536];
        max_curve.duration = 1.;
        assert!(validate(&auto(vec![max_curve])).is_ok());
    }

    #[test]
    fn value_at_follows_the_web_audio_ramp_target_and_curve_formulas() {
        let rate = 48000.;
        // A first ramp starts from the intrinsic value at t=0.
        let linear = auto(vec![ev("linear", 2., 10.)]);
        assert_eq!(value_at(2., linear.clone(), 1., rate).unwrap(), 6.);
        // Exponential: v0 * (v1/v0)^fraction; with a sign change it holds v0.
        let expo = auto(vec![ev("set", 0., 1.), ev("exponential", 2., 4.)]);
        assert!((value_at(0., expo, 1., rate).unwrap() - 2.).abs() < 1e-6);
        let crossing = auto(vec![ev("set", 0., -1.), ev("exponential", 2., 4.)]);
        assert_eq!(value_at(0., crossing, 1., rate).unwrap(), -1.);
        // A later "set" holds the previous value until it starts.
        let step = auto(vec![ev("set", 0., 3.), ev("set", 5., 9.)]);
        assert_eq!(value_at(0., step.clone(), 4.9, rate).unwrap(), 3.);
        assert_eq!(value_at(0., step, 5., rate).unwrap(), 9.);
        // A curve interpolates linearly over its duration and clamps after it.
        let mut curve = ev("curve", 1., 0.);
        curve.curve = vec![0., 10., 20.];
        curve.duration = 2.;
        let curve = auto(vec![curve, ev("set", 10., 0.)]);
        assert_eq!(value_at(0., curve.clone(), 2.5, rate).unwrap(), 15.);
        assert_eq!(value_at(0., curve, 4., rate).unwrap(), 20.);
        // A target that has been superseded by a later event uses the closed form.
        let mut target = ev("target", 0., 0.);
        target.constant = 1.;
        let after = auto(vec![ev("set", 0., 1.), target, ev("set", 5., 7.)]);
        let expected = (-4.9f64).exp() as f32;
        let held = value_at(1., after.clone(), 4.9, rate).unwrap();
        assert!((held - expected).abs() < 1e-5, "{held} vs {expected}");
        // An active target is rendered sample by sample (the hold limit applies).
        let mut active = ev("target", 0., 0.);
        active.constant = 0.5;
        let active = auto(vec![active]);
        let v = value_at(1., active.clone(), 0.01, rate).unwrap();
        assert!((v - (-0.02f64).exp() as f32).abs() < 1e-3, "{v}");
        assert_eq!(
            value_at(1., active, 6., rate).unwrap_err(),
            "AudioParam hold evaluation limit exceeded"
        );
        for (time, r) in [
            (f64::NAN, rate),
            (-1., rate),
            (0., 2999.),
            (0., 768001.),
            (0., f64::INFINITY),
        ] {
            assert_eq!(
                value_at(0., auto(vec![]), time, r).unwrap_err(),
                "Invalid AudioParam hold time"
            );
        }
        assert_eq!(value_at(5., auto(vec![]), 1., rate).unwrap(), 5.);
    }

    #[test]
    fn rendered_ramps_handle_degenerate_spans_and_instant_targets() {
        let rate = 3000.;
        // Linear ramp onto the same instant: no division by zero, jumps to the end value.
        let same = render(
            0.,
            &auto(vec![ev("set", 0., 1.), ev("linear", 0., 5.)]),
            4,
            rate,
        )
        .unwrap();
        assert!(same.samples.iter().all(|v| v.is_finite()));
        assert_eq!(*same.samples.last().unwrap(), 5.);
        // Exponential across zero holds its start value until the end time.
        let held = render(
            0.,
            &auto(vec![ev("set", 0., -2.), ev("exponential", 1., 3.)]),
            8,
            rate,
        )
        .unwrap();
        assert_eq!(&held.samples[..8], &[-2.; 8]);
        // A first ramp starts from the intrinsic value.
        let ramp = render(4., &auto(vec![ev("linear", 1., 0.)]), 3001, rate).unwrap();
        assert_eq!(ramp.samples[0], 4.);
        assert!((ramp.samples[1500] - 2.).abs() < 1e-3);
        // A zero time constant reaches the target immediately.
        let instant = render(1., &auto(vec![ev("target", 0., 6.)]), 4, rate).unwrap();
        assert_eq!(instant.samples, vec![6.; 4]);
    }
}
