#[warn(clippy::ptr_arg)]
pub fn first(values: &Vec<u8>) -> Option<&u8> {
    values.first()
}
