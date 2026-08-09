#[warn(clippy::useless_vec)]
pub fn value() -> u8 {
    let values = [2_u8];
    values[0]
}
