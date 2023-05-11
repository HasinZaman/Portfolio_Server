use log::LevelFilter;
use log4rs::{
    append::{console::ConsoleAppender, file::FileAppender},
    config::{Appender, Root},
    encode::pattern::PatternEncoder,
    Config, Handle,
};

const LOG_FILE_TAG: &str = "log_file";
const STD_OUT: &str = "stdout";

pub fn logger_init() -> Handle {
    let log_file = FileAppender::builder()
        .encoder(Box::new(PatternEncoder::new("{d} {l} {t} - {m}{n}\n")))
        .build(".log")
        .unwrap();

    let stdout = ConsoleAppender::builder().build();

    let config = Config::builder()
        .appender(Appender::builder().build(STD_OUT, Box::new(stdout)))
        .appender(Appender::builder().build(LOG_FILE_TAG, Box::new(log_file)))
        .build(
            Root::builder()
                .appender(LOG_FILE_TAG)
                .appender(STD_OUT)
                .build(LevelFilter::max_level()),
        )
        .unwrap();

    return log4rs::init_config(config).unwrap();
}
