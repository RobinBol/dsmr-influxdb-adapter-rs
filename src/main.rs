extern crate mio;
extern crate mio_serial;

use chrono::prelude::NaiveDateTime;
use reqwest;
use tokio;

use mio::unix::UnixReady;
use mio::{Events, Poll, PollOpt, Ready, Token};
use std::io;
use std::io::Read;
use std::str;
use std::time::Duration;

const SERIAL_TOKEN: Token = Token(0);
const DEFAULT_TTY: &str = "/dev/ttyUSB0";
const INFLUX_DB_URI: &str = "http://localhost:8086/write?db=p1meter";

/**
 * This adapter POSTs the following measurements to InfluxDB
 * - currentTariff - 1 or 2
 * - wattUsage - Current usage in Watt
 * - wattUsageAccumulative - Current accumulative usage in kWh (sum of both tariffs)
 * - wattProduction - Current production in Watt
 * - wattProductionAccumulative - Current accumulative produced in kWh (sum of both tariffs)
 * - wattNett - Current nett power consumption in Watt (production minus usage)
 * - wattAccumulativeNett - Current accumulative nett power consumption in kWh (production minus usage)
 * - gasUsageAccumulative - Current accumulative gas usage in m3
 */

// Post a measurement to InfluxDB
async fn post_influx_db(client: &reqwest::Client, key: &str, value: f32) {
    let request = format!("{},host=pi,region=eu-west value={}", key, value);
    println!("InfluxDB POST: {} {}", INFLUX_DB_URI, request);

    // Send request to InfluxDB
    let response = client
        .post(&INFLUX_DB_URI.to_string())
        .body(request)
        .send()
        .await;

    // Handle success and error response
    match response {
        Ok(_response) => {
            // Print if unexpected status code is received as response
            if _response.status().to_string() != "204 No Content" {
                println!("InfluxDB POST: Error Status: {}", _response.status());
            }
        }
        Err(_err) => println!("Request error: {}", _err),
    }
}

// Read the telegram until a provided id is found, parse the values belonging to that id and return
fn get_values_by_id<'a>(id: &'a str, telegram: &'a str) -> Result<Vec<&'a str>, &'static str> {
    let vector_telegram_lines: Vec<&str> = telegram.lines().collect();
    let index_of_item = vector_telegram_lines
        .iter()
        .position(|x| x.starts_with(id) == true);

    // Check if item was found
    match index_of_item {
        Some(_index_of_item) => {
            // Parse the values from this line (between ())
            let mut values: Vec<&str> = vector_telegram_lines
                .get(_index_of_item)
                .unwrap()
                .split(|x| x == '(' || x == ')')
                .filter(|x| x.len() != 0)
                .collect();

            // Remove the id from the string
            values.remove(0);

            // If values are present return them
            if values.len() != 0 {
                return Ok(values);
            }
            return Err("Values not found");
        }
        None => return Err("Index not found"),
    }
}

// Parse timestamp from telegram
async fn parse_timestamp(telegram: &str) -> Result<i64, &'static str> {
    let values = get_values_by_id("0-0:1.0.0", &telegram)?;
    let timestamp = values.get(0);
    match timestamp {
        Some(_timestamp) => Ok(NaiveDateTime::parse_from_str(
            &_timestamp.replace("W", ""),
            "%y%m%d%H%M%S",
        )
        .unwrap()
        .timestamp()),
        None => Err("Could not read timestamp"),
    }
}

// Parse current Watt usage
async fn parse_w_usage(telegram: &str) -> Result<f32, &'static str> {
    let values = get_values_by_id("1-0:1.7.0", &telegram)?;
    let value = values.get(0);
    match value {
        Some(_value) => {
            let mut a = _value
                .replace("*kW", "")
                .parse()
                .expect("Parse Watt usage string to f32");
            a = a * 1000.0; // kW -> W
            Ok(a)
        }
        None => Err("Could not read Watt usage"),
    }
}

