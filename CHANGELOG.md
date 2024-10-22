# Changelog

## [3.5.0] 2024-10-22

### Added

- Total values summary for forwards, pays and invoices table at the bottom in normal view and as ``totals`` object in json mode
- Show timeframe and limits you are currently using for each table of forwards, pays and invoices
- New options to limit output of forwards, pays and invoices tables: ``summars-forwards-limit``, ``summars-pays-limit`` and ``summars-invoices-limit``. Defaults to 0 (off) and limits the outputs to the last x entries, useful if you are setting high time values for ``summars-forwards``, ``summars-pays`` or ``summars-invoices``

## [3.4.0] 2024-09-22

### Added

- nix flake (thanks to @RCasatta)
- New column for the main channels table: ``PERC_US``: the percentage of funds in the channel that belong to us.
- New column ``TOTAL_SATS``: the total size of the channel in sats
- New column ``MIN_HTLC``: the minimum size of an outgoing htlc for that channel in sats
- New column ``IN_BASE``: the base fee in msats set by the channel's peer
- New column ``IN_PPM``: the ppm fee set by the channel's peer

### Changed

- Can only sort by enabled columns, since we don't collect data on some disabled columns for performance reasons
- updated dependencies

### Fixed

- Improved performance if invoices or forwards tables don't have anything to show in the requested time window

## [3.3.1] 2024-06-29

### Added

- ``summars-exclude-states``: added `ONLINE,OFFLINE` states to filter by connection status
- ``summars-forwards-columns``: added ``fee_sats``, ``in_msats``, ``out_msats`` as non-default columns
- ``summars-pays-columns``: added ``fee_sats`` as a new default column and ``msats_requested``, ``msats_sent`` as non-default columns
- ``summars-invoices-columns``: added ``msats_received`` as non-default column

### Changed

- sats values are rounded to the closest integer instead of rounded down

### Fixed

- column names and states are now case insensitive

## [3.3.0] 2024-06-05

### Added

- ``summars-pays-columns`` Comma-separated list of enabled columns in the pays table. Also dictates order of columns. Valid columns: ``completed_at``, ``payment_hash``, ``sats_requested``, ``sats_sent``, ``fee_msats``, ``destination``, ``description``, ``preimage``. Default columns are: ``completed_at``, ``payment_hash``, ``sats_sent``, ``destination`` :warning: If you enable the ``description`` field in most cases an extra RPC call is necessary for each displayed payment. This could slow down the response time of ``summars`` if you have alot of payments in your configured time window.
- ``summars-max-description-length`` How long descriptions are allowed to be before they get cut off. If you use a negative value (e.g. ``-30``) it will use wrapping at that length instead. Default is ``30``
- ``summars-invoices-columns`` Comma-separated list of enabled columns in the invoices table. Also dictates order of columns. Valid columns: ``paid_at``, ``label``, ``description``, ``sats_received``, ``payment_hash``, ``preimage``. Default columns are: ``paid_at``, ``label``, ``sats_received``, ``payment_hash``
- ``summars-max-label-length`` How long invoice labels are allowed to be before they get cut off. If you use a negative value (e.g. ``-30``) it will use wrapping at that length instead. Default is ``30``
- ``summars-forwards-columns`` Comma-separated list of enabled columns in the forwards table. Also dictates order of columns. Valid columns: ``received_time``, ``resolved_time``, ``in_channel``, ``out_channel``, ``in_sats``, ``out_sats``, ``fee_msats``. Default columns: ``resolved_time``, ``in_channel``, ``out_channel``, ``in_sats``, ``out_sats``, ``fee_msats``

### Changed

- Options code refactored. All options are now natively dynamic and there is no longer any manual reading of config files. Read the updated README section on how to set options for more information
- ``summars-max-alias-length`` supports wrapping by using negative values (just like the new ``summars-max-description-length`` and ``summars-max-label-length``), see README
- ``summars-columns`` and the new ``-columns`` options now also dictate the order of the columns
- ``summars-forwards`` now sorted by `resolved_time` instead of `received_time`
- ``summars-json``: in forwards objects ``received`` is now called ``received_time``

### Fixed

- Documentation error that stated you can sort by ``GRAPH_SATS`` (you never could and it will error now if you try)
- Filter message spelling and formatting

## [3.2.0] - 2024-05-03

### Added

- `summars-sort-by`: now supports reverse sorting by using a `-` prefix, e.g. `-ALIAS`
- `summars-json`: new boolean option to output data in json format

### Changed

- if you had the plugin with config file options start with CLN and then changed an option and only reloaded the plugin, CLN would pass stale option values to the plugin so the load priority changed to:
    1. config file options
    2. ``plugin start`` options

### Fixed

- no longer ignore general config file if a network config file exists