//! A tiny 2-D vector. Combat is 2-D to match the galaxy's 2-D coordinates; a
//! later model may promote this to 3-D.

use std::ops::{Add, Mul, Sub};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Vec2 {
    pub x: f64,
    pub y: f64,
}

impl Vec2 {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    pub fn length(self) -> f64 {
        (self.x * self.x + self.y * self.y).sqrt()
    }

    pub fn distance(self, v: Vec2) -> f64 {
        (self - v).length()
    }

    /// Unit vector in the same direction, or zero for a zero vector.
    pub fn normalized(self) -> Vec2 {
        let len = self.length();
        if len > 0.0 {
            self * (1.0 / len)
        } else {
            Vec2::default()
        }
    }

    /// Rescale to at most `max` length.
    pub fn clamp_length(self, max: f64) -> Vec2 {
        let len = self.length();
        if len > max && len > 0.0 {
            self * (max / len)
        } else {
            self
        }
    }
}

impl Add for Vec2 {
    type Output = Vec2;
    fn add(self, v: Vec2) -> Vec2 {
        Vec2::new(self.x + v.x, self.y + v.y)
    }
}

impl Sub for Vec2 {
    type Output = Vec2;
    fn sub(self, v: Vec2) -> Vec2 {
        Vec2::new(self.x - v.x, self.y - v.y)
    }
}

impl Mul<f64> for Vec2 {
    type Output = Vec2;
    fn mul(self, s: f64) -> Vec2 {
        Vec2::new(self.x * s, self.y * s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_ops() {
        let a = Vec2::new(3.0, 4.0);
        assert_eq!(a.length(), 5.0);
        assert_eq!(a.distance(Vec2::new(0.0, 0.0)), 5.0);
        assert_eq!(a.normalized().length(), 1.0);
        assert_eq!(a * 2.0, Vec2::new(6.0, 8.0));
        assert_eq!(a - Vec2::new(1.0, 1.0), Vec2::new(2.0, 3.0));
        assert_eq!(a + Vec2::new(1.0, 1.0), Vec2::new(4.0, 5.0));
    }

    #[test]
    fn clamp_length_caps_magnitude() {
        let a = Vec2::new(10.0, 0.0).clamp_length(4.0);
        assert!((a.length() - 4.0).abs() < 1e-9);
        // Already-short vectors are untouched.
        let b = Vec2::new(1.0, 0.0).clamp_length(4.0);
        assert_eq!(b, Vec2::new(1.0, 0.0));
    }

    #[test]
    fn zero_vector_normalizes_to_zero() {
        assert_eq!(Vec2::default().normalized(), Vec2::default());
    }
}