// Parse current accumulative Watt usage
async fn parse_w_usage_accumulative(telegram: &str) -> Result<f32, &'static str> {
    // Get tariff 1 usage
    let values_tariff_1 = get_values_by_id("1-0:1.8.1", &telegram)?;
    let value_tariff_1 = values_tariff_1.get(0);

    // Get tariff 2 usage
    let values_tariff_2 = get_values_by_id("1-0:1.8.2", &telegram)?;
    let value_tariff_2 = values_tariff_2.get(0);

    // If both are found, parse, add and return them
    match value_tariff_1 {
        Some(_value_tariff_1) => {
            let _value_tariff_1_parsed: f32 = _value_tariff_1
                .replace("*kWh", "")
                .parse()
                .expect("Parse Watt usage accumulative tariff 1 string to f32");

            match value_tariff_2 {
                Some(_value_tariff_2) => {
                    let _value_tariff_2_parsed: f32 = _value_tariff_2
                        .replace("*kWh", "")
                        .parse()
                        .expect("Parse Watt usage accumulative tariff 2 string to f32");
                    Ok(_value_tariff_1_parsed + _value_tariff_2_parsed)
                }
                None => Err("Could not read Watt usage accumulative tariff 2"),
            }
        }
        None => Err("Could not read Watt usage accumulative tariff 1"),
    }
}

// Parse current accumulative Watt usage
async fn parse_w_production_accumulative(telegram: &str) -> Result<f32, &'static str> {
    // Get tariff 1 usage
    let values_tariff_1 = get_values_by_id("1-0:2.8.1", &telegram)?;
    let value_tariff_1 = values_tariff_1.get(0);

    // Get tariff 2 usage
    let values_tariff_2 = get_values_by_id("1-0:2.8.2", &telegram)?;
    let value_tariff_2 = values_tariff_2.get(0);

    // If both are found, parse, add and return them
    match value_tariff_1 {
        Some(_value_tariff_1) => {
            let _value_tariff_1_parsed: f32 = _value_tariff_1
                .replace("*kWh", "")
                .parse()
                .expect("Parse Watt production accumulative tariff 1 string to f32");

            match value_tariff_2 {
                Some(_value_tariff_2) => {
                    let _value_tariff_2_parsed: f32 = _value_tariff_2
                        .replace("*kWh", "")
                        .parse()
                        .expect("Parse Watt production accumulative tariff 2 string to f32");
                    Ok(_value_tariff_1_parsed + _value_tariff_2_parsed)
                }
                None => Err("Could not read Watt production accumulative tariff 2"),
            }
        }
        None => Err("Could not read Watt production accumulative tariff 1"),
    }
}

// Parse current Watt production
async fn parse_w_production(telegram: &str) -> Result<f32, &'static str> {
    let values = get_values_by_id("1-0:2.7.0", &telegram)?;
    let value = values.get(0);
    match value {
        Some(_value) => {
            let mut _value_parsed = _value
                .replace("*kW", "")
                .parse()
                .expect("Parse Watt production string to f32");
            _value_parsed = _value_parsed * 1000.0; // kW -> W
            Ok(_value_parsed)
        }
        None => Err("Could not read Watt production"),
    }
}

// Parse current tariff (1 or 2)
async fn parse_current_tariff(telegram: &str) -> Result<f32, &'static str> {
    let values = get_values_by_id("0-0:96.14.0", &telegram)?;
    let value = values.get(0);
    match value {
        Some(_value) => {
            let _value_parsed: f32 = _value.parse().expect("Parse current tariff string to i8");
            Ok(_value_parsed)
        }
        None => Err("Could not read current tariff"),
    }
}

// Parse current gas accumulative usage
async fn parse_gas_usage_accumulative(telegram: &str) -> Result<f32, &'static str> {
    let values = get_values_by_id("0-1:24.2.1", &telegram)?;

    let _timestamp = values.get(0);
    let value = values.get(1);

    match value {
        Some(_value) => {
            if !_value.contains("*m3") {
                return Err("Invalid gas usage detected, not parsing");
            }
            let _value_parsed = _value.replace("*m3", "").parse::<f32>();
            if _value_parsed.is_err() {
                return Err("Could not parse gas usage accumulative");
            }
            Ok(_value_parsed.unwrap())
        }
        None => Err("Could not read gas usage accumulative"),
    }
}

