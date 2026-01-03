use nalgebra_glm::Vec2;

#[derive(Clone, Copy, Debug)]
pub enum Direction {
    NegX,
    PosX,
    NegY,
    PosY,
}
impl Direction {
    pub fn vec2(self) -> Vec2 {
        match self {
            Direction::NegX => Vec2::new(-1.0, 0.0),
            Direction::PosX => Vec2::new(1.0, 0.0),
            Direction::NegY => Vec2::new(0.0, -1.0),
            Direction::PosY => Vec2::new(0.0, 1.0),
        }
    }

    pub fn closest_to(vec: Vec2) -> Direction {
        if vec.x.abs() >= vec.y.abs() {
            if vec.x.is_sign_negative() {
                Direction::NegX
            } else {
                Direction::PosX
            }
        } else {
            if vec.y.is_sign_negative() {
                Direction::NegY
            } else {
                Direction::PosY
            }
        }
    }
}
