#[warn(clippy::useless_vec)]
pub fn value() -> u8 {
    let values = vec![1_u8];
    values[0]
}
