pub fn time_zone() -> Result<chrono_tz::Tz, &'static str> {
    std::env::var("LAVEMILL_TIME_ZONE")
        .unwrap_or_else(|_| "Europe/London".into())
        .parse()
        .map_err(|_| "Invalid household time zone.")
}
