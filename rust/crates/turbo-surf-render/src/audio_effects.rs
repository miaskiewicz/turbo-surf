//! Independent Chrome155 CPU effect kernels with checked, bounded native ownership.
use std::{ffi::c_void, ptr::NonNull};

const QUANTUM: usize = 128;
const MAX_CURVE: usize = 262_144;
const MAX_DELAY_FRAMES: f64 = 1_000_000.0;

unsafe extern "C" {
    fn surf_audio_effect_new(
        kind: u32,
        shape: u32,
        rate: f32,
        maximum: f32,
        curve: *const f32,
        length: usize,
    ) -> *mut c_void;
    fn surf_audio_effect_process(
        effect: *mut c_void,
        left: *mut f32,
        right: *mut f32,
        channels: u32,
        frequency: *const f32,
        q: *const f32,
        gain: *const f32,
        detune: *const f32,
        accurate: bool,
    );
    fn surf_audio_effect_free(effect: *mut c_void);
    fn surf_audio_effect_set_input(effect: *mut c_void, silent: bool, current_time: f64);
    fn surf_audio_effect_output_silent(effect: *mut c_void) -> bool;
    fn surf_audio_biquad_frequency_response(
        effect: *mut c_void,
        parameters: *const f32,
        frequencies: *const f32,
        length: usize,
        magnitude: *mut f32,
        phase: *mut f32,
    );
}

pub(crate) struct Effect {
    native: NonNull<c_void>,
    parameters: [f32; 4],
    count: usize,
    panner: bool,
    accurate: bool,
    delay_maximum: Option<f32>,
}

impl Effect {
    pub(crate) fn frequency_response(
        kind: &str,
        sample_rate: f32,
        parameters: &[f32],
        frequencies: &[f32],
    ) -> Result<(Vec<f32>, Vec<f32>), String> {
        if !kind.starts_with("biquad:") || frequencies.len() > MAX_CURVE {
            return Err("Invalid or oversized native frequency response".into());
        }
        let effect = Self::new(kind, sample_rate, parameters, &[])?;
        let mut magnitude = vec![0.0; frequencies.len()];
        let mut phase = vec![0.0; frequencies.len()];
        if !frequencies.is_empty() {
            // A temporary kernel uses the current intrinsic AudioParam values;
            // both response vectors are owned and sized before the native call.
            unsafe {
                surf_audio_biquad_frequency_response(
                    effect.native.as_ptr(),
                    parameters.as_ptr(),
                    frequencies.as_ptr(),
                    frequencies.len(),
                    magnitude.as_mut_ptr(),
                    phase.as_mut_ptr(),
                );
            }
        }
        Ok((magnitude, phase))
    }

    pub(crate) fn new(
        kind: &str,
        sample_rate: f32,
        params: &[f32],
        curve: &[f32],
    ) -> Result<Self, String> {
        if !sample_rate.is_finite() || !(3000.0..=384_000.0).contains(&sample_rate) {
            return Err("Invalid native effect sample rate".into());
        }
        let (code, shape, count) = if let Some(shape) = kind.strip_prefix("biquad:") {
            let shape = [
                "lowpass",
                "highpass",
                "bandpass",
                "lowshelf",
                "highshelf",
                "peaking",
                "notch",
                "allpass",
            ]
            .iter()
            .position(|name| *name == shape)
            .ok_or("Unsupported native biquad type")?;
            (0, shape as u32, 4)
        } else {
            match kind {
                "delay" => (1, 0, 1),
                "panner" | "stereopanner" => (2, 0, 1),
                "waveshaper" => (3, 0, 0),
                _ => return Err("Unsupported native audio effect".into()),
            }
        };
        let valid_count = if code == 1 {
            (1..=2).contains(&params.len())
        } else {
            params.len() == count
        };
        if !valid_count || params.iter().any(|value| !value.is_finite()) {
            return Err("Invalid native effect parameters".into());
        }
        if curve.len() == 1 || curve.len() > MAX_CURVE {
            return Err("Invalid or oversized native shaping curve".into());
        }
        let maximum = if code == 1 {
            params.get(1).copied().unwrap_or(1.0)
        } else {
            1.0
        };
        if code == 1
            && (maximum <= 0.0 || f64::from(maximum) * f64::from(sample_rate) > MAX_DELAY_FRAMES)
        {
            return Err("Native delay allocation exceeds its resource budget".into());
        }
        // The kernel's constructor copies the curve before this borrow ends.
        let native = NonNull::new(unsafe {
            surf_audio_effect_new(
                code,
                shape,
                sample_rate,
                maximum,
                curve.as_ptr(),
                curve.len(),
            )
        })
        .ok_or("Native audio effect allocation failed")?;
        let mut parameters = [0.0; 4];
        parameters[..count].copy_from_slice(&params[..count]);
        Ok(Self {
            native,
            parameters,
            count,
            panner: code == 2,
            accurate: false,
            delay_maximum: (code == 1).then_some(maximum),
        })
    }

