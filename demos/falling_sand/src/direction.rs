use nalgebra_glm::{I16Vec2, Vec2};

#[derive(Clone, Copy, Debug)]
pub enum Direction {
    NegX,
    PosX,
    NegY,
    PosY,
}
impl Direction {
    pub const fn vec2(self) -> Vec2 {
        match self {
            Direction::NegX => Vec2::new(-1.0, 0.0),
            Direction::PosX => Vec2::new(1.0, 0.0),
            Direction::NegY => Vec2::new(0.0, -1.0),
            Direction::PosY => Vec2::new(0.0, 1.0),
        }
    }

    pub const fn i16vec2(self) -> I16Vec2 {
        match self {
            Direction::NegX => I16Vec2::new(-1, 0),
            Direction::PosX => I16Vec2::new(1, 0),
            Direction::NegY => I16Vec2::new(0, -1),
            Direction::PosY => I16Vec2::new(0, 1),
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

#[derive(Clone, Copy, Debug)]
pub enum CornerDirection {
    NegXNegY,
    PosXNegY,
    NegXPosY,
    PosXPosY,
}
impl CornerDirection {
    pub const fn i16vec2(self) -> I16Vec2 {
        match self {
            CornerDirection::NegXNegY => I16Vec2::new(-1, -1),
            CornerDirection::PosXNegY => I16Vec2::new(1, -1),
            CornerDirection::NegXPosY => I16Vec2::new(-1, 1),
            CornerDirection::PosXPosY => I16Vec2::new(1, 1),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum EightDirection {
    Axis(Direction),
    Corner(CornerDirection),
}
impl EightDirection {
    pub const fn i16vec2(self) -> I16Vec2 {
        match self {
            EightDirection::Axis(it) => it.i16vec2(),
            EightDirection::Corner(it) => it.i16vec2(),
        }
    }

    // pub const fn cw(self) -> Self {
    //     match self {
    //         EightDirection::Axis(Direction::NegX) => EightDirection::Corner(CornerDirection::NegXNegY),
    //         EightDirection::Corner(CornerDirection::NegXNegY) => EightDirection::Corner(CornerDirection::NegXPosY),
    //     }
    // }
}

impl From<Direction> for EightDirection {
    fn from(value: Direction) -> Self {
        Self::Axis(value)
    }
}

impl From<CornerDirection> for EightDirection {
    fn from(value: CornerDirection) -> Self {
        Self::Corner(value)
    }
}