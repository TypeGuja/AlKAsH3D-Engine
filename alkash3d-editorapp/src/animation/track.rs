use super::interpolatable::Interpolatable;

#[derive(Debug, Clone, Copy)]
pub enum EasingType { Linear, EaseIn, EaseOut, EaseInOut }

#[derive(Debug, Clone)]
pub struct Keyframe<T: Clone> {
    pub time: f32,
    pub value: T,
    pub easing: EasingType,
}

#[derive(Debug, Clone)]
pub struct AnimationTrack<T: Clone + Interpolatable> {
    pub keyframes: Vec<Keyframe<T>>,
    pub looped: bool,
}

impl<T: Clone + Interpolatable> AnimationTrack<T> {
    pub fn new() -> Self {
        Self { keyframes: Vec::new(), looped: false }
    }

    pub fn add_keyframe(&mut self, time: f32, value: T, easing: EasingType) {
        self.keyframes.push(Keyframe { time, value, easing });
        self.keyframes.sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap());
    }

    pub fn evaluate(&self, time: f32) -> Option<T> {
        if self.keyframes.is_empty() { return None; }

        let max_time = self.keyframes.last().unwrap().time;
        let time = if self.looped { time % max_time } else { time.min(max_time) };

        if time <= self.keyframes[0].time {
            return Some(self.keyframes[0].value.clone());
        }

        for i in 0..self.keyframes.len() - 1 {
            let k1 = &self.keyframes[i];
            let k2 = &self.keyframes[i + 1];

            if time >= k1.time && time <= k2.time {
                let t = (time - k1.time) / (k2.time - k1.time);
                let t = match k1.easing {
                    EasingType::Linear => t,
                    EasingType::EaseIn => t * t,
                    EasingType::EaseOut => 1.0 - (1.0 - t) * (1.0 - t),
                    EasingType::EaseInOut => {
                        if t < 0.5 { 2.0 * t * t }
                        else { 1.0 - (-2.0 * t + 2.0).powi(2) / 2.0 }
                    }
                };
                return Some(k1.value.interpolate(&k2.value, t));
            }
        }

        Some(self.keyframes.last().unwrap().value.clone())
    }
}