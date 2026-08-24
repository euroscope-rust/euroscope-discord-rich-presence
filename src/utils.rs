use jiff::Timestamp;

#[inline]
pub fn now() -> i64 {
    Timestamp::now().as_millisecond()
}