    pub(crate) fn set_sample_accurate(&mut self, accurate: bool) {
        self.accurate = accurate;
    }

    pub(crate) fn set_input_silent(
        &mut self,
        silent: bool,
        current_time: f64,
    ) -> Result<(), String> {
        if !current_time.is_finite() || current_time < 0.0 {
            return Err("Invalid native effect clock".into());
        }
        unsafe { surf_audio_effect_set_input(self.native.as_ptr(), silent, current_time) };
        Ok(())
    }

    pub(crate) fn output_silent(&self) -> bool {
        unsafe { surf_audio_effect_output_silent(self.native.as_ptr()) }
    }

    pub(crate) fn output_channels(&self, input_channels: usize) -> usize {
        if self.panner {
            2
        } else {
            input_channels
        }
    }

    pub(crate) fn process(
        &mut self,
        channels: &mut [Vec<f32>],
        input_channels: usize,
        parameter_values: &[Vec<f32>],
    ) -> Result<(), String> {
        if channels.len() != 2
            || channels.iter().any(|channel| channel.len() != QUANTUM)
            || !(1..=2).contains(&input_channels)
        {
            return Err("Native effect requires two bounded 128-frame storage channels".into());
        }
        if !parameter_values.is_empty()
            && (parameter_values.len() != self.count
                || parameter_values
                    .iter()
                    .any(|values| values.len() != QUANTUM))
        {
            return Err("Invalid native effect automation quantum".into());
        }
        let defaults = self.parameters.map(|value| [value; QUANTUM]);
        let mut pointers = std::array::from_fn::<_, 4, _>(|index| {
            parameter_values
                .get(index)
                .map_or(defaults[index].as_ptr(), |values| values.as_ptr())
        });
        let mut bounded_delay = [0.0; QUANTUM];
        if let Some(maximum) = self.delay_maximum {
            for (index, target) in bounded_delay.iter_mut().enumerate() {
                let value = parameter_values
                    .first()
                    .map_or(defaults[0][index], |values| values[index]);
                *target = if value.is_nan() {
                    maximum
                } else {
                    value.clamp(0.0, maximum)
                };
            }
            pointers[0] = bounded_delay.as_ptr();
        }
        let (left, right) = channels.split_at_mut(1);
        // Both writable vectors and all parameter vectors were checked above.
        // The opaque C++ state survives quanta and is released exactly once.
        unsafe {
            surf_audio_effect_process(
                self.native.as_ptr(),
                left[0].as_mut_ptr(),
                right[0].as_mut_ptr(),
                input_channels as u32,
                pointers[0],
                pointers[1],
                pointers[2],
                pointers[3],
                self.accurate,
            );
        }
        Ok(())
    }
}

