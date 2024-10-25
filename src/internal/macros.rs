#[macro_export]
macro_rules! MessageLog {
    ($level:expr, $message:expr) => {
        {
            use chrono::Local;
            let settings = Settings::new().unwrap();
            let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

            if settings.logging_level == "DEBUG"
                || (settings.logging_level == "INFO" && ($level == "INFO" || $level == "ERROR"))
                || (settings.logging_level == "ERROR" && $level == "ERROR")
            {
                println!(
                    "{{ \"timestamp\": \"{}\", \"type\": \"{}\", \"message\": \"{}\" }}",
                    timestamp,
                    $level,
                    $message
                );
            }
        }
    };
    ($level:expr, $fmt:expr, $($arg:tt)*) => {
        {
            use chrono::Local;
            let settings = Settings::new().unwrap();
            let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

            if settings.logging_level == "DEBUG"
                || (settings.logging_level == "INFO" && ($level == "INFO" || $level == "ERROR"))
                || (settings.logging_level == "ERROR" && $level == "ERROR")
            {
                println!(
                    "{{ \"timestamp\": \"{}\", \"type\": \"{}\", \"message\": \"{}\" }}",
                    timestamp,
                    $level,
                    format!($fmt, $($arg)*)
                );
            }
        }
    };
}
