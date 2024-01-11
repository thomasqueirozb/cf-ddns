# cf-ddns: Cloudflare Dynamic DNS

## Usage
1. Copy config.toml.example to ~/.config/cf-ddns/config.toml and edit the values accordingly (`XDG_CONFIG_HOME` is used if set)
   - ⚠️**CHANGE PERMISSIONS IF STORING KEYS IN THERE**⚠️ (`chmod 600 ~/.config/cf-ddns/config.toml`).
   Keys can be set using environment variables:
       * `CF_API_KEY`, `CF_ACCOUNT_EMAIL`
       * `CF_ACCOUNT_EMAIL`
   - The config file location can be overriden with the `-c` (or `--config-path`) flag
2. `cargo run`

Command line values and environment variables can be used to override the values in the config. Run with `--help` to see the values and how to use them.

It is possible to run without a config file and only use command line flags/environment variables. The `--subdomain` flag is needed to specify the subdomain to be used.
