#[warn(clippy::useless_vec)]
pub fn value() -> u8 {
    let values = [1_u8];
    values[0]
}
