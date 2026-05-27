use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stroke {
    pub id: String,
    pub points: Vec<Point>,
    pub color: (u8, u8, u8),
    pub width: f64,
}

impl Stroke {
    pub fn new(color: (u8, u8, u8), width: f64) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            points: Vec::new(),
            color,
            width,
        }
    }

    pub fn add_point(&mut self, point: Point) {
        self.points.push(point);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stroke_new_has_unique_ids() {
        let s1 = Stroke::new((255, 0, 0), 2.0);
        let s2 = Stroke::new((0, 255, 0), 3.0);
        assert_ne!(s1.id, s2.id);
    }

    #[test]
    fn test_stroke_add_points() {
        let mut s = Stroke::new((0, 0, 255), 1.5);
        assert!(s.points.is_empty());

        s.add_point(Point { x: 1.0, y: 2.0 });
        s.add_point(Point { x: 3.0, y: 4.0 });
        assert_eq!(s.points.len(), 2);
        assert_eq!(s.points[0].x, 1.0);
        assert_eq!(s.points[1].y, 4.0);
    }

    #[test]
    fn test_stroke_id_is_valid_uuid() {
        let s = Stroke::new((0, 0, 0), 1.0);
        assert!(uuid::Uuid::parse_str(&s.id).is_ok());
    }
}
