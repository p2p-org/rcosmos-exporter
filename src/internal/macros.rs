#[macro_export]
macro_rules! MessageLog {
    ($message:expr) => {
        println!("{:?}", JsonLog {
            message: format!("{}", $message),
        });
    };
    ($fmt:expr, $($arg:tt)*) => {
        println!("{:?}", JsonLog {
            message: format!($fmt, $($arg)*),
        });
    };
}
