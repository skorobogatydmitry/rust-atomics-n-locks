use std::env::args;
use std::str::FromStr;
use std::string::ToString;

use strum::IntoEnumIterator;
use strum_macros::{self, Display, EnumIter, EnumString};

fn main() -> Result<(), String> {
    args()
        .nth(1)
        .ok_or(format!(
            "no chapter supplied, use one of {} or see unit tests",
            Chapter::iter()
                .map(|c| c.to_string())
                .collect::<Vec<String>>()
                .join(",")
        ))
        .and_then(|selector| {
            Chapter::from_str(&selector)
                .map(|chapter| match chapter {
                    Chapter::One => atomics_n_locks::ch1_basic_concurrency::run(),
                    Chapter::Two => atomics_n_locks::ch2_atomics::run(),
                    Chapter::Three => atomics_n_locks::ch3_memory_ordering::run(),
                    Chapter::Four => atomics_n_locks::ch4_building_our_own_spin_lock::run(),
                    Chapter::Five => atomics_n_locks::ch5_building_our_own_channels::run(),
                    Chapter::Six => println!("see unit tests within the ch6 module"),
                    Chapter::Seven => atomics_n_locks::ch7_understanding_the_processor::cache(),
                })
                .map_err(|e| e.to_string())
        })
}

#[derive(EnumIter, EnumString, Display)]
#[strum(serialize_all = "snake_case")]
enum Chapter {
    One,
    Two,
    Three,
    Four,
    Five,
    Six,
    Seven,
}
