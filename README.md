<table border="0">
  <tr>
    <td>
      <a href="https://github.com/daywalker90/summars/actions/workflows/latest_v24.11.yml">
        <img src="https://github.com/daywalker90/summars/actions/workflows/latest_v24.11.yml/badge.svg?branch=main">
      </a>
    </td>
    <td>
      <a href="https://github.com/daywalker90/summars/actions/workflows/main_v24.11.yml">
        <img src="https://github.com/daywalker90/summars/actions/workflows/main_v24.11.yml/badge.svg?branch=main">
      </a>
    </td>
  </tr>
  <tr>
    <td>
      <a href="https://github.com/daywalker90/summars/actions/workflows/latest_v25.02.yml">
        <img src="https://github.com/daywalker90/summars/actions/workflows/latest_v25.02.yml/badge.svg?branch=main">
      </a>
    </td>
    <td>
      <a href="https://github.com/daywalker90/summars/actions/workflows/main_v25.02.yml">
        <img src="https://github.com/daywalker90/summars/actions/workflows/main_v25.02.yml/badge.svg?branch=main">
      </a>
    </td>
  </tr>
  <tr>
    <td>
      <a href="https://github.com/daywalker90/summars/actions/workflows/latest_v25.05.yml">
        <img src="https://github.com/daywalker90/summars/actions/workflows/latest_v25.05.yml/badge.svg?branch=main">
      </a>
    </td>
    <td>
      <a href="https://github.com/daywalker90/summars/actions/workflows/main_v25.05.yml">
        <img src="https://github.com/daywalker90/summars/actions/workflows/main_v25.05.yml/badge.svg?branch=main">
      </a>
    </td>
  </tr>
  <tr>
    <td>
      <a href="https://github.com/daywalker90/summars/actions/workflows/latest_v25.09.yml">
        <img src="https://github.com/daywalker90/summars/actions/workflows/latest_v25.09.yml/badge.svg?branch=main">
      </a>
    </td>
    <td>
      <a href="https://github.com/daywalker90/summars/actions/workflows/main_v25.09.yml">
        <img src="https://github.com/daywalker90/summars/actions/workflows/main_v25.09.yml/badge.svg?branch=main">
      </a>
    </td>
  </tr>
</table>

# summars
A core lightning plugin to show a summary of your channels and optionally recent forwards, payments and/or paid invoices.