// TODO: use the timestamps from the telegram instead of the InfluxDB fallback
async fn parse_telegram(client: &reqwest::Client, telegram: &str) {
    // let timestamp = parse_timestamp(&telegram).await;
    // match timestamp {
    //     Ok(_timestamp) => println!("Timestamp: {:?}", _timestamp),
    //     Err(_err) => println!("Error: could not find timestamp {}", _err),
    // }

    let current_tariff = parse_current_tariff(&telegram).await;
    match current_tariff {
        Ok(_current_tariff) => {
            println!("Current tariff: {:?}", _current_tariff);
            post_influx_db(client, "currentTariff", _current_tariff).await;
        }
        Err(_err) => println!("Error: could not find current tariff {}", _err),
    }

    let w_usage = parse_w_usage(&telegram).await;
    match w_usage {
        Ok(_w_usage) => {
            println!("Watt usage: {:?}", _w_usage);
            post_influx_db(client, "wattUsage", _w_usage).await;
        }
        Err(_err) => println!("Error: could not find Watt usage {}", _err),
    }

    let w_usage_accumulative = parse_w_usage_accumulative(&telegram).await;
    match w_usage_accumulative {
        Ok(_w_usage_accumulative) => {
            println!("Watt usage accumulative: {:?}", _w_usage_accumulative);
            post_influx_db(client, "wattUsageAccumulative", _w_usage_accumulative).await;
        }
        Err(_err) => println!("Error: could not find Watt usage accumulative {}", _err),
    }

    let w_production = parse_w_production(&telegram).await;
    match w_production {
        Ok(_w_production) => {
            println!("Watt production: {:?}", _w_production);
            post_influx_db(client, "wattProduction", _w_production).await;

            // Calculate nett usage
            match w_usage {
                Ok(_w_usage) => {
                    println!("Watt production - usage: {:?}", _w_production - _w_usage);
                    post_influx_db(client, "wattNett", _w_production - _w_usage).await;
                }
                Err(_err) => println!("Error: could not find Watt production - usage {}", _err),
            }
        }
        Err(_err) => println!("Error: could not find Watt production {}", _err),
    }

    let w_production_accumulative = parse_w_production_accumulative(&telegram).await;
    match w_production_accumulative {
        Ok(_w_production_accumulative) => {
            println!(
                "Watt production accumulative: {:?}",
                _w_production_accumulative
            );
            post_influx_db(
                client,
                "wattProductionAccumulative",
                _w_production_accumulative,
            )
            .await;

            // Calculate nett accumulative usage
            match w_usage_accumulative {
                Ok(_w_usage_accumulative) => {
                    println!(
                        "Watt accumulative production - usage: {:?}",
                        _w_production_accumulative - _w_usage_accumulative
                    );
                    post_influx_db(
                        client,
                        "wattAccumulativeNett",
                        _w_production_accumulative - _w_usage_accumulative,
                    )
                    .await;
                }
                Err(_err) => println!("Error: could not find Watt production - usage {}", _err),
            }
        }
        Err(_err) => println!(
            "Error: could not find Watt production accumulative {}",
            _err
        ),
    }

    let gas_usage = parse_gas_usage_accumulative(&telegram).await;
    match gas_usage {
        Ok(_gas_usage) => {
            println!("Gas usage accumulative: {:?}", _gas_usage);
            post_influx_db(client, "gasUsageAccumulative", _gas_usage).await;
        }
        Err(_err) => println!("Error: could not find gas usage accumulative {}", _err),
    }
}

fn ready_of_interest() -> Ready {
    Ready::readable() | UnixReady::hup() | UnixReady::error()
}

fn is_closed(state: Ready) -> bool {
    state.contains(UnixReady::hup() | UnixReady::error())
}