impl Drop for Effect {
    fn drop(&mut self) {
        unsafe { surf_audio_effect_free(self.native.as_ptr()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_biquad_response_matches_eight_independent_chrome_filters_and_frequency_bounds() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../harness/detection/fixtures/audio-effects-frequency-chrome155.json"
        ))
        .unwrap();
        let frequencies = [
            -1.0,
            0.0,
            10.0,
            100.0,
            350.0,
            1000.0,
            10_000.0,
            22_050.0,
            22_051.0,
            f32::NAN,
            f32::INFINITY,
        ];
        for (kind, expected) in fixture["result"]["graphs"].as_object().unwrap() {
            let (magnitude, phase) = Effect::frequency_response(
                &format!("biquad:{kind}"),
                44_100.0,
                &[1200.0, 0.7, 3.0, 120.0],
                &frequencies,
            )
            .unwrap();
            for (name, values) in [("magnitude", magnitude), ("phase", phase)] {
                for (index, (value, expected)) in values
                    .iter()
                    .zip(expected[name].as_array().unwrap())
                    .enumerate()
                {
                    if expected == "NaN" {
                        assert!(value.is_nan(), "{kind} {name} {index}");
                    } else {
                        assert_eq!(
                            *value,
                            expected.as_f64().unwrap() as f32,
                            "{kind} {name} {index}"
                        );
                    }
                }
            }
        }
        assert_eq!(
            Effect::frequency_response("biquad:lowpass", 44_100.0, &[350.0, 1.0, 0.0, 0.0], &[])
                .unwrap(),
            (vec![], vec![])
        );
    }

