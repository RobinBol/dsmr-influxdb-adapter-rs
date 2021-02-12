# InfluxDB Adapter for DSMR5.0 compatible Dutch Smart Meters

This is a Rust application that reads data from a Dutch Smart Meter using a serial connection to the P1 port. Currently only the DSMRS5.0 protocol is supported.

Requirements: 
- P1 to USB-cable
- Something to plug the cable in and run this program
- InfluxDB on the same machine as this program
- Optional: Grafana to visualize the InfluxDB data

### Install

1. Install [InfluxDB](https://docs.influxdata.com/influxdb/v1.8/introduction/install/) and optionally [Grafana](https://grafana.com/docs/grafana/latest/installation/).
2. Checkout this repo.
3. Make sure `libudev-dev` and `libssl-dev` are installed:
```sh
sudo apt-get install libudev-dev && sudo apt-get install libssl-dev
```
4. Check the serial path (by default `/dev/ttyUSB0`) and change according to your setup.
5. Test if it works by running `cargo run` (if you don't have the Rust toolchain installed click [here](https://www.rust-lang.org/tools/install))
6. Finally, run `cargo build` to create the binary executable. Use this executable as you wish, for example add it as systemd service so that it automatically starts and restarts.

Create `/etc/systemd/system/smart-meter.service`:
```
[Unit]
Description=InfluxDB-DSMR Adapter Process

[Service]
ExecStart=/home/<user>/path/to/dsmr-influxdb-adapter-rs/target/debug/dsmr-influxdb-adapter-rs
Restart=always

[Install]
WantedBy=multi-user.target
```

And run `sudo systemctl enable smart-meter.service`.

### Usage

Use a data visualization tool that uses InfluxDB as data source to create some nice graphs and/or dashboards. For example:

_Disclaimer: I probably forgot to document something..._