#[tokio::main]
async fn main() -> Result<(), reqwest::Error> {
    // Create reqwest HTTP client
    let client = reqwest::Client::new();

    // let example_telegram = "\u{0}\n/KFM5KAIFA-METER\r\n\r\n1-3:0.2.8(42)\r\n0-0:1.0.0(210212094443W)\r\n0-0:96.1.1(4530303235303030303634383435373136)\r\n1-0:1.8.1(007392.132*kWh)\r\n1-0:1.8.2(007139.800*kWh)\r\n1-0:2.8.1(001795.226*kWh)\r\n1-0:2.8.2(004446.275*kWh)\r\n0-0:96.14.0(0002)\r\n1-0:1.7.0(00.131*kW)\r\n1-0:2.7.0(00.000*kW)\r\n0-0:96.7.21(00001)\r\n0-0:96.7.9(00001)\r\n1-0:99.97.0(2)(0-0:96.7.19)(181206112732W)(0000007692*s)(000101000001W)(2147483647*s)\r\n1-0:32.32.0(00000)\r\n1-0:32.36.0(00000)\r\n0-0:96.13.1()\r\n0-0:96.13.0()\r\n1-0:31.7.0(002*A)\r\n1-0:21.7.0(00.123*kW)\r\n1-0:22.7.0(00.000*kW)\r\n0-1:24.1.0(003)\r\n0-1:96.1.0(4730303331303033333930303231353136)\r\n0-1:24.2.1(210205130000W)(07025.512*m3)\r\n!8234\r\n";

    let poll = Poll::new().unwrap();
    let mut events = Events::with_capacity(1024);

    // These settings are specific to your Smart Meter
    let serial_settings = mio_serial::SerialPortSettings {
        baud_rate: 115200,
        data_bits: mio_serial::DataBits::Eight,
        flow_control: mio_serial::FlowControl::None,
        parity: mio_serial::Parity::None,
        stop_bits: mio_serial::StopBits::One,
        timeout: Duration::from_millis(1),
    };

    println!(
        "Opening {}, serial settings: {:?}",
        DEFAULT_TTY, serial_settings
    );

    // Open serial port
    let mut rx = mio_serial::Serial::from_path(&DEFAULT_TTY, &serial_settings)
        .expect("Could not open serial port");

    poll.register(&rx, SERIAL_TOKEN, ready_of_interest(), PollOpt::edge())
        .unwrap();

    let mut rx_buf = [0u8; 1024];
    let mut telegram_buffer: String = "".to_owned();

    'outer: loop {
        if let Err(ref e) = poll.poll(&mut events, None) {
            println!("poll failed: {}", e);
            break;
        }

        if events.is_empty() {
            println!("Read timed out!");
            continue;
        }

        for event in events.iter() {
            match event.token() {
                SERIAL_TOKEN => {
                    let ready = event.readiness();
                    if is_closed(ready) {
                        println!("Quitting due to event: {:?}", ready);
                        break 'outer;
                    }
                    if ready.is_readable() {
                        // With edge triggered events, we must perform reading until we receive a WouldBlock.
                        // See https://docs.rs/mio/0.6/mio/struct.Poll.html for details.
                        loop {
                            match rx.read(&mut rx_buf) {
                                Ok(count) => {
                                    // Read a chunk of the telegram
                                    let telegram_chunk = String::from_utf8_lossy(&rx_buf[..count]);

                                    // Check if this line includes the telegram start of frame char "/"
                                    // and clear the accumulation string if it does.
                                    let includes_sof = telegram_chunk.find("/");
                                    if includes_sof.is_some() {
                                        telegram_buffer = "".to_string()
                                    }

                                    // Check if this line includes the telegram end of frame char "!"
                                    let includes_eof = telegram_chunk.find("!");

                                    // If it includes the terminator char complete the telegram, if it
                                    // doesn't append the string to the accumulation string.
                                    if includes_eof.is_some() {
                                        // Push final line and complete telegram
                                        telegram_buffer.push_str(&telegram_chunk);
                                        println!("Complete Telegram:");
                                        println!("{}", telegram_buffer);
                                        println!("\n");
                                        parse_telegram(&client, &telegram_buffer).await;
                                        telegram_buffer = "".to_string();
                                    } else {
                                        telegram_buffer.push_str(&telegram_chunk)
                                    }
                                }
                                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                                    break;
                                }
                                Err(ref e) => {
                                    println!("Quitting due to read error: {}", e);
                                    break 'outer;
                                }
                            }
                        }
                    }
                }
                t => unreachable!("Unexpected token: {:?}", t),
            }
        }
    }
    Ok(())
}