    #[test]
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    fn native_effect_kernels_match_independent_chrome155_pcm_and_real_tail_lifecycle() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../harness/detection/fixtures/audio-effects-chrome155.json"
        ))
        .unwrap();
        let input: Vec<f32> = fixture["input"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_f64().unwrap() as f32)
            .collect();
        let cases: [(&str, &str, &[f32], &[f32]); 5] = [
            ("lowpass", "biquad:lowpass", &[1200.0, 0.707, 0.0, 0.0], &[]),
            ("highpass", "biquad:highpass", &[800.0, 2.0, 0.0, 0.0], &[]),
            (
                "waveshaper",
                "waveshaper",
                &[],
                &[-1.0, -0.2, 0.0, 0.2, 1.0],
            ),
            ("delay", "delay", &[0.003, 1.0], &[]),
            ("pan", "panner", &[0.4], &[]),
        ];
        for (name, kind, parameters, curve) in cases {
            let mut effect = Effect::new(kind, 44_100.0, parameters, curve).unwrap();
            effect.set_sample_accurate(true);
            let mut output = [Vec::new(), Vec::new()];
            for start in (0..2048).step_by(QUANTUM) {
                effect
                    .set_input_silent(start >= input.len(), start as f64 / 44_100.0)
                    .unwrap();
                let mono = if start < input.len() {
                    input[start..start + QUANTUM].to_vec()
                } else {
                    vec![0.0; QUANTUM]
                };
                let mut channels = [mono.clone(), mono];
                effect.process(&mut channels, 1, &[]).unwrap();
                for (result, samples) in output.iter_mut().zip(&channels) {
                    result.extend_from_slice(samples);
                }
            }
            for (index, expected) in fixture["graphs"][name]["pcm"]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
            {
                let expected: Vec<f32> = expected
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|value| value.as_f64().unwrap() as f32)
                    .collect();
                assert_eq!(output[index], expected, "{name} channel {index}");
            }
        }
    }

    #[test]
    fn native_effect_state_validates_memory_bounds_and_retains_delay_history() {
        assert!(Effect::new("biquad:unknown", 44_100.0, &[1.0; 4], &[]).is_err());
        assert!(Effect::new("delay", 44_100.0, &[0.0, 180.0], &[]).is_err());
        assert!(Effect::new("waveshaper", 44_100.0, &[], &[0.0]).is_err());
        assert!(Effect::new("waveshaper", 44_100.0, &[], &vec![0.0; MAX_CURVE + 1]).is_err());
        assert!(Effect::new("panner", f32::NAN, &[0.0], &[]).is_err());
        // A power-of-two rate represents this delay exactly as an AudioParam
        // float; 128/44100 rounds slightly early and interpolates across quanta.
        let mut delay = Effect::new("delay", 32_768.0, &[128.0 / 32_768.0, 1.0], &[]).unwrap();
        let mut impulse = [vec![0.0; QUANTUM], vec![0.0; QUANTUM]];
        impulse[0][0] = 1.0;
        impulse[1][0] = 1.0;
        delay.process(&mut impulse, 1, &[]).unwrap();
        assert!(impulse.iter().flatten().all(|sample| *sample == 0.0));
        let mut tail = [vec![0.0; QUANTUM], vec![0.0; QUANTUM]];
        delay.process(&mut tail, 1, &[]).unwrap();
        assert_eq!(tail[0][0], 1.0);
        assert!(delay
            .process(&mut [vec![0.0; 127], vec![0.0; 128]], 1, &[])
            .is_err());
        assert!(delay.process(&mut tail, 0, &[]).is_err());
        assert!(delay.process(&mut tail, 1, &[vec![0.0; 127]]).is_err());
        delay.set_sample_accurate(true);
        delay
            .process(&mut tail, 1, &[vec![f32::INFINITY; QUANTUM]])
            .unwrap();
        assert!(!delay.output_silent());
    }

    #[test]
    fn effects_reject_unknown_kinds_bad_parameters_and_clocks() {
        let message = |result: Result<Effect, String>| result.err().unwrap();
        assert_eq!(
            message(Effect::new("convolver", 44_100.0, &[], &[])),
            "Unsupported native audio effect"
        );
        assert_eq!(
            message(Effect::new("panner", 44_100.0, &[f32::NAN], &[])),
            "Invalid native effect parameters"
        );
        assert_eq!(
            message(Effect::new("biquad:lowpass", 44_100.0, &[1.0; 3], &[])),
            "Invalid native effect parameters"
        );
        assert_eq!(
            Effect::frequency_response("delay", 44_100.0, &[0.0], &[100.0])
                .err()
                .unwrap(),
            "Invalid or oversized native frequency response"
        );
        assert_eq!(
            Effect::frequency_response(
                "biquad:lowpass",
                44_100.0,
                &[350.0, 0.0, 1.0, 0.0],
                &vec![1.0; MAX_CURVE + 1]
            )
            .err()
            .unwrap(),
            "Invalid or oversized native frequency response"
        );
        let mut panner = Effect::new("panner", 44_100.0, &[0.0], &[]).unwrap();
        assert_eq!(panner.output_channels(1), 2);
        for clock in [f64::NAN, -1.0, f64::INFINITY] {
            assert_eq!(
                panner.set_input_silent(true, clock).err().unwrap(),
                "Invalid native effect clock"
            );
        }
        assert!(panner.set_input_silent(false, 0.0).is_ok());
        let waveshaper = Effect::new("waveshaper", 44_100.0, &[], &[]).unwrap();
        assert_eq!(waveshaper.output_channels(1), 1);
    }

    #[test]
    fn a_nan_delay_time_uses_the_maximum_delay() {
        // DelayNode: NaN delayTime samples are replaced by the node's maximum
        // (as Chrome clamps a non-finite computed value to maxDelayTime).
        let rate = 32_768.0;
        let maximum = 128.0 / rate;
        let mut delay = Effect::new("delay", rate, &[0.0, maximum], &[]).unwrap();
        let mut impulse = [vec![0.0; QUANTUM], vec![0.0; QUANTUM]];
        impulse[0][0] = 1.0;
        delay
            .process(&mut impulse, 1, &[vec![f32::NAN; QUANTUM]])
            .unwrap();
        assert!(impulse[0].iter().all(|sample| *sample == 0.0));
        let mut tail = [vec![0.0; QUANTUM], vec![0.0; QUANTUM]];
        delay
            .process(&mut tail, 1, &[vec![f32::NAN; QUANTUM]])
            .unwrap();
        assert_eq!(tail[0][0], 1.0);
    }
}
