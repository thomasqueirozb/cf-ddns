# cf-ddns: Cloudflare Dynamic DNS

## Usage
1. Copy and edit accordingly config.toml.example to ~/.config/cf-ddns/config.toml (`XDG_CONFIG_HOME` is used if set)
2. `cargo run`

Command line values and environment variables can be used to override the values in the config. Run with `--help` to see the values and how to use them.

The config file location can be overriden with the `-c` (or `--config-path`) flag

It is possible to run without a config file and only use command line flags/environment variables. The `--subdomain` flag is needed to specify the subdomain to be used.
