# summars
A core lightning plugin to show a summary of your channels and optionally recent forwards.

* [Installation](#installation)
* [Building](#building)
* [Example Usage](#example-usage)
* [How to set options](#how-to-set-options)
* [Options](#options)
* [Availability Database](#availability-database)
* [Thanks](#thanks)

### Installation
For general plugin installation instructions see the plugins repo [README.md](https://github.com/lightningd/plugins/blob/master/README.md#Installation)

### Building
You can build the plugin yourself instead of using the release binaries.
First clone the repo:

``git clone https://github.com/daywalker90/summars.git``

Install a recent rust version ([rustup](https://rustup.rs/) is recommended) and in the ``summars`` folder run:

``cargo build --release``

After that the binary will be here: ``target/release/summars``

### Example Usage

There are currently two commands:
* ``summars`` the main command
* ``summars-refreshalias`` to manually refresh the alias cache

```
lightning-cli summars
address=03b2687cb99a272ab73796071ef5c545c33087f0ae39ec3bf4fb91551ac959c453@127.0.0.1:7272
num_utxos=4
utxo_amount=1.95473201 BTC
num_channels=5
num_connected=3
num_gossipers=0
avail_out=0.04454602 BTC
avail_in=0.02405398 BTC
fees_collected=0.00000005 BTC
channels_flags=P:private O:offline
┌───────────┬─────────┬─────────┬───────────┬──────┬──────┬─────┬─────────────┬────────────────────────────────────────────────────────────────────┬────────┬───────┬────────────┐
│ OUT_SATS  │ IN_SATS │  SCID   │ MAX_HTLC  │ FLAG │ BASE │ PPM │    ALIAS    │                              PEER_ID                               │ UPTIME │ HTLCS │   STATE    │
├───────────┼─────────┼─────────┼───────────┼──────┼──────┼─────┼─────────────┼────────────────────────────────────────────────────────────────────┼────────┼───────┼────────────┤
│   494,956 │ 505,043 │ 103x1x1 │   990,000 │ [__] │    1 │  10 │ SLIMYGLEE   │ 0247a9c9098827b15b76bf7e6b867595e1adef69817caf0f9850c9c13d883e7345 │   100% │     0 │     OK     │
├───────────┼─────────┼─────────┼───────────┼──────┼──────┼─────┼─────────────┼────────────────────────────────────────────────────────────────────┼────────┼───────┼────────────┤
│   479,848 │ 520,151 │ 312x4x0 │   990,000 │ [_O] │    0 │  40 │ ODDFEED     │ 0358695158e877b3ddd10012de0695025a7a41a9c02df23478996a113b30cb8b2d │    10% │     0 │     OK     │
├───────────┼─────────┼─────────┼───────────┼──────┼──────┼─────┼─────────────┼────────────────────────────────────────────────────────────────────┼────────┼───────┼────────────┤
│   479,950 │ 520,049 │ 312x7x1 │   990,000 │ [__] │    1 │  10 │ REDWALK     │ 026935bc8ee97458163a09d8fc0b9860a8d7464a24593858cdf22fa3b170230099 │   100% │     0 │  ONCHAIN   │
├───────────┼─────────┼─────────┼───────────┼──────┼──────┼─────┼─────────────┼────────────────────────────────────────────────────────────────────┼────────┼───────┼────────────┤
│ 1,469,962 │ 530,037 │ 573x1x0 │ 1,980,000 │ [__] │    0 │  30 │ WEIRDTROUGH │ 0252e6ee10696089721f4fec5d14c041fff92a12b654a5b221df8895bff64c5e5b │   100% │     0 │     OK     │
├───────────┼─────────┼─────────┼───────────┼──────┼──────┼─────┼─────────────┼────────────────────────────────────────────────────────────────────┼────────┼───────┼────────────┤
│ 2,079,833 │ 920,166 │ 585x1x1 │ 2,970,000 │ [_O] │    0 │  20 │ ODDFEED     │ 0358695158e877b3ddd10012de0695025a7a41a9c02df23478996a113b30cb8b2d │    10% │     0 │     OK     │
├───────────┼─────────┼─────────┼───────────┼──────┼──────┼─────┼─────────────┼────────────────────────────────────────────────────────────────────┼────────┼───────┼────────────┤
│ 1,000,000 │       0 │ PENDING │   990,000 │ [__] │    1 │  10 │ REDWALK     │ 026935bc8ee97458163a09d8fc0b9860a8d7464a24593858cdf22fa3b170230099 │   100% │     0 │ AWAIT_LOCK │
└───────────┴─────────┴─────────┴───────────┴──────┴──────┴─────┴─────────────┴────────────────────────────────────────────────────────────────────┴────────┴───────┴────────────┘
 
 forwards              in_channel   out_channel   in_sats   out_sats   fee_msats
 2022-12-28 14:47:28   SLIMYGLEE    REDWALK        10,000     10,000         101
 2022-12-28 14:49:12   SLIMYGLEE    REDWALK        10,000     10,000         101
 2022-12-28 14:50:13   SLIMYGLEE    REDWALK        10,000     10,000         101
 2022-12-29 17:32:15   SLIMYGLEE    REDWALK        10,000     10,000         101
 2022-12-30 12:20:49   SLIMYGLEE    REDWALK        10,000     10,000         101
 2022-12-30 15:44:24   SLIMYGLEE    REDWALK        10,000     10,000         101
 2022-12-30 15:45:17   SLIMYGLEE    REDWALK        10,000     10,000         101
 2022-12-30 15:48:28   SLIMYGLEE    REDWALK        10,000     10,000         101
 2022-12-30 15:50:17   SLIMYGLEE    REDWALK        10,000     10,000         101
 2023-01-04 14:00:54   SLIMYGLEE    REDWALK        10,000     10,000         101
 2023-01-04 14:10:35   SLIMYGLEE    REDWALK        10,000     10,000         101

 pays                  payment_hash                                                       destination
 2023-01-17 21:03:10   b4222a957dc058ec5a4613e4a34f5bea26f9b2e36561497894d838774bd42dff   RoboSats
 2023-01-17 21:18:48   4a48c7690f4ac0a16c39ed7a71f232380fc6d3c9927a87d7c531da49204f86c7   STACKER.NEWS
 2023-01-17 23:20:41   12b2c23562eb77b468f679d0aec0067f60d3e39edf3b33fb0e4d9ad51ef4e9c2   WalletOfSatoshi.com
 2023-01-19 22:32:31   b924776ae274ddc39f41b9f170e18b67d648aa68d04dd682eef60fe8addc965d   Kraken
 2023-01-19 22:49:30   812e80e8e82923b8ab937d72461784542da6b605c11fb2a5be2cf16c83761c97   Kraken

 invoices              label     sats_received
 2023-01-17 17:16:13   label1           15,000
 2023-01-17 23:19:11   label2               10
 2023-01-18 00:11:54   label3                1
 2023-01-18 18:16:06   label4          200,000
 2023-01-18 19:25:31   label5           13,000
 2023-01-18 23:05:04   label6          200,000
 2023-01-20 13:24:18   label7               10
```

### How to set options
``summars`` is a dynamic plugin, so you can start it after cln is already running. You have three different methods of setting the options:

1. running the summars command
2. when starting the plugin via ``lightning-cli plugin -k subcommand=start plugin=/path/to/summars``
3. the cln config file

:warning:Warning: If you use the cln config file to set summars options make sure you include plugin=/path/to/summars or cln will not start next time!

You can mix theses methods but if you set the same option with multiple of these three methods the priority is 1. -> 2. -> 3.

Examples:
1. ``lightning-cli summars summars-forwards=6``
2. ``lightning-cli -k plugin subcommand=start plugin=/path/to/summars summars-forwards=6``
3. just like other cln options in the config file: ``summars-forwards=6``

### Options
* ``summars-show-pubkey`` Include pubkey in summary table. Default is ``true``
* ``summars-show-maxhtlc`` Include max_htlc in summary table. Default is ``true``
* ``summars-sort-by`` Sort by column name. Default is ``SCID``
* ``summars-forwards`` List successfull forwards of the last x hours. Default is ``0`` hours (disabled)
* ``summars-forward-alias`` In the forwards list show aliases insted of scid's. Default is ``true``
* ``summars-pays`` List successfull payments of the last x hours. Default is ``0`` hours (disabled)
* ``summars-invoices`` List successfully paid invoices of the last x hours. Default is ``0`` hours (disabled)
* ``summars-locale`` Set locale to change the thousand delimiter. Default is ``en``
* ``summars-refresh-alias`` How many hours between refreshing the node aliases in memory. Default is ``24`` hours
* ``summars-max-alias-length`` How long aliases are allowed to be before they get cut off. Default is ``20`` chars
* ``summars-availability-interval`` How often the availability should be calculated. Default is ``300`` seconds
* ``summars-availability-window`` How many hours the availability should be averaged over. Default is ``72`` hours
* ``summars-utf8`` Switch on/off special characters in node aliases. Off replaces special characters with a ``?``. Default is ``true`` (on)

### Availability Database
The availability is persistent thorugh plugin restarts.
The db is located in your lightning folder in the summars folder (e.g. ``.lightning/bitcoin/summars/availdb.json``).
If you want to reset these stats stop the plugin and then remove the file.

## Thanks
Thank you to [cdecker](https://github.com/cdecker) for helping me get into writing a plugin with cln-plugin, the people in https://t.me/lightningd and the authors of the original [summary](https://github.com/lightningd/plugins/tree/master/summary) plugin.






















