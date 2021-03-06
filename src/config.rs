use std::{fs, path::Path, result};

use chrono::{DateTime, Duration, FixedOffset, Local, TimeZone};
use dirs;
use serde::Deserialize;
use structopt::StructOpt;

use super::{
    enums,
    errors::{ConfigErrorKind, HeliocronError},
    parsers, structs,
};

type Result<T> = result::Result<T, HeliocronError>;

#[derive(Debug, StructOpt)]
#[structopt(
    about = "A simple utility for finding out what time various solar events occur, such as sunrise and \
             sunset, at a given location on a given date. It can be integrated into cron commands to \
             trigger program execution relative to these events.\n\n\
             For example, to execute a script 'turn-on-lights.sh' at sunrise, make a Crontab entry to trigger \
             at a time that will always be before the chosen event (say, 2am) and use heliocron to calculate \
             and perform the appropriate delay:\n\n\
             \t0 2 * * * heliocron --latitude 51.47N --longitude 3.1W wait --event sunrise && turn-on-lights.sh"
)]
struct Cli {
    #[structopt(subcommand)]
    subcommand: Subcommand,

    #[structopt(flatten)]
    date_args: DateArgs,

    // the default values for latitude and longitude are handled differently to enable the user to set the values
    // either on the command line, in a config file or have a default provided by the program
    #[structopt(
        short = "l",
        long = "latitude",
        help = "Set the latitude in decimal degrees. Can also be set in ~/.config/heliocron.toml. [default: 51.4769N]",
        requires = "longitude"
    )]
    latitude: Option<String>,

    #[structopt(
        short = "o",
        long = "longitude",
        help = "Set the longitude in decimal degrees. Can also be set in ~/.config/heliocron.toml. [default: 0.0005W]",
        requires = "latitude"
    )]
    longitude: Option<String>,
}

#[derive(Debug, StructOpt)]
pub enum Subcommand {
    Report {},

    Wait {
        #[structopt(
            help = "Choose a delay from your chosen event (see --event) in one of the following formats: {HH:MM:SS | HH:MM}. You may prepend the delay with '-' to make it negative. A negative offset will set the delay to be before the event, whilst a positive offset will set the delay to be after the event.",
            short = "o",
            long = "offset",
            default_value = "00:00:00",
            parse(from_str=parsers::parse_offset),
            allow_hyphen_values = true,
        )]
        offset: Result<Duration>,

        #[structopt(
            help = "Choose an event from which to base your delay.", 
            short = "e", 
            long = "event", 
            parse(from_str=parsers::parse_event),
            possible_values = &["sunrise", "sunset", "civil_dawn", "civil_dusk", "nautical_dawn", "nautical_dusk", "astronomical_dawn", "astronomical_dusk"]
        )]
        event: Result<enums::Event>,
    },
}

#[derive(Debug, StructOpt)]
struct DateArgs {
    #[structopt(short = "d", long = "date")]
    date: Option<String>,

    #[structopt(short = "f", long = "date-format", default_value = "%Y-%m-%d")]
    date_format: String,

    #[structopt(short = "t", long = "time-zone", allow_hyphen_values = true)]
    time_zone: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TomlConfig {
    latitude: Option<String>,
    longitude: Option<String>,
}

impl TomlConfig {
    fn new() -> TomlConfig {
        TomlConfig {
            latitude: None,
            longitude: None,
        }
    }

    fn from_toml(config: result::Result<TomlConfig, toml::de::Error>) -> TomlConfig {
        match config {
            Ok(conf) => conf,
            _ => TomlConfig::new(),
        }
    }
}

#[derive(Debug)]
pub struct Config {
    pub coordinates: structs::Coordinates,
    pub date: DateTime<FixedOffset>,
    pub subcommand: Option<Subcommand>,
    pub event: Option<enums::Event>,
}

impl Config {
    fn merge_toml(mut self, toml_config: TomlConfig) -> Result<Config> {
        if let (Some(latitude), Some(longitude)) = (toml_config.latitude, toml_config.longitude) {
            self.coordinates = structs::Coordinates::from_decimal_degrees(&latitude, &longitude)?
        }
        Ok(self)
    }

    fn merge_cli_args(mut self, cli_args: Cli) -> Result<Config> {
        // merge in location if set. Structopt requires either both or neither of lat and long to be set
        if let (Some(latitude), Some(longitude)) = (cli_args.latitude, cli_args.longitude) {
            self.coordinates = structs::Coordinates::from_decimal_degrees(&latitude, &longitude)?
        }

        // set the date
        let date_args = cli_args.date_args;
        if let Some(date) = date_args.date {
            self.date = parsers::parse_date(
                &date,
                &date_args.date_format,
                date_args.time_zone.as_deref(),
            )?;
        }

        // set the subcommand to execute
        self.subcommand = Some(cli_args.subcommand);

        Ok(self)
    }
}

pub fn get_config() -> Result<Config> {
    // master function for collecting all config variables and returning a single runtime configuration

    // 0. Set up default config
    let default_config = Config {
        coordinates: structs::Coordinates::from_decimal_degrees("51.4769N", "0.0005W")?,
        date: Local::today()
            .and_hms(12, 0, 0)
            .with_timezone(&FixedOffset::from_offset(Local::today().offset())),
        subcommand: None,
        event: None,
    };

    // 1. Overwrite defaults with config from ~/.config/heliocron.toml if present

    let config: Config = if cfg!(feature = "integration-test") {
        default_config
    } else {
        let path = dirs::config_dir()
            .unwrap() // this shouldn't ever really be None?
            .join(Path::new("heliocron.toml"));

        let file = fs::read_to_string(path);

        let config: Config = match file {
            Ok(f) => match default_config.merge_toml(TomlConfig::from_toml(toml::from_str(&f))) {
                Ok(merged_config) => Ok(merged_config),
                // any errors parsing the .toml raise an error
                Err(_) => Err(HeliocronError::Config(ConfigErrorKind::InvalidTomlFile)),
            },
            // any problems opening the .toml file and we just continue on with the default configuration
            Err(_) => Ok(default_config),
        }?;

        config
    };
    // if we are running integration tests, we actually just want to use the default config

    // 2. Overwrite any currently set config with CLI arguments
    let cli_args = Cli::from_args();

    let config = config.merge_cli_args(cli_args)?;

    Ok(config)
}
