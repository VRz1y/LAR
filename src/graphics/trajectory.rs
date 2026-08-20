use crate::lifecycle::InputEvent;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrajectoryPoint {
    pub x: f64,
    pub y: f64,
}

pub fn bezier_trajectory(
    start: (f64, f64),
    control1: (f64, f64),
    control2: (f64, f64),
    end: (f64, f64),
    steps: usize,
    seed: u64,
    noise_amplitude: f64,
) -> Vec<TrajectoryPoint> {
    if steps == 0 {
        return vec![TrajectoryPoint {
            x: start.0,
            y: start.1,
        }];
    }
    let mut rng = seed;
    (0..=steps)
        .map(|index| {
            let t = index as f64 / steps as f64;
            let u = 1.0 - t;
            let base = (
                u * u * u * start.0
                    + 3.0 * u * u * t * control1.0
                    + 3.0 * u * t * t * control2.0
                    + t * t * t * end.0,
                u * u * u * start.1
                    + 3.0 * u * u * t * control1.1
                    + 3.0 * u * t * t * control2.1
                    + t * t * t * end.1,
            );
            let noise = if index == 0 || index == steps {
                (0.0, 0.0)
            } else {
                (
                    next_noise(&mut rng, noise_amplitude),
                    next_noise(&mut rng, noise_amplitude),
                )
            };
            TrajectoryPoint {
                x: base.0 + noise.0,
                y: base.1 + noise.1,
            }
        })
        .collect()
}

pub fn trajectory_touch_events(points: &[TrajectoryPoint], pressed: bool) -> Vec<InputEvent> {
    points
        .iter()
        .enumerate()
        .map(|(index, point)| InputEvent::Touch {
            x: (point.x.round() as i64).clamp(i32::MIN as i64, i32::MAX as i64) as i32,
            y: (point.y.round() as i64).clamp(i32::MIN as i64, i32::MAX as i64) as i32,
            pressed: if index + 1 == points.len() {
                false
            } else {
                pressed
            },
        })
        .collect()
}

fn next_noise(state: &mut u64, amplitude: f64) -> f64 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    let unit = ((*state >> 11) as f64) / ((1u64 << 53) as f64);
    (unit * 2.0 - 1.0) * amplitude
}