* [Installation](#installation)
* [Building](#building)
* [Methods](#methods)
* [Example Usage](#example-usage)
* [How to set options](#how-to-set-options)
* [Options](#options)
* [Availability Database](#availability-database)
* [Thanks](#thanks)

# Installation
For general plugin installation instructions see the plugins repo [README.md](https://github.com/lightningd/plugins/blob/master/README.md#Installation)

Release binaries for
* x86_64-linux
* armv7-linux (Raspberry Pi 32bit)
* aarch64-linux (Raspberry Pi 64bit)

can be found on the [release](https://github.com/daywalker90/summars/releases) page. If you are unsure about your architecture you can run ``uname -m``.

They require ``glibc>=2.31``, which you can check with ``ldd --version``.

# Building
You can build the plugin yourself instead of using the release binaries.
First clone the repo:

```
git clone https://github.com/daywalker90/summars.git
```

Install a recent rust version ([rustup](https://rustup.rs/) is recommended) and in the ``summars`` folder run:

```
cargo build --release
```

After that the binary will be here: ``target/release/summars``

Note: Release binaries are built using ``cross`` and the ``optimized`` profile.

# Methods

There are currently two commands:
* ``summars`` the main command
* ``summars-refreshalias`` to manually refresh the peer alias cache

# Example Usage

```
lightning-cli summars summars-forwards=200 summars-pays=3 summars-invoices=3
address=03da2efc78ba5420048e636e541e3b484d3e314e2fca7672c0450214f7a9f2fd2e@189.78.23.211:9735
num_utxos=11
utxo_amount=0.125046909 BTC
num_channels=13
num_connected=12
num_gossipers=2
avail_out=0.05614095 BTC
avail_in=0.09208059 BTC
fees_collected=0.00001555 BTC
channels_flags=P:private O:offline
 OUT_SATS  |  IN_SATS   |     SCID      |  MAX_HTLC  | FLAG | BASE |  PPM  |        ALIAS         |                              PEER_ID                               | UPTIME | HTLCS | STATE
-----------+------------+---------------+------------+------+------+-------+----------------------+--------------------------------------------------------------------+--------+-------+-------
   103,313 |     96,686 | 2471854x37x7  |     51,000 | [__] |    0 |     1 | node204.fra.memp[..] | 039c14fdec2d958e3d14cebf657451bbd9e039196615785e82c917f274e3fb2205 |   100% |     0 |  OK
    84,313 |    115,686 | 2471854x37x9  |     51,000 | [_O] |    0 |     1 | node205.fra.memp[..] | 033589bbcb233ffc416cefd5437c7f37e9d7cb7942d405e39e72c4c846d9b37f18 |    81% |     0 |  OK
   172,977 |     27,022 | 2471854x37x10 |     51,000 | [__] |    0 |     1 | OLYMPUS by ZEUS      | 03e84a109cd70e57864274932fc87c5e6434c59ebb8e6e7d28532219ba38f7f6df |   100% |     0 |  OK
   194,978 |      5,021 | 2471854x37x12 |     51,000 | [__] |    0 |     1 | 1ML.com node ALPHA   | 02312627fdf07fbdd7e5ddb136611bdde9b00d26821d14d94891395452f67af248 |   100% |     0 |  OK
    10,147 |    189,852 | 2471854x37x13 |          1 | [P_] |    0 |     1 | cyclopes             | 028ec70462207b57e3d4d9332d9e0aee676c92d89b7c9fb0850fc2a24814d4d83c |   100% |     0 |  OK
    13,947 |    186,052 | 2471854x37x14 |          1 | [__] |    0 |     1 | 030f375d8aecdddc8523 | 030f375d8aecdddc852309c15c3b67c2934de0de4d31e1e04a03d656ca0a78d008 |   100% |     0 |  OK
   127,035 |    873,964 | 2476625x46x0  |     51,000 | [__] |    0 | 1,849 | lndus1.next.zaphq.io | 022251a9fa007cd60acee9cbc6ab4b15d2ad52cad5f271b0276d3b2d97e3d87b43 |    87% |     0 |  OK
    48,836 |    952,163 | 2476625x46x1  |          1 | [__] |    0 | 1,849 | lndus0.next.zaphq.io | 028c3640c57ffe47eb41db8225968833c5032f297aeba98672d6f7037090d59e3f |    86% |     0 |  OK
   380,431 |    620,568 | 2476625x46x2  |     51,000 | [__] |    0 |     1 | Bitnob(Jos, 2001)    | 035c32eded21dd4a073153c4e3c1e56618f1f77b8edb66653a0a643f7a78260117 |    99% |     0 |  OK
   375,512 |    625,487 | 2476625x46x3  |    990,990 | [__] |    0 |     1 | lndus0.dev.zaphq.io  | 03819f6e407d3890484bed25b56b2ca582a883a4aa5671965462f591732381b358 |   100% |     0 |  OK
   253,922 |    747,077 | 2476625x46x4  |    990,990 | [__] |    0 |     1 | lndus1.dev.zaphq.io  | 02be8f360e57600486b93dd33ea0872a4e14a259924ba4084f27d693a77d151158 |   100% |     0 |  OK
    26,738 |  1,819,119 | 2476654x19x0  |          1 | [__] |    0 |     1 | Boltz                | 03f060953bef5b777dc77e44afa3859d022fc1a77c55138deb232ad7255e869c00 |   100% |     0 |  OK
    49,714 |    951,285 | 2501790x6x4   |          1 | [__] |    0 |     1 | endurance            | 03933884aaf1d6b108397e5efe5c86bcf2d8ca8d2f700eda99db9214fc2712b134 |   100% |     0 |  OK

                                  forwards (last 200h, limit: off)
 resolved_time         in_alias               out_alias             in_sats   out_sats   fee_msats
 5/16/24, 3:48:58 PM   lndus1.next.zaphq.io   02758997f184be06f435    11,503     11,503          11

                                                 pays (last 3h, limit: off)
 completed_at          payment_hash                                                       sats_sent  fee_msats   destination
 5/19/24, 5:34:58 PM   19483c34259aa832a6bb805b72bc02db89d639975608adf4371d8ff4eda8be99      50,000           0   Boltz
 5/19/24, 5:34:59 PM   175168a906829f668d712786604bc1db04358f02fcf37563afb58402a7cb9282      50,000           0   Boltz
 5/19/24, 5:35:01 PM   ad6491728f6a120980f14a5eb5688b7406ec2563525af812b911a39649da3f09      50,000           0   Boltz
 5/19/24, 5:35:02 PM   d2ca9282a0a546b01b6ff5ecc71a313cbe0c2f0ac084b9075c203078f8dbaa71      50,000           0   Boltz
 5/19/24, 5:35:03 PM   71ce7cabc80f55b51dd9373f2ea57acaa21b9087e993ebfad3b4302dc46a943b      50,000           0   Boltz
 5/19/24, 5:35:04 PM   cbd929f8a0bc4bc46dba51b0dc3188662708d1bd8e6ae5ee71ccd894194d40db      50,000           0   Boltz

                                                 invoices (last 3h, limit: off)
 paid_at               label                            sats_received   payment_hash
 5/19/24, 5:35:21 PM   WpcvbXKW                                50,000   39eb0fa161fb85ab7c653bdfb49da8ca4914287b2ecd567968a3c4156c285768
 5/19/24, 5:35:22 PM   TQCankEM                                50,000   432c37e7bfae40df087e2e8a3c4590d3cd7011ebf56760dee8edfea3b8281e0e
 5/19/24, 5:35:23 PM   HgGRjkrs                                50,000   3568ebd9431ec2cf17528ba4e7a144aca62f9606f7766a33afd41ef30c50700c
 5/19/24, 5:35:25 PM   82ac9e392f2dd41f4d53ff9ebe[..]          50,000   623743a48f5d0a8f20b9605228b4da0be7bd2cd72f7da39ec0028bf953317785
 5/19/24, 5:35:26 PM   82ac9e392f2dd41f4d53ff9ebe[..]          50,000   be5fe3a6fff1efe77362186f357414f69d6d43e26f35af6233135fb2fcac06e3
 5/19/24, 5:35:27 PM   82ac9e392f2dd41f4d53ff9ebe[..]          50,000   7ad2e0bb16ecc6f3f893c9f626cdb85802aa27c0ff00d24bce32e28dbebb32ab
```


```
lightning-cli summars summars-columns=GRAPH_SATS,SCID summars-style=empty summars-sort-by=IN_SATS
address=03da2efc781a5420088e636e342e3b484d3e514e1fca7672c0450515f7a9f2fd6e@129.38.53.21:9735
num_utxos=5
utxo_amount=0.25046909 BTC
num_channels=5
num_connected=5
num_gossipers=1
avail_out=0.15614095 BTC
avail_in=0.49208059 BTC
fees_collected=0.00103555 BTC
channels_flags=P:private O:offline
 ├0.16777060   OUT GRAPH_SATS IN     0.16777060┤      SCID
                        ╟─┤                       2472654x19x0
            ├───────────┼──────────┤              2534335x3x0
              ├─────────┼────────────┤            2536331x16x0
                   ├────┼─────────────────┤       2531339x15x0
                        ╟──────────────────────┤  2576452x228x0
```

# How to set options
``summars`` is a dynamic plugin with dynamic options, so you can start it after CLN is already running and modify it's options after the plugin is started. There are three different methods of setting the options:

1. Running the ``summars`` command. This only sets the options temporarily for this one execution of ``summars`` and there are some exceptions for options that don't make sense here.

* Example: ``lightning-cli summars summars-forwards=6``

2. When starting the plugin dynamically.

* Example: ``lightning-cli -k plugin subcommand=start plugin=/path/to/summars summars-forwards=6``

3. Permanently saving them in the CLN config file. :warning:If you want to do this while CLN is running you must use [setconfig](https://docs.corelightning.org/reference/lightning-setconfig) instead of manually editing your config file! :warning:If you have options in the config file (either by manually editing it or by using the ``setconfig`` command) make sure the plugin will start automatically with CLN (include ``plugin=/path/to/summars`` or have a symlink to ``summars`` in your ``plugins`` folder). This is because CLN will refuse to start with config options that don't have a corresponding plugin loaded. :warning:If you edit your config file manually while CLN is running and a line changes their line number CLN will crash when you use the [setconfig](https://docs.corelightning.org/reference/lightning-setconfig) command, so better stick to ``setconfig`` only during CLN's uptime!

* Example: ``lightning-cli setconfig summars-forwards 6``

You can mix these methods and if you set the same option with different methods, it will pick the value from your most recently used method.

# Options
### Channels table
* ``summars-columns`` Comma-separated list of enabled columns in the channel table. Also dictates order of columns. Valid columns: ``GRAPH_SATS``, ``PERC_US``, ``OUT_SATS``, ``IN_SATS``, ``TOTAL_SATS``, ``SCID``, ``MIN_HTLC``, ``MAX_HTLC``, ``FLAG``, ``BASE``, ``IN_BASE``, ``PPM``, ``IN_PPM``, ``ALIAS``, ``PEER_ID``, ``UPTIME``, ``HTLCS``, ``STATE``, ``PING``. Default columns: ``OUT_SATS``, ``IN_SATS``, ``SCID``, ``MAX_HTLC``, ``FLAG``, ``BASE``, ``PPM``, ``ALIAS``, ``PEER_ID``, ``UPTIME``, ``HTLCS``, ``STATE``
* ``summars-sort-by`` Sort by column name. Use ``-`` before column name to reverse sort. Valid columns are all ``summars-columns`` except for ``GRAPH_SATS`` (use ``IN_SATS``, ``OUT_SATS`` or ``TOTAL_SATS`` instead). Default is ``SCID``
* ``summars-exclude-states`` List if excluded channel states. Comma-separated. Valid states are: ``OPENING``, ``AWAIT_LOCK``, ``OK``, ``SHUTTING_DOWN``, ``CLOSINGD_SIGEX``, ``CLOSINGD_DONE``, ``AWAIT_UNILATERAL``, ``FUNDING_SPEND``, ``ONCHAIN``, ``DUAL_OPEN``, ``DUAL_COMITTED``, ``DUAL_COMMIT_RDY``, ``DUAL_AWAIT``, ``AWAIT_SPLICE`` and ``PUBLIC``, ``PRIVATE`` to filter channels by their network visibility, aswell as  or `ONLINE,OFFLINE` to filter by connection status.
### Forwards table
* ``summars-forwards`` List successfull forwards of the last x hours. Default is ``0`` hours (disabled)
* ``summars-forwards-limit`` Additionally limit the amount of entries shown by ``summars-forwards``. Default is ``0`` (off)
* ``summars-forwards-columns`` Comma-separated list of enabled columns in the forwards table. Also dictates order of columns. Valid columns: ``received_time``, ``resolved_time``, ``in_channel``, ``out_channel``, ``in_alias``, ``out_alias``, ``in_sats``, ``in_msats``, ``out_sats``, ``out_msats``, ``fee_sats``, ``fee_msats``, ``eff_fee_ppm``. Default columns: ``resolved_time``, ``in_alias``, ``out_alias``, ``in_sats``, ``out_sats``, ``fee_msats``
* ``summars-forwards-filter-amount-msat`` Filter forwards where **in** amount is smaller than or equal to x msat and show a summary of those forwards instead. Default is ``-1`` (disabled)
* ``summars-forwards-filter-fee-msat`` Filter forwards where **fee** amount is smaller than or equal to x msat and show a summary of those forwards instead. Default is ``-1`` (disabled)
### Pays table
* ``summars-pays`` List successfull payments of the last x hours. Default is ``0`` hours (disabled)
* ``summars-pays-limit`` Additionally limit the amount of entries shown by ``summars-pays``. Default is ``0`` (off)
* ``summars-pays-columns`` Comma-separated list of enabled columns in the pays table. Also dictates order of columns. Valid columns: ``completed_at``, ``payment_hash``, ``sats_requested``, ``msats_requested``, ``sats_sent``, ``msats_sent``, ``fee_sats``, ``fee_msats``, ``destination``, ``description``, ``preimage``. Default columns are: ``completed_at``, ``payment_hash``, ``sats_sent``, ``fee_sats``, ``destination``:warning: If you enable the ``description`` field in most cases an extra RPC call is necessary for each displayed payment. This could slow down the response time of ``summars`` if you have alot of payments in your configured time window.
### Invoices table
* ``summars-invoices`` List successfully paid invoices of the last x hours. Default is ``0`` hours (disabled)
* ``summars-invoices-limit`` Additionally limit the amount of entries shown by ``summars-invoices``. Default is ``0`` (off)
* ``summars-invoices-columns`` Comma-separated list of enabled columns in the invoices table. Also dictates order of columns. Valid columns: ``paid_at``, ``label``, ``description``, ``sats_received``, ``msats_received``, ``payment_hash``, ``preimage``. Default columns are: ``paid_at``, ``label``, ``sats_received``, ``payment_hash``
* ``summars-invoices-filter-amount-msat`` Filter invoices where **received** amount is smaller than or equal to x msat and show a summary of those invoices instead. Default is ``-1`` (disabled)
### Background tasks
* ``summars-refresh-alias`` How many hours between refreshing the node aliases in memory. Default is ``24`` hours
* ``summars-availability-interval`` How often the availability should be calculated. Default is ``300`` seconds
* ``summars-availability-window`` How many hours the availability should be averaged over. Default is ``72`` hours
### Formatting
* ``summars-locale`` Set locale for number and date formatting. Default is the systems locale with ``en-US`` as fallback, if it could not be detected.
* ``summars-utf8`` Switch on/off special characters in node aliases. Off replaces special characters with a ``?``. Default is ``true`` (on)
* ``summars-style`` Set the table style for the summary table. Valid values are: ``ascii``, ``modern``, ``sharp``, ``rounded``, ``extended``, ``psql``, ``markdown``, ``re_structured_text``, ``dots``, ``ascii_rounded``, ``blank``, ``empty``. You can see previews here: [tabled-styles](https://github.com/zhiburt/tabled/?tab=readme-ov-file#styles). Default is ``modern``
* ``summars-flow-style`` Same as ``summars-style`` but for the "flow" tables (forwards/pays/invoices). Default is ``blank``
* ``summars-max-alias-length`` How long aliases are allowed to be before they get cut off. If you use a negative value (e.g. ``-20``) it will use wrapping at that length instead. Default is ``20``
* ``summars-max-description-length`` How long descriptions are allowed to be before they get cut off. If you use a negative value (e.g. ``-30``) it will use wrapping at that length instead. Default is ``30``
* ``summars-max-label-length`` How long invoice labels are allowed to be before they get cut off. If you use a negative value (e.g. ``-30``) it will use wrapping at that length instead. Default is ``30``
### Misc
* ``summars-json`` Set output to json format. Default is ``false``

# Availability Database
The availability is persistent through plugin restarts.
The db is located in your lightning folder in the ``summars`` folder (e.g. ``.lightning/bitcoin/summars/availdb.json``).
If you want to reset these stats stop the plugin and then remove the file.

# Thanks
Thank you to [cdecker](https://github.com/cdecker) for helping me get into writing a plugin with ``cln-plugin``, the people in https://t.me/lightningd and the authors of the original [summary](https://github.com/lightningd/plugins/tree/master/summary) plugin.
