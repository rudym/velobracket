use clap::{App, Arg};

pub fn read_arguments() -> clap::ArgMatches<'static> {
    App::new("VeloRLTK")
        .version("0.0.1")
        .author("Rodion Martynov <rmartynov@gmail.com>")
        .about("Veloren client frontend on RLTK")
        .arg(
            Arg::with_name("username")
                .long("username")
                .value_name("USERNAME")
                .help("Set the username used to log in")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("password")
                .long("password")
                .value_name("PASSWORD")
                .help("Set the password to log in with")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("server")
                .long("server")
                .value_name("SERVER_ADDR")
                .help("Set the server address")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("port")
                .long("port")
                .value_name("PORT")
                .help("Set the server port")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("character")
                .long("character")
                .value_name("CHARACTER")
                .help("Select the character to play")
                .required(true)
                .takes_value(true),
        )
        .get_matches()
}
