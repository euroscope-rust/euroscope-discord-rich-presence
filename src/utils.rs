use chrono::Utc;

#[inline]
pub fn now() -> i64 {
    Utc::now().timestamp_millis()
}
