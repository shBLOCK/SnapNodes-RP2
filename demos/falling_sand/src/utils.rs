// use nalgebra_glm::{vec2, I16Vec2, Vec2};
// use nalgebra_glm as glm;
// use crate::direction::Direction;
//
// fn f32_offset(value: f32, offset: i32) -> f32 {
//     let mut bits = value.to_bits();
//     if value.is_sign_negative() {
//         bits -= offset as u32;
//     } else {
//         bits += offset as u32;
//     }
//     f32::from_bits(bits)
// }
//
// struct MarchGridResult {
//     pos: Vec2,
//     major_move_direction: Direction,
//     major_move_dest: f32,
//     major_move_delta: f32,
//     minor_major_ratio: f32,
//     signum: Vec2,
//     signum_i16: I16Vec2,
// }
//
// fn march_grid(pos: Vec2, delta: Vec2) -> MarchGridResult {
//     // use this to flip the space so that delta has positive elements (_pe)
//     let signum = delta.map(f32::signum);
//     let signum_i16 = glm::convert_unchecked::<_, I16Vec2>(signum);
//     let mut pos_pe = pos.component_mul(&signum);
//     let mut delta_pe = delta.component_mul(&signum);
//     let ceil_pos_pe = glm::ceil(&pos_pe);
//     let pos_to_ceil_pos_pe = ceil_pos_pe - pos_pe;
//     // core::assert!(pos_to_ceil_pos_pe.x != 0.0 && pos_to_ceil_pos_pe.y != 0.0);
//     // core::assert!(!delta_pe.is_zero(), "delta_pe: {:?}, old: {:?}, simple: {:?}", delta_pe, old_slot_pos, simple_move_slot_pos);
//     let step_len_xy_pe = pos_to_ceil_pos_pe.component_div(&delta_pe);
//     // core::assert!(!step_len_xy_pe.iter().all(|it| !it.is_finite()));
//     if step_len_xy_pe.x <= step_len_xy_pe.y {
//         // should step in x direction
//         let step_y_pe = delta_pe.y * step_len_xy_pe.x;
//         // core::assert!(step_y_pe.is_finite(), "{:?} {:?} {:?}", step_len_xy_pe, pos_to_ceil_pos_pe, delta_pe);
//         pos_pe.y += step_y_pe;
//         delta_pe.y -= step_y_pe;
//         delta_pe.x -= pos_to_ceil_pos_pe.x;
//         pos_pe.x = ceil_pos_pe.x;
//     } else {
//         // should step in y direction
//         let step_x_pe = delta_pe.x * step_len_xy_pe.y;
//         // core::assert!(step_x_pe.is_finite());
//         pos_pe.x += step_x_pe;
//         delta_pe.x -= step_x_pe;
//         delta_pe.y -= pos_to_ceil_pos_pe.y;
//         pos_pe.y = ceil_pos_pe.y;
//     }
//
//     pos = pos_pe.component_mul(&signum);
//     delta = delta_pe.component_mul(&signum);
// }