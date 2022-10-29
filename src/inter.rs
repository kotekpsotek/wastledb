pub const MAXIMUM_RESPONSE_SIZE_BYTES: usize = 128;
pub const MAXIMUM_REQUEST_SIZE_BYTES: usize = 8 * 1024;
pub const TCP_PORT: u16 = 20050;
pub const MAXIMUM_SESSION_LIVE_TIME_MILS: u128 = 1_000 * 60 * 1; // 1 minute in milliseconds // this value is added to session generation timestamp for calucate session live in trashold behind which session expired