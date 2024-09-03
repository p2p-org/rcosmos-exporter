// use crate::config::Settings;

#[macro_export]
macro_rules! MessageLog {
    ($level:expr, $message:expr) => {
        {
            let settings = Settings::new().unwrap();

            if settings.logging_level == "DEBUG"
                || (settings.logging_level == "INFO" && ($level == "INFO" || $level == "ERROR"))
                || (settings.logging_level == "ERROR" && $level == "ERROR")
            {
                println!(
                    "{{ \"type\": \"{}\", \"message\": \"{}\" }}", // Output type first
                    $level,
                    $message
                );
            }
        }
    };
    ($level:expr, $fmt:expr, $($arg:tt)*) => {
        {
            let settings = Settings::new().unwrap();

            if settings.logging_level == "DEBUG"
                || (settings.logging_level == "INFO" && ($level == "INFO" || $level == "ERROR"))
                || (settings.logging_level == "ERROR" && $level == "ERROR")
            {
                println!(
                    "{{ \"type\": \"{}\", \"message\": \"{}\" }}", // Output type first
                    $level,
                    format!($fmt, $($arg)*)
                );
            }
        }
    };
}
