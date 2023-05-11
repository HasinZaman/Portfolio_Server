use action::{action_boi, generate_utility_thread};
use logging::logger_init;
use pipelined_server::{
    pipeline::{
        builder::pipeline::Builder,
        default::{
            self,
            action::{generate_read_only_file_utility_thread, NO_BOUND},
        },
        Server,
    },
    setting::ServerSetting,
};

mod action;
mod logging;
//mod post_logic;
//mod sql_reader;
//mod get_logic;

fn main() {
    logger_init();

    let utility_thread = generate_utility_thread();

    let setting = ServerSetting::load();

    println!("{setting:#?}");

    let builder = Builder::default()
        .set_settings(setting.clone())
        .set_parser(default::parser::<264, 1024>)
        .set_action(action_boi)
        .set_compression(default::no_compression)
        .set_utility_thread(utility_thread.0.clone());

    let server = Server::new(setting, utility_thread, builder);

    server.run::<1>();
}
