# Changelog

## [Unreleased]

### Changed
- if node aliases are missing, `summars` will check the gossip faster and only when almost all aliases are found it will use `summars-refresh-alias` again

### Removed
- all code for CLN versions ``< v24.11``

## [5.2.0] - 2025-09-06

### Added
- *CLN v25.09+ only*: `summars-columns`: new `PING` column which pings your peers and shows the result in milliseconds, not enabled by default since it raises response time significantly
- Added a count to show how much of your `summars-*-limit` is used

## [5.1.0] - 2025-07-07

### Changed
- upgraded dependencies, locales now must not be written as `en_US` but rather `en-US` (just the language part is enough anyways)
- raise MSRV to 1.82 because of `icu_calendar`

### Fixed
- correctly apply ``summars-max-alias-length`` option to new ``in_alias`` and ``out_alias`` forwards columns instead of ``in_channel`` / ``out_channel``
- forwards that are not settled for a time longer than your `summars-forwards` now show up when they settle

## [5.0.0] 2025-04-10

### Added
- ``summars-forwards-columns``: added ``in_alias`` and ``out_alias`` to show the peer aliases of the forward if possible, falls back to ShortchannelId's if no alias was found

### Changed
- ``summars-forwards-columns``: ``in_channel`` and ``out_channel`` are now always the ShortchannelId's, since ``summars-forwads-alias`` no longer exists
- ``summars-forwards-columns``: default columns are now ``resolved_time``, ``in_alias``, ``out_alias``, ``in_sats``, ``out_sats``, ``fee_msats``
- ``description`` columns in ``summars-pays-columns`` and ``summars-invoices-columns`` have characters replaced that would be escaped by CLN and then would misalign the column e.g. replace `"` with `'`
- ``summars-sort-by`` can now sort by disabled columns aswell

### Removed
- ``summars-forwads-alias`` option was removed in favor of additional ``summars-forwards-columns``


## [4.0.2] 2025-03-11

### Changed

- upgraded dependencies

### Fixed

- fix error in edge case for pending channels in cln 23.11 and earlier
- improve version detection on alternative cln builds (e.g. btcpayserver custom cln builds)

## [4.0.1] 2024-12-20

### Fixed

- pays: CLN 24.11+: payments that were started (but not yet finished) earlier than the first payment shown by summars would not show up once paid
- invoices: invoices created before the first invoice shown by summars would not show up once paid
- forwards: offered and not yet settled forwards that were created before the first forward shown by summars would not show up once settled

## [4.0.0] 2024-12-10

### Added

- pays: CLN 24.11+ will use newly added indexing in ``listpays`` to speed up building the pays table on subsequent summars calls

### Changed

- pays: summars can't guarantee that it can find/calculate ``sats_requested``,``msats_requested`` and therefore also can't guarantee ``fee_sats`` and ``fee_msats``. If this is the case these will be shown as ``N/A`` and not included in the totals summary at the end and in json mode the fields will be omitted
- pays: summars can't guarantee that it can find the ``destination``. If this is the case it will be shown as ``N/A`` and in json mode this field will be omitted
- pays: description field now included in json output mode if a description was found

### Fixed

- no longer panic on missing payment ``destination`` or ``amount_msat``

## [3.5.0] 2024-10-22

### Added

- Total values summary for forwards, pays and invoices table at the bottom in normal view and as ``totals`` object in json mode
- Show timeframe and limits you are currently using for each table of forwards, pays and invoices
- New options to limit output of forwards, pays and invoices tables: ``summars-forwards-limit``, ``summars-pays-limit`` and ``summars-invoices-limit``. Defaults to 0 (off) and limits the outputs to the last x entries, useful if you are setting high time values for ``summars-forwards``, ``summars-pays`` or ``summars-invoices``
- New ``summars-forwards-columns`` column: ``eff_fee_ppm`` showing the effective fee ppm

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